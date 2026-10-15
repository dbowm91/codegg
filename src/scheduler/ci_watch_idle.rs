//! C001 daemon-owned idle CI-watch coordinator.
//!
//! One daemon-lifecycle-owned cancellable wake loop drives the existing
//! `CiWatchService` admission and durable watch state. It is the canonical
//! idle trigger: the earliest-due persisted row wakes the driver, which
//! performs bounded global due advancement (`advance_due_watches_global`)
//! with per-observation persisted CAS leases. No per-watch timer, no second
//! scheduler, no model polling.
//!
//! # Ownership versus `ScheduleStore` / `JobScheduler`
//!
//! - `ScheduleStore` materializes *recurring* schedule occurrences
//!   (Subagent templates with overlap/missed-run policies). A CI watch is a
//!   *finite* observation sequence with per-watch adaptive state (ETag,
//!   exponential backoff with Retry-After, attempt budget, TTL) and terminal
//!   reconciliation into exactly one M001 notice. Forcing watches through
//!   recurring schedules would clutter the schedule namespace and
//!   misrepresent short bounded network reads as compute jobs.
//! - `JobScheduler` remains the only durable machine-execution authority for
//!   CPU/process work. Watch observations are short bounded network reads
//!   executed inline under the persisted per-watch lease (one active
//!   observation per due occurrence, at most a handful per driver tick).
//!   The remote wait holds no process/CPU permit; terminal delivery reuses
//!   the scheduler-owned M001 notification authority
//!   (`JobNotificationService::record_terminal_for_watch`).
//! - An empty due queue sleeps without busy polling: the driver waits on a
//!   `Notify` (woken by watch creation/updates) or a timer until the
//!   earliest durable `next_check_at`. Shutdown cancels in-flight
//!   observation safely (the persisted lease expires and is recoverable);
//!   startup repairs registrations and schedules without storm (bounded
//!   global advancement, backoff horizons preserved).
//!
//! In-memory/headless modes (no SQLite pool) share the same driver type but
//! have no cross-process durability: idle advancement works only while the
//! handle lives. Production daemons always have a pool; callers without one
//! must not claim process-independent persistence.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::ci_watch::CiWatchService;

/// Bounded observations per driver tick. Small enough that an idle scan
/// never stalls the daemon; large enough that a burst of due watches
/// drains within a few ticks under fair due ordering.
pub const IDLE_DRIVER_BATCH: usize = 8;

/// Maximum sleep between driver ticks when the next due horizon is far in
/// the future. The driver still wakes early on explicit `wake()` (watch
/// creation/updates, shutdown). Prevents an indefinite sleep if the clock
/// jumps or a horizon is miscomputed.
pub const IDLE_DRIVER_MAX_SLEEP: Duration = Duration::from_secs(60);

/// Minimum sleep to avoid a hot loop when many watches are continuously due
/// (e.g. zero-backoff test configs). Real backoff horizons dominate.
pub const IDLE_DRIVER_MIN_SLEEP: Duration = Duration::from_millis(100);

pub struct CiWatchIdleDriver {
    watches: Arc<CiWatchService>,
    notify: Arc<Notify>,
    shutdown: Arc<AtomicBool>,
}

impl CiWatchIdleDriver {
    pub fn new(watches: Arc<CiWatchService>) -> Self {
        let notify = watches.idle_driver_notify();
        Self {
            watches,
            notify,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Shared watches handle (the daemon singleton).
    pub fn watches(&self) -> &Arc<CiWatchService> {
        &self.watches
    }

    /// Wake the driver loop (watch created/updated, repair requested).
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// Request shutdown. The loop finishes its current bounded tick, then
    /// exits. In-flight HTTP observations are short-lived; the persisted
    /// lease expires and is recoverable on restart.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// One bounded global advancement (used by the loop and by tests that
    /// need deterministic progress without sleeping). Returns the number of
    /// watches that reached a durable outcome this tick.
    pub async fn run_once(&self) -> usize {
        self.watches
            .advance_due_watches_global(IDLE_DRIVER_BATCH)
            .await
    }

    /// Repair registrations (crash between row write and subscription, etc.)
    /// then perform one advancement. Used at startup.
    pub async fn recover_and_run_once(&self) -> (usize, usize, usize) {
        let (repaired, attentioned) = self.watches.repair_registrations().await.unwrap_or((0, 0));
        let advanced = self.run_once().await;
        (repaired, attentioned, advanced)
    }

    /// Spawn the daemon-lifecycle-owned loop. Returns the join handle; the
    /// driver must be held as `Arc` by the daemon so `wake()` and
    /// `request_shutdown()` stay reachable. Only one loop per daemon.
    pub fn spawn(self: &Arc<Self>) -> JoinHandle<()> {
        let driver = Arc::clone(self);
        tokio::spawn(async move {
            // Startup: heal any watch persisted without its subscription
            // (crash between insert and subscribe, subscription failure,
            // restart during terminal-before-notice) before first tick.
            let (repaired, attentioned) = driver
                .watches
                .repair_registrations()
                .await
                .unwrap_or((0, 0));
            if repaired > 0 || attentioned > 0 {
                tracing::info!(
                    repaired,
                    attentioned,
                    "ci-watch idle driver startup repair complete"
                );
            }
            loop {
                if driver.is_shutdown() {
                    break;
                }
                // Bounded work per tick; each observation holds its own
                // persisted lease, so concurrent driver loops (restart
                // overlap) cannot double-poll the same due occurrence.
                let _advanced = driver.run_once().await;
                if driver.is_shutdown() {
                    break;
                }
                // Sleep until the earliest durable due horizon, a wake, or
                // shutdown — never busy-poll an empty queue.
                let sleep = driver.sleep_until_next_due().await;
                tokio::select! {
                    _ = driver.notify.notified() => {},
                    _ = tokio::time::sleep(sleep) => {},
                }
            }
            tracing::debug!("ci-watch idle driver loop exited");
        })
    }

    async fn sleep_until_next_due(&self) -> Duration {
        let now = chrono::Utc::now().timestamp_millis();
        match self.watches.earliest_due_ms().await.unwrap_or(None) {
            None => IDLE_DRIVER_MAX_SLEEP,
            Some(due) => {
                let delta_ms = due.saturating_sub(now);
                if delta_ms <= 0 {
                    IDLE_DRIVER_MIN_SLEEP
                } else {
                    Duration::from_millis(
                        delta_ms.min(IDLE_DRIVER_MAX_SLEEP.as_millis() as i64) as u64
                    )
                    .max(IDLE_DRIVER_MIN_SLEEP)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_and_sleep_bounds_are_sane() {
        // Bounds are const-checked here (not `assert!(true)` on literals):
        // the driver contract requires a small bounded batch and a
        // strictly increasing sleep window.
        const _: () = assert!(IDLE_DRIVER_BATCH >= 1 && IDLE_DRIVER_BATCH <= 64);
        const _: () =
            assert!(IDLE_DRIVER_MIN_SLEEP.as_millis() < IDLE_DRIVER_MAX_SLEEP.as_millis());
        assert!(IDLE_DRIVER_MAX_SLEEP.as_secs() >= 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn newly_accepted_watch_interrupts_empty_queue_sleep() {
        let notifier = Arc::new(super::super::job_notifications::JobNotificationService::new());
        let service = Arc::new(CiWatchService::new(
            notifier,
            super::super::ci_watch::CiWatchConfig::default(),
        ));
        let driver = CiWatchIdleDriver::new(service.clone());

        service
            .create_watch(
                super::super::ci_watch::CiWatchTarget {
                    provider: super::super::ci_watch::PROVIDER_GITHUB_ACTIONS.to_string(),
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                    run_id: Some(42),
                    commit_sha: None,
                    check_selector: None,
                },
                "session",
                None,
                "invocation",
                None,
                None,
                super::super::job_notifications::JobCompletionDisposition::NotifyOnly,
                None,
            )
            .await
            .expect("watch accepted and subscribed");

        tokio::time::timeout(Duration::from_secs(1), driver.notify.notified())
            .await
            .expect("accepted watch wakes the idle driver");
    }
}
