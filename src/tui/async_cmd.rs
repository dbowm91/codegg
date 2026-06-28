//! Async command helpers for spawning TUI background tasks.
//!
//! Provides [`spawn_tui_task`] for moving high-latency core-backed work
//! off the event loop while keeping all UI state mutation on the event loop
//! through typed completion commands.
//!
//! [`spawn_registered_tui_task`] extends this with task lifecycle tracking
//! via [`TuiTaskRegistry`](super::task_lifecycle::TuiTaskRegistry).

use super::app::TuiCommand;
use super::task_lifecycle::{TuiTaskKind, TuiTaskRegistry};
use tokio::sync::mpsc;

/// Spawn a Tokio task that performs async work and sends a completion
/// [`TuiCommand`] back to the event loop.
///
/// If `tx` is `None` (no command sender available), the task is not spawned
/// and a warning is logged. If the receiver is gone when the task completes,
/// the completion is silently dropped.
pub fn spawn_tui_task<F>(tx: Option<mpsc::Sender<TuiCommand>>, name: &'static str, fut: F)
where
    F: std::future::Future<Output = Option<TuiCommand>> + Send + 'static,
{
    let Some(tx) = tx else {
        tracing::warn!(task = name, "cannot spawn TUI task without command sender");
        return;
    };
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let result = fut.await;
        tracing::debug!(
            task = name,
            elapsed_ms = started.elapsed().as_millis(),
            "TUI async task finished"
        );
        if let Some(cmd) = result {
            if tx.send(cmd).await.is_err() {
                tracing::debug!(task = name, "TUI task completion dropped: receiver gone");
            }
        }
    });
}

/// Spawn a tracked background task that performs async work and sends a
/// completion [`TuiCommand`] back to the event loop.
///
/// Like [`spawn_tui_task`] but registers the task in the given
/// [`TuiTaskRegistry`] so it can be counted, cancelled, and reaped.
///
/// Returns a [`TuiTaskId`](super::task_lifecycle::TuiTaskId) for the
/// registered task.  Returns `None` if no command sender is available.
pub fn spawn_registered_tui_task<F>(
    tx: Option<mpsc::Sender<TuiCommand>>,
    registry: &mut TuiTaskRegistry,
    kind: TuiTaskKind,
    name: &'static str,
    fut: F,
) -> Option<super::task_lifecycle::TuiTaskId>
where
    F: std::future::Future<Output = Option<TuiCommand>> + Send + 'static,
{
    let Some(tx) = tx else {
        tracing::warn!(task = name, "cannot spawn TUI task without command sender");
        return None;
    };
    let id = registry.spawn(kind, name, async move {
        let started = std::time::Instant::now();
        let result = fut.await;
        tracing::debug!(
            task = name,
            elapsed_ms = started.elapsed().as_millis(),
            "TUI registered task finished"
        );
        if let Some(cmd) = result {
            if tx.send(cmd).await.is_err() {
                tracing::debug!(task = name, "TUI task completion dropped: receiver gone");
            }
        }
    });
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::TuiCommand;

    #[tokio::test]
    async fn spawn_helper_sends_completion() {
        let (tx, mut rx) = mpsc::channel(10);
        let handle = tokio::spawn(async move {
            spawn_tui_task(Some(tx), "test_task", async {
                Some(TuiCommand::ReloadSessions)
            });
        });
        handle.await.unwrap();
        // Give the task a moment to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let cmd = rx.try_recv();
        assert!(cmd.is_ok(), "should receive completion command");
        assert!(matches!(cmd.unwrap(), TuiCommand::ReloadSessions));
    }

    #[tokio::test]
    async fn spawn_helper_drops_when_receiver_gone() {
        let (tx, rx) = mpsc::channel(10);
        drop(rx);
        // Should not panic
        spawn_tui_task(Some(tx), "test_drop", async {
            Some(TuiCommand::ReloadSessions)
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // No panic = success
    }

    #[tokio::test]
    async fn spawn_helper_no_sender_does_not_panic() {
        spawn_tui_task(None, "test_no_sender", async {
            Some(TuiCommand::ReloadSessions)
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // No panic = success
    }

    #[tokio::test]
    async fn spawn_helper_none_result_is_noop() {
        let (tx, mut rx) = mpsc::channel(10);
        spawn_tui_task(Some(tx), "test_none", async { None });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(rx.try_recv().is_err(), "should not receive anything");
    }

    #[tokio::test]
    async fn spawn_registered_sends_completion_and_registers() {
        let (tx, mut rx) = mpsc::channel(10);
        let mut reg = TuiTaskRegistry::new();
        let id = spawn_registered_tui_task(
            Some(tx),
            &mut reg,
            TuiTaskKind::Command,
            "reg_test",
            async { Some(TuiCommand::ReloadSessions) },
        );
        assert!(id.is_some(), "should return task id");
        assert_eq!(reg.active_count(), 1);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let cmd = rx.try_recv();
        assert!(cmd.is_ok(), "should receive completion command");
        assert!(matches!(cmd.unwrap(), TuiCommand::ReloadSessions));
        // Task should be reaped now
        reg.reap_finished();
        assert_eq!(reg.active_count(), 0);
    }

    #[tokio::test]
    async fn spawn_registered_no_sender_returns_none() {
        let mut reg = TuiTaskRegistry::new();
        let id =
            spawn_registered_tui_task(None, &mut reg, TuiTaskKind::Command, "no_sender", async {
                Some(TuiCommand::ReloadSessions)
            });
        assert!(id.is_none());
        assert_eq!(reg.active_count(), 0);
    }
}
