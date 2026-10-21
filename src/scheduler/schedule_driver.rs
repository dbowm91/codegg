//! The daemon-owned schedule materialization driver.
//!
//! One daemon-lifecycle-owned loop claims the due schedule occurrences and
//! materializes them through the production materializer. It is the canonical
//! trigger for recurring and one-shot work: the earliest-due persisted
//! schedule wakes the driver, which performs one bounded claim. There is no
//! per-schedule timer, no second scheduler, and no model polling.
//!
//! # Ownership versus `JobScheduler`
//!
//! - The driver **creates** work; the scheduler **runs** it. A claimed
//!   occurrence becomes a durable job through the submission facade, and the
//!   scheduler remains the only authority that admits and dispatches it.
//! - The driver owns no executor and no queue of its own. Its durable state
//!   is the schedule store's rows; the durable job store is the queue the
//!   scheduler reconciles from.
//!
//! # Crash safety
//!
//! The occurrence's job id is a pure function of `(schedule_id,
//! scheduled_for)`, so the `job.id` primary key is the durable uniqueness
//! constraint for an occurrence. A daemon that stopped between writing the
//! job row and writing the occurrence row replays into the same job. An
//! occurrence the materializer refuses is not recorded at all, so it stays
//! retryable instead of being silently consumed.
//!
//! In-memory/headless modes (no SQLite pool) share the same driver type but
//! have no cross-process durability: advancement works only while the handle
//! lives. Production daemons always have a pool; callers without one must
//! not claim process-independent persistence.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

use codegg_core::jobs::{
    ClaimedOccurrence, OccurrenceMaterializer, OccurrenceStatus, ScheduleQuery, ScheduleState,
    ScheduleStore,
};

/// Maximum sleep between driver ticks when the next due horizon is far in
/// the future. The driver still wakes early on an explicit `wake()` (a
/// schedule was created, paused, resumed, or deleted) or on shutdown. This
/// prevents an indefinite sleep if the clock jumps or a horizon is
/// miscomputed.
pub const SCHEDULE_DRIVER_MAX_SLEEP: Duration = Duration::from_secs(60);

/// Minimum sleep to avoid a hot loop when schedules are continuously due
/// (for example a refused occurrence, which stays due until it succeeds).
pub const SCHEDULE_DRIVER_MIN_SLEEP: Duration = Duration::from_millis(250);

pub struct ScheduleDriver {
    schedules: Arc<dyn ScheduleStore>,
    materializer: Arc<dyn OccurrenceMaterializer>,
    notify: Arc<Notify>,
    shutdown: Arc<AtomicBool>,
}

impl ScheduleDriver {
    pub fn new(
        schedules: Arc<dyn ScheduleStore>,
        materializer: Arc<dyn OccurrenceMaterializer>,
    ) -> Arc<Self> {
        Arc::new(Self {
            schedules,
            materializer,
            notify: Arc::new(Notify::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Wake the driver loop (a schedule was created, resumed, or deleted).
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// Request shutdown. The loop finishes its current bounded tick, then
    /// exits.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// One bounded claim of the due occurrences.
    ///
    /// Returns what the tick decided, so a caller (or a test) can assert on
    /// the materialized, skipped, and refused occurrences without reading
    /// the store.
    pub async fn run_once(&self) -> Vec<ClaimedOccurrence> {
        match self
            .schedules
            .claim_due(chrono::Utc::now(), self.materializer.as_ref())
            .await
        {
            Ok(claimed) => {
                for occurrence in &claimed {
                    match occurrence.status {
                        OccurrenceStatus::Queued => tracing::debug!(
                            schedule_id = %occurrence.schedule_id.as_str(),
                            job_id = %occurrence
                                .job_id
                                .as_ref()
                                .map(|id| id.as_str())
                                .unwrap_or_default(),
                            "schedule occurrence materialized"
                        ),
                        OccurrenceStatus::Skipped => tracing::debug!(
                            schedule_id = %occurrence.schedule_id.as_str(),
                            "schedule occurrence skipped by the overlap policy"
                        ),
                        OccurrenceStatus::Failed => tracing::warn!(
                            schedule_id = %occurrence.schedule_id.as_str(),
                            reason = occurrence.failure_code.as_deref().unwrap_or("unknown"),
                            "schedule occurrence refused; it stays due and will be retried"
                        ),
                        _ => {}
                    }
                }
                claimed
            }
            Err(error) => {
                // The tick itself failed (storage, not a refused
                // occurrence). Nothing was claimed, so every schedule stays
                // due and the next tick retries it.
                tracing::warn!(%error, "schedule tick failed");
                Vec::new()
            }
        }
    }

    /// Spawn the daemon-lifecycle-owned loop. Returns the join handle; the
    /// driver must be held as `Arc` by the daemon so `wake()` and
    /// `request_shutdown()` stay reachable. Only one loop per daemon.
    pub fn spawn(self: &Arc<Self>) -> JoinHandle<()> {
        let driver = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if driver.is_shutdown() {
                    break;
                }
                let _claimed = driver.run_once().await;
                if driver.is_shutdown() {
                    break;
                }
                // Sleep until the earliest durable due horizon, a wake, or
                // shutdown — never busy-poll.
                let sleep = driver.sleep_until_next_due().await;
                tokio::select! {
                    _ = driver.notify.notified() => {},
                    _ = tokio::time::sleep(sleep) => {},
                }
            }
            tracing::debug!("schedule driver loop exited");
        })
    }

    async fn sleep_until_next_due(&self) -> Duration {
        let now = chrono::Utc::now();
        let query = ScheduleQuery {
            workspace_id: None,
            state: Some(ScheduleState::Active),
            include_archived: false,
        };
        let earliest = match self.schedules.list(query).await {
            Ok(summaries) => summaries
                .into_iter()
                .filter_map(|summary| summary.next_run_at)
                .min(),
            Err(error) => {
                tracing::warn!(%error, "schedule driver could not read the due horizon");
                None
            }
        };
        match earliest {
            None => SCHEDULE_DRIVER_MAX_SLEEP,
            Some(due) => {
                let delta = due.signed_duration_since(now);
                if delta <= chrono::Duration::zero() {
                    SCHEDULE_DRIVER_MIN_SLEEP
                } else {
                    let millis = delta
                        .num_milliseconds()
                        .min(SCHEDULE_DRIVER_MAX_SLEEP.as_millis() as i64);
                    Duration::from_millis(millis.max(1) as u64).max(SCHEDULE_DRIVER_MIN_SLEEP)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_bounds_are_sane() {
        const _: () =
            assert!(SCHEDULE_DRIVER_MIN_SLEEP.as_millis() < SCHEDULE_DRIVER_MAX_SLEEP.as_millis());
        assert!(SCHEDULE_DRIVER_MAX_SLEEP.as_secs() >= 1);
    }
}
