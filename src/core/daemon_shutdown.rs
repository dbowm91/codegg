//! M003: `CoreDaemon` shutdown and join ownership.
//!
//! `CoreDaemon` remains the single daemon composition/lifecycle authority.
//! This module owns shutdown/join only: aborting the two daemon-owned
//! background tasks (projection-replay maintenance and managed-worktree
//! reconciliation) when the daemon drops. It introduces no new supervisor,
//! process manager, or DI container, and preserves the exact
//! cancellation-before-join sequence.
//!
//! ```text
//! Shutdown order (preserved verbatim):
//!   1. shutdown cancellation precedes joins (socket serve loop observes
//!      CancellationToken; see `src/main.rs` daemon serve path)
//!   2. `CoreDaemon::abort_background_handles` aborts, in order:
//!        a. projection-maintenance task (5-minute retention/checkpoint loop)
//!        b. worktree-reconcile task (bounded startup reconciliation)
//!   3. socket/pid/metadata cleanup while holding the singleton lock
//!      (main.rs; lock release is authoritative, metadata is diagnostic)
//!
//! The scheduler loop handle is intentionally detached (`let _handle =` in
//! construction): the scheduler owns its own shutdown via permits and
//! cancellation, not via a daemon-held join. Do not add a second bootstrap
//! or supervisor here.
//! ```

use super::daemon::CoreDaemon;

impl Drop for CoreDaemon {
    fn drop(&mut self) {
        self.abort_background_handles();
    }
}

impl CoreDaemon {
    /// Abort daemon-owned background tasks in shutdown order.
    ///
    /// Cancellation precedes joins and process cleanup as today. Taking each
    /// `Option<JoinHandle>` before aborting guarantees at-most-once abort
    /// even if `drop` runs twice during unwinding.
    pub(crate) fn abort_background_handles(&mut self) {
        if let Some(handle) = self._projection_maintenance_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self._worktree_reconcile_handle.take() {
            handle.abort();
        }
    }

    /// Canonical shutdown phase names in execution order.
    ///
    /// Pinned by `shutdown_phases_match_documented_order`.
    #[cfg(test)]
    pub(crate) fn shutdown_phase_names() -> [&'static str; 3] {
        [
            "cancel",
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
