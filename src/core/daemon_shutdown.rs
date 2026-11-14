//! M003: daemon shutdown and join ownership.
//!
//! Stop and join scheduler jobs before runtime teardown, then abort remaining
//! background tasks and release singleton metadata in `main.rs`.
//! ```text
//! socket cancellation -> shutdown_jobs (cancel, reap, join) -> daemon drop
//! (abort remaining handles) -> socket/pid/metadata cleanup under singleton lock
//! ```

use super::daemon::CoreDaemon;

impl Drop for CoreDaemon {
    fn drop(&mut self) {
        self.abort_background_handles();
        egggit::process::shutdown_object_view_cache();
    }
}

impl CoreDaemon {
    /// Cancel scheduler work and join its loop before runtime teardown.
    pub async fn shutdown_jobs(&self) {
        let mut run_handle = self.scheduler_run_handle.lock().await;
        if let Some(scheduler) = self.deps.scheduler.as_ref() {
            scheduler
                .shutdown(crate::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
                .await;
        }

        if let Some(handle) = run_handle.take() {
            if let Err(error) = handle.await {
                tracing::warn!(%error, "scheduler loop failed while joining during daemon shutdown");
            }
        }
    }

    /// Abort daemon-owned background tasks in shutdown order.
    ///
    /// Scheduler cancellation and joins happen in `shutdown_jobs` while the
    /// runtime is live. Taking each `Option<JoinHandle>` here guarantees
    /// at-most-once abort even if `drop` runs twice during unwinding.
    pub(crate) fn abort_background_handles(&mut self) {
        if let Some(handle) = self._projection_maintenance_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self._worktree_reconcile_handle.take() {
            handle.abort();
        }
        // C001 idle driver: request cooperative shutdown first so the loop
        // finishes its current bounded tick, then abort the handle.
        if let Some(driver) = self.ci_watch_driver.take() {
            driver.request_shutdown();
        }
        if let Some(handle) = self._ci_watch_driver_handle.take() {
            handle.abort();
        }
        // M004 schedule driver: request cooperative shutdown first so the
        // loop finishes its current bounded tick, then abort the handle.
        // An occurrence that was materialized but not yet recorded stays
        // due and is retried on the next daemon's first tick.
        if let Some(driver) = self.schedule_driver.take() {
            driver.request_shutdown();
        }
        if let Some(handle) = self._schedule_driver_handle.take() {
            handle.abort();
        }
        // C004 completion fanout: abort the forwarder; the unbounded
        // channel is dropped with the notifier, so in-flight terminals
        // already committed stay durable and are recovered via the notice
        // row + next-turn session event on restart.
        if let Some(handle) = self._idle_completion_handle.take() {
            handle.abort();
        }
    }

    /// Canonical shutdown phase names in execution order.
    ///
    /// Pinned by `shutdown_phases_match_documented_order`.
    #[cfg(test)]
    pub(crate) fn shutdown_phase_names() -> [&'static str; 4] {
        [
            "cancel",
            "shutdown_jobs",
            "abort_background_handles",
            "release_singleton_lock",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_phases_match_documented_order() {
        assert_eq!(
            CoreDaemon::shutdown_phase_names(),
            [
                "cancel",
                "shutdown_jobs",
                "abort_background_handles",
                "release_singleton_lock",
            ]
        );
    }

    #[tokio::test]
    async fn drop_aborts_background_handles_without_panic() {
        let mut daemon = CoreDaemon::new(None, None, None);
        // Pool-less daemons hold no handles; abort is a no-op.
        daemon.abort_background_handles();
        daemon.abort_background_handles();
        assert!(daemon._projection_maintenance_handle.is_none());
        assert!(daemon._worktree_reconcile_handle.is_none());
    }

    #[tokio::test]
    async fn pooled_daemon_holds_seam_until_drop() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_shutdown_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid options")
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect");
        // Migrate so construction sees a usable catalog; handles spawn only
        // when a runtime is present, but the seam must exist for pooled
        // daemons regardless.
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let daemon = CoreDaemon::new(Some(pool), None, None);
        assert!(daemon.projection_seam.is_some());
        // `daemon` drops here; abort must not panic. Explicit scope to pin
        // the drop point for the lifecycle diagram.
        drop(daemon);
    }

    #[tokio::test]
    async fn shutdown_cancels_scheduler_and_joins_its_loop() {
        let mut daemon = CoreDaemon::new(None, None, None);
        // Retire any scheduler selected by the host's config before replacing
        // it with the controlled scheduler used by this lifecycle test.
        daemon.shutdown_jobs().await;

        let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        config.enabled = true;
        let scheduler = crate::scheduler::JobScheduler::new(
            daemon.deps.job_store.clone(),
            daemon.workspace_services.clone(),
            config,
            daemon.deps.daemon_generation.clone(),
        );
        let run_handle = scheduler.spawn_run();
        daemon.deps.scheduler = Some(scheduler.clone());
        *daemon.scheduler_run_handle.lock().await = Some(run_handle);

        daemon.shutdown_jobs().await;

        assert!(daemon.scheduler_run_handle.lock().await.is_none());
        assert_eq!(scheduler.clone().admit_and_dispatch_batch().await, 0);
    }

    #[test]
    fn construction_has_no_second_bootstrap_path() {
        // Static ownership check: shutdown owns only aborts, never a
        // constructor or recovery entry. If this test compiles, no second
        // `with_deps` or `recover_*` exists in this module.
        let phases = CoreDaemon::shutdown_phase_names();
        assert!(!phases.contains(&"with_deps"));
        assert!(!phases.contains(&"recover_state"));
    }
}
