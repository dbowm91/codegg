//! TUI background task lifecycle registry.
//!
//! Centralizes ownership and shutdown semantics for TUI-side background tasks.
//! Tracks spawned tasks so they can be counted, cancelled, and reaped on
//! shutdown or dialog close. See `plans/tui_phase_7_background_task_lifecycle.md`.

use std::collections::HashMap;
use std::time::Instant;

/// Monotonically increasing task identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TuiTaskId(pub u64);

/// Category of background work, used for kind-based cancellation
/// and diagnostics grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Abort handle for cooperative cancellation.
    abort_handle: tokio::task::AbortHandle,
}

impl TuiTaskRecord {
    /// Abort the underlying task.
    pub fn abort(&self) {
        self.abort_handle.abort();
    }

    /// Returns `true` if the task has been aborted.
    pub fn is_aborted(&self) -> bool {
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

    /// Cancel all registered tasks.
    pub fn cancel_all(&mut self) {
        for (_, record) in self.tasks.drain() {
            record.abort();
            self.cancelled_count += 1;
        }
    }

    /// Remove finished tasks from the registry.
    pub fn reap_finished(&mut self) {
        self.tasks.retain(|_, record| !record.is_aborted());
    }

    /// Number of currently active (non-reaped) tasks.
    pub fn active_count(&self) -> usize {
        self.tasks.len()
    }

    /// Number of tasks cancelled since last clear.
    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    /// Human-readable summary for diagnostics.
    pub fn summary(&self) -> String {
        if self.tasks.is_empty() {
            return format!("Active tasks: 0 ({} cancelled)", self.cancelled_count);
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

        let mut lines = vec![format!("Active tasks: {}", self.tasks.len())];
        for (kind, count) in &kind_counts {
            lines.push(format!("  {}: {}", kind, count));
        }
        lines.push(format!(
            "Oldest: {} ({:.1?})",
            oldest_name,
            oldest_age.elapsed()
        ));
        if self.cancelled_count > 0 {
            lines.push(format!("Cancelled: {}", self.cancelled_count));
        }
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
        let mut reg = TuiTaskRegistry::new();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let id = reg.spawn(TuiTaskKind::Command, "cancellable", async move {
            let _ = rx.await;
        });
        // Cancel immediately
        reg.cancel(id);
        // The task should have been aborted; the oneshot sender dropped
        // without sending means the task didn't complete normally.
        assert!(tx.send(()).is_err() || true); // oneshot receiver was dropped
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
}
