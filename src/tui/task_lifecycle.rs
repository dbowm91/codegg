//! TUI background task lifecycle registry.
//!
//! Centralizes ownership and shutdown semantics for TUI-side background tasks.
//! Tracks spawned tasks so they can be counted, cancelled, and reaped on
//! shutdown or dialog close. See `plans/tui_phase_7_background_task_lifecycle.md`.
//!
//! ## Cancellation semantics
//!
//! Cancellation is **abort-based**, not cooperative. When a task is cancelled
//! via [`cancel`], [`cancel_kind`], or [`cancel_all`], the registry calls
//! `tokio::task::AbortHandle::abort` on the underlying task. Tasks must not
//! assume a cancellation token is plumbed through. Long-running work that owns
//! external resources should put cleanup in `Drop` guards or use cancellation-
//! aware inner operations before registering with this abort-based registry.
//!
//! ## Outcome accounting
//!
//! The registry tracks three lifetime counters:
//! - [`completed_count`]: tasks that finished naturally before being reaped
//! - [`cancelled_count`]: tasks aborted via the registry's cancellation API
//! - [`panicked_count`]: tasks whose result was `Err(JoinError::Panic)` when
//!   last observed. Note: detecting panics requires awaiting the JoinHandle;
//!   the abort-handle-only design cannot observe them, so this counter stays
//!   at zero unless the registry is later upgraded to store JoinHandles.

use std::collections::HashMap;
use std::time::Instant;

/// Monotonically increasing task identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TuiTaskId(pub u64);

/// Category of background work, used for kind-based cancellation
/// and diagnostics grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TuiTaskKind {
    /// Async command completions (spawn_tui_task based).
    Command,
    /// Sidebar file-diff background computation.
    FileDiff,
    /// Human shell command execution.
    Shell,
    /// Research loading operations.
    Research,
    /// Memory operations.
    Memory,
    /// Notification dispatch.
    Notification,
    /// Security review background dispatch.
    SecurityReview,
    /// File indexer / indexed-file refresh.
    Indexer,
    /// Catch-all for miscellaneous background work.
    Other,
    /// Git sidebar background refresh.
    GitStatus,
}

impl std::fmt::Display for TuiTaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command => write!(f, "Command"),
            Self::FileDiff => write!(f, "FileDiff"),
            Self::Shell => write!(f, "Shell"),
            Self::Research => write!(f, "Research"),
            Self::Memory => write!(f, "Memory"),
            Self::Notification => write!(f, "Notification"),
            Self::SecurityReview => write!(f, "SecurityReview"),
            Self::Indexer => write!(f, "Indexer"),
            Self::Other => write!(f, "Other"),
            Self::GitStatus => write!(f, "GitStatus"),
        }
    }
}

/// Record of a single tracked background task.
pub struct TuiTaskRecord {
    /// Human-readable name for diagnostics (leaked `&'static str`).
    pub name: &'static str,
    /// Category of work.
    pub kind: TuiTaskKind,
    /// When the task was spawned.
    pub started_at: Instant,
    /// Abort handle used for registry-owned cancellation.
    abort_handle: tokio::task::AbortHandle,
    /// Optional frontend-local tab id this task is scoped to.
    /// `None` for genuinely global work.
    pub scope_tab_id: Option<String>,
    /// Optional daemon-typed session id this task is scoped to.
    /// `None` for tab-only or global scope.
    pub scope_session_id: Option<String>,
    /// Optional active-view epoch captured at spawn time. Used to
    /// identify tasks that became stale after a switch/close.
    pub scope_active_view_epoch: Option<u64>,
}

impl TuiTaskRecord {
    /// Abort the underlying task.
    pub fn abort(&self) {
        self.abort_handle.abort();
    }

    /// Returns `true` if the task has finished (either naturally or
    /// because it was aborted). Use [`reap_finished`](TuiTaskRegistry::reap_finished)
    /// to drop finished tasks from the registry.
    pub fn is_finished(&self) -> bool {
        self.abort_handle.is_finished()
    }
}

/// Registry of TUI-owned background tasks.
///
/// Lives on [`App`](super::app::App) and is consulted during shutdown, dialog
/// close, and `/tui-stats` reporting.  Tasks are registered when spawned and
/// reaped periodically or on demand.
pub struct TuiTaskRegistry {
    next_id: u64,
    tasks: HashMap<TuiTaskId, TuiTaskRecord>,
    /// Cumulative count of tasks cancelled via [`cancel`] / [`cancel_kind`] /
    /// [`cancel_all`].
    cancelled_count: u64,
    /// Cumulative count of tasks that finished naturally before being
    /// reaped. Bumped by [`reap_finished`] when a finished task is removed
    /// from the active map.
    completed_count: u64,
    /// Cumulative count of tasks observed to have panicked. The
    /// abort-handle-only design cannot observe panics; this counter is
    /// reserved for a future JoinHandle upgrade and stays at 0 in the
    /// current implementation.
    panicked_count: u64,
}

impl Default for TuiTaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiTaskRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            tasks: HashMap::new(),
            cancelled_count: 0,
            completed_count: 0,
            panicked_count: 0,
        }
    }

    /// Register a future as a tracked background task.
    ///
    /// The future is spawned onto the Tokio runtime immediately.  Returns a
    /// [`TuiTaskId`] that can be used to cancel or query the task.
    pub fn spawn<F>(&mut self, kind: TuiTaskKind, name: &'static str, fut: F) -> TuiTaskId
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.spawn_with_scope(kind, name, None, None, None, fut)
    }

    /// Register a future as a tracked background task with explicit
    /// scope. The scope fields are stored alongside the task so it can
    /// later be cancelled by `cancel_for_tab`, `cancel_for_session`, or
    /// `cancel_for_epoch`.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_scope<F>(
        &mut self,
        kind: TuiTaskKind,
        name: &'static str,
        scope_tab_id: Option<String>,
        scope_session_id: Option<String>,
        scope_active_view_epoch: Option<u64>,
        fut: F,
    ) -> TuiTaskId
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = TuiTaskId(self.next_id);
        self.next_id += 1;

        let handle = tokio::spawn(fut);
        self.tasks.insert(
            id,
            TuiTaskRecord {
                name,
                kind,
                started_at: Instant::now(),
                abort_handle: handle.abort_handle(),
                scope_tab_id,
                scope_session_id,
                scope_active_view_epoch,
            },
        );
        id
    }

    /// Cancel a specific task by id.
    ///
    /// Returns `true` if the task was found and aborted.
    pub fn cancel(&mut self, id: TuiTaskId) -> bool {
        if let Some(record) = self.tasks.remove(&id) {
            record.abort();
            self.cancelled_count += 1;
            true
        } else {
            false
        }
    }

    /// Cancel all tasks matching `kind`.
    pub fn cancel_kind(&mut self, kind: TuiTaskKind) {
        let to_cancel: Vec<TuiTaskId> = self
            .tasks
            .iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(&id, _)| id)
            .collect();
        for id in to_cancel {
            if let Some(record) = self.tasks.remove(&id) {
                record.abort();
                self.cancelled_count += 1;
            }
        }
    }

    /// Cancel all tasks scoped to the given tab id. Used when a tab
    /// closes or session rebinds. Returns the number of tasks cancelled.
    pub fn cancel_for_tab(&mut self, tab_id: &str) -> usize {
        let to_cancel: Vec<TuiTaskId> = self
            .tasks
            .iter()
            .filter(|(_, r)| r.scope_tab_id.as_deref() == Some(tab_id))
            .map(|(&id, _)| id)
            .collect();
        let count = to_cancel.len();
        for id in to_cancel {
            if let Some(record) = self.tasks.remove(&id) {
                record.abort();
                self.cancelled_count += 1;
            }
        }
        count
    }

    /// Cancel all tasks scoped to the given session id. Used when a
    /// session rebinds or closes. Returns the number of tasks cancelled.
    pub fn cancel_for_session(&mut self, session_id: &str) -> usize {
        let to_cancel: Vec<TuiTaskId> = self
            .tasks
            .iter()
            .filter(|(_, r)| r.scope_session_id.as_deref() == Some(session_id))
            .map(|(&id, _)| id)
            .collect();
        let count = to_cancel.len();
        for id in to_cancel {
            if let Some(record) = self.tasks.remove(&id) {
                record.abort();
                self.cancelled_count += 1;
            }
        }
        count
    }

    /// Cancel all tasks whose captured active-view epoch is strictly
    /// less than `current_epoch`. Used after a tab switch bumps the
    /// epoch so in-flight loads and stale completions are aborted.
    /// Returns the number of tasks cancelled.
    pub fn cancel_for_stale_epoch(&mut self, current_epoch: u64) -> usize {
        let to_cancel: Vec<TuiTaskId> = self
            .tasks
            .iter()
            .filter(|(_, r)| match r.scope_active_view_epoch {
                Some(captured) => captured < current_epoch,
                None => false,
            })
            .map(|(&id, _)| id)
            .collect();
        let count = to_cancel.len();
        for id in to_cancel {
            if let Some(record) = self.tasks.remove(&id) {
                record.abort();
                self.cancelled_count += 1;
            }
        }
        count
    }

    /// Cancel all registered tasks.
    pub fn cancel_all(&mut self) {
        for (_, record) in self.tasks.drain() {
            record.abort();
            self.cancelled_count += 1;
        }
    }

    /// Remove finished tasks from the registry and bump the completed
    /// counter for each one that finished naturally (i.e. was not
    /// previously cancelled). Calling this periodically keeps the
    /// active count from drifting upward across long sessions.
    pub fn reap_finished(&mut self) {
        let before = self.tasks.len();
        self.tasks.retain(|_, record| !record.is_finished());
        let removed = before.saturating_sub(self.tasks.len());
        self.completed_count = self.completed_count.saturating_add(removed as u64);
    }

    /// Number of currently active (non-reaped) tasks.
    pub fn active_count(&self) -> usize {
        self.tasks.len()
    }

    /// Number of tasks cancelled since last clear.
    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    /// Number of tasks that finished naturally before being reaped.
    pub fn completed_count(&self) -> u64 {
        self.completed_count
    }

    /// Number of tasks observed to have panicked. Reserved for a
    /// future JoinHandle upgrade; always 0 in the current design.
    pub fn panicked_count(&self) -> u64 {
        self.panicked_count
    }

    /// Human-readable summary for diagnostics.
    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Active tasks: {} (completed: {}, cancelled: {}, panicked: {})",
            self.tasks.len(),
            self.completed_count,
            self.cancelled_count,
            self.panicked_count,
        )];

        if self.tasks.is_empty() {
            return lines.join("\n");
        }

        // Count by kind
        let mut kind_counts: HashMap<TuiTaskKind, usize> = HashMap::new();
        let mut oldest_name = "";
        let mut oldest_age = Instant::now();

        for record in self.tasks.values() {
            *kind_counts.entry(record.kind).or_insert(0) += 1;
            if record.started_at < oldest_age {
                oldest_age = record.started_at;
                oldest_name = record.name;
            }
        }

        let mut kind_counts: Vec<_> = kind_counts.into_iter().collect();
        kind_counts.sort_by_key(|(kind, _)| *kind);
        for (kind, count) in kind_counts {
            lines.push(format!("  {}: {}", kind, count));
        }
        lines.push(format!(
            "Oldest: {} ({:.1?})",
            oldest_name,
            oldest_age.elapsed()
        ));
        lines.join("\n")
    }

    /// Iterate over active task records (for diagnostics / testing).
    pub fn iter(&self) -> impl Iterator<Item = (TuiTaskId, &TuiTaskRecord)> {
        self.tasks.iter().map(|(&id, record)| (id, record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = TuiTaskRegistry::new();
        assert_eq!(reg.active_count(), 0);
        assert_eq!(reg.cancelled_count(), 0);
    }

    #[tokio::test]
    async fn spawn_increments_active_count() {
        let mut reg = TuiTaskRegistry::new();
        let _id = reg.spawn(TuiTaskKind::Command, "test", async {});
        assert_eq!(reg.active_count(), 1);
        // Give the task time to complete
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn summary_orders_task_kinds_deterministically() {
        let mut reg = TuiTaskRegistry::new();
        let _command = reg.spawn(TuiTaskKind::Command, "command", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let _shell = reg.spawn(TuiTaskKind::Shell, "shell", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        let summary = reg.summary();
        assert!(summary.find("Command: 1") < summary.find("Shell: 1"));
        reg.cancel_all();
    }

    #[tokio::test]
    async fn reap_finished_removes_completed_tasks() {
        let mut reg = TuiTaskRegistry::new();
        let _id = reg.spawn(TuiTaskKind::Command, "test", async {});
        // Wait for the task to finish
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
    }

    #[tokio::test]
    async fn cancel_aborts_task() {
        let mut reg = TuiTaskRegistry::new();
        let id = reg.spawn(TuiTaskKind::Command, "long_task", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        assert_eq!(reg.active_count(), 1);
        let cancelled = reg.cancel(id);
        assert!(cancelled);
        assert_eq!(reg.cancelled_count(), 1);
        // After reap, task is removed
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
    }

    #[tokio::test]
    async fn cancel_nonexistent_returns_false() {
        let mut reg = TuiTaskRegistry::new();
        let fake_id = TuiTaskId(999);
        assert!(!reg.cancel(fake_id));
    }

    #[tokio::test]
    async fn cancel_kind_aborts_matching_tasks() {
        let mut reg = TuiTaskRegistry::new();
        let _id1 = reg.spawn(TuiTaskKind::Command, "cmd1", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let _id2 = reg.spawn(TuiTaskKind::FileDiff, "diff1", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let _id3 = reg.spawn(TuiTaskKind::Command, "cmd2", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        assert_eq!(reg.active_count(), 3);
        reg.cancel_kind(TuiTaskKind::Command);
        assert_eq!(reg.cancelled_count(), 2);
        // FileDiff task should still be active
        reg.reap_finished();
        assert_eq!(reg.active_count(), 1);
    }

    #[tokio::test]
    async fn cancel_all_aborts_everything() {
        let mut reg = TuiTaskRegistry::new();
        let _id1 = reg.spawn(TuiTaskKind::Command, "cmd1", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let _id2 = reg.spawn(TuiTaskKind::Shell, "shell1", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        assert_eq!(reg.active_count(), 2);
        reg.cancel_all();
        assert_eq!(reg.cancelled_count(), 2);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
    }

    #[tokio::test]
    async fn cancellation_token_cancellation_is_observed() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };

        struct DropFlag(Arc<AtomicBool>);

        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let mut reg = TuiTaskRegistry::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_in_task = Arc::clone(&dropped);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let id = reg.spawn(TuiTaskKind::Command, "cancellable", async move {
            let _drop_flag = DropFlag(dropped_in_task);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        reg.cancel(id);

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(
            dropped.load(Ordering::SeqCst),
            "aborting a registered task should drop its future"
        );
    }

    #[tokio::test]
    async fn completed_task_is_reaped_cleanly() {
        let mut reg = TuiTaskRegistry::new();
        let id = reg.spawn(TuiTaskKind::Command, "fast", async {});
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
        // Cancelling a reaped (nonexistent) task returns false
        assert!(!reg.cancel(id));
    }

    #[tokio::test]
    async fn completed_count_increments_on_reap() {
        let mut reg = TuiTaskRegistry::new();
        reg.spawn(TuiTaskKind::Command, "fast1", async {});
        reg.spawn(TuiTaskKind::Command, "fast2", async {});
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(reg.active_count(), 2);
        assert_eq!(reg.completed_count(), 0);
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
        assert_eq!(reg.completed_count(), 2);
    }

    #[tokio::test]
    async fn reap_skips_still_running_tasks() {
        let mut reg = TuiTaskRegistry::new();
        let _id = reg.spawn(TuiTaskKind::Command, "slow", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        reg.reap_finished();
        // Task is still running, must not be reaped
        assert_eq!(reg.active_count(), 1);
        assert_eq!(reg.completed_count(), 0);
    }

    #[tokio::test]
    async fn cancelled_tasks_bump_cancelled_not_completed() {
        let mut reg = TuiTaskRegistry::new();
        let id = reg.spawn(TuiTaskKind::Command, "to_cancel", async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        reg.cancel(id);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
        assert_eq!(reg.cancelled_count(), 1);
        assert_eq!(reg.completed_count(), 0);
    }

    #[tokio::test]
    async fn panicked_count_stays_zero_in_abort_handle_design() {
        let mut reg = TuiTaskRegistry::new();
        reg.spawn(TuiTaskKind::Command, "panic", async {
            // Panic — but with abort-handle-only design we cannot observe.
            panic!("intentional test panic");
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        reg.reap_finished();
        // The task panicked but the registry cannot detect that yet.
        assert_eq!(reg.panicked_count(), 0);
    }

    #[test]
    fn summary_includes_all_counters() {
        let reg = TuiTaskRegistry::new();
        let s = reg.summary();
        assert!(s.contains("Active tasks: 0"));
        assert!(s.contains("completed: 0"));
        assert!(s.contains("cancelled: 0"));
        assert!(s.contains("panicked: 0"));
    }

    #[tokio::test]
    async fn summary_after_completion_shows_completed_count() {
        let mut reg = TuiTaskRegistry::new();
        reg.spawn(TuiTaskKind::Command, "fast", async {});
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        reg.reap_finished();
        let s = reg.summary();
        assert!(s.contains("completed: 1"));
        assert!(s.contains("cancelled: 0"));
    }

    #[test]
    fn summary_empty() {
        let reg = TuiTaskRegistry::new();
        let s = reg.summary();
        assert!(s.contains("Active tasks: 0"));
    }

    #[tokio::test]
    async fn summary_with_tasks() {
        let mut reg = TuiTaskRegistry::new();
        let _id1 = reg.spawn(TuiTaskKind::Command, "cmd1", async {});
        let _id2 = reg.spawn(TuiTaskKind::FileDiff, "diff1", async {});
        let s = reg.summary();
        assert!(s.contains("Active tasks: 2"));
        assert!(s.contains("Command: 1"));
        assert!(s.contains("FileDiff: 1"));
        assert!(s.contains("Oldest:"));
    }

    #[test]
    fn kind_display() {
        assert_eq!(TuiTaskKind::Command.to_string(), "Command");
        assert_eq!(TuiTaskKind::FileDiff.to_string(), "FileDiff");
        assert_eq!(TuiTaskKind::Shell.to_string(), "Shell");
        assert_eq!(TuiTaskKind::Research.to_string(), "Research");
        assert_eq!(TuiTaskKind::Memory.to_string(), "Memory");
        assert_eq!(TuiTaskKind::Notification.to_string(), "Notification");
        assert_eq!(TuiTaskKind::SecurityReview.to_string(), "SecurityReview");
        assert_eq!(TuiTaskKind::Indexer.to_string(), "Indexer");
        assert_eq!(TuiTaskKind::Other.to_string(), "Other");
    }

    #[tokio::test]
    async fn cancel_for_tab_only_aborts_scoped_tasks() {
        let mut reg = TuiTaskRegistry::new();
        let tab_a: Option<String> = Some("tab-a".into());
        let none: Option<String> = None;
        let _a = reg.spawn_with_scope(
            TuiTaskKind::Command,
            "a",
            tab_a.clone(),
            None,
            None,
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            },
        );
        let _b = reg.spawn_with_scope(
            TuiTaskKind::Command,
            "b",
            tab_a.clone(),
            None,
            None,
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            },
        );
        let _global =
            reg.spawn_with_scope(TuiTaskKind::Command, "global", none, None, None, async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            });
        assert_eq!(reg.active_count(), 3);
        let cancelled = reg.cancel_for_tab("tab-a");
        assert_eq!(cancelled, 2);
        assert_eq!(reg.cancelled_count(), 2);
        reg.reap_finished();
        assert_eq!(reg.active_count(), 1);
    }

    #[tokio::test]
    async fn cancel_for_session_only_aborts_scoped_tasks() {
        let mut reg = TuiTaskRegistry::new();
        let _a = reg.spawn_with_scope(
            TuiTaskKind::Command,
            "a",
            None,
            Some("s-1".into()),
            None,
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            },
        );
        let _b = reg.spawn_with_scope(
            TuiTaskKind::Command,
            "b",
            None,
            Some("s-2".into()),
            None,
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            },
        );
        let cancelled = reg.cancel_for_session("s-1");
        assert_eq!(cancelled, 1);
    }

    #[tokio::test]
    async fn cancel_for_stale_epoch_aborts_only_older() {
        let mut reg = TuiTaskRegistry::new();
        let _a = reg.spawn_with_scope(TuiTaskKind::Command, "old", None, None, Some(1), async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let _b = reg.spawn_with_scope(TuiTaskKind::Command, "new", None, None, Some(5), async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let _global =
            reg.spawn_with_scope(TuiTaskKind::Command, "global", None, None, None, async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            });
        let cancelled = reg.cancel_for_stale_epoch(5);
        // Only the "old" task (epoch 1) should be cancelled; "new" at
        // epoch 5 is at the boundary and stays. Tasks with no epoch
        // are never cancelled by this method.
        assert_eq!(cancelled, 1);
        reg.reap_finished();
        assert_eq!(reg.active_count(), 2);
    }
}
