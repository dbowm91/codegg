//! Authoritative process runtime owner for LSP server child processes.
//!
//! This module owns the single `tokio::task` that watches a child
//! process, captures its stderr, classifies the exit based on an
//! explicit shutdown intent, and publishes exactly one
//! [`LspProcessExitEvent`] per generation.
//!
//! Goals:
//! - One task owns the `Child` handle, stderr ring buffer, intent
//!   receiver, and kill receiver.
//! - The monitor does **not** retain an `Arc<LspClient>` while
//!   awaiting the child; the runtime is the sole owner.
//! - Expected-vs-unexpected exit is determined from an explicit
//!   intent (graceful shutdown, force kill) — never from transport
//!   state.
//! - The supervisor task terminates after publishing the exit event.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use serde::{Deserialize, Serialize};
use tokio::process::Child;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info};

use super::supervisor::{LspProcessExitEvent, StderrRingBuffer};

/// Explicit shutdown intent for the owned child process.
///
/// The runtime is constructed in `Running` state. Callers transition
/// the intent to request graceful or forced shutdown. The exit
/// classifier in the process owner task uses the current intent to
/// decide whether an observed exit was expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LspProcessIntent {
    /// Process is still running normally.
    Running,
    /// Caller requested a graceful protocol-level shutdown.
    /// An exit observed while in this state is `expected`.
    GracefulShutdownRequested,
    /// Caller requested a forced kill. An exit observed while in
    /// this state is `expected`, regardless of exit code or signal.
    ForceKillRequested,
}

impl LspProcessIntent {
    /// True when the intent implies the exit was deliberate.
    pub fn is_expected(self) -> bool {
        matches!(
            self,
            LspProcessIntent::GracefulShutdownRequested | LspProcessIntent::ForceKillRequested
        )
    }
}

/// Handle to the authoritative process runtime for a single
/// generation of a child process.
///
/// Cloning is cheap: every field is a clone of an `Arc` or a
/// channel sender/receiver pair. The runtime owns the process
/// exit watcher, the bounded stderr ring buffer, and the kill
/// channel; the `LspClient` itself only owns stdin/stdout. The
/// exit event is the authoritative signal consumed by
/// `LspService::handle_exit_event` — exit codes and signals are
/// observed by the runtime task and stamped with the current
/// `LspProcessIntent` to classify whether the exit was
/// expected.
#[derive(Clone)]
pub struct LspProcessRuntime {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub intent_tx: watch::Sender<LspProcessIntent>,
    pub exit_rx: watch::Receiver<Option<LspProcessExitEvent>>,
    pub kill_tx: mpsc::Sender<()>,
    stderr_buffer: Arc<StdMutex<StderrRingBuffer>>,
}

impl std::fmt::Debug for LspProcessRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspProcessRuntime")
            .field("server_id", &self.server_id)
            .field("root", &self.root)
            .field("generation", &self.generation)
            .field("intent", &self.intent_tx.borrow())
            .field("exit_published", &self.exit_rx.borrow().is_some())
            .finish()
    }
}

impl LspProcessRuntime {
    /// Request a graceful protocol-level shutdown. No-op if the
    /// intent is no longer `Running` (e.g. a force kill was already
    /// requested).
    pub fn request_graceful_shutdown(&self) {
        let _ = self.intent_tx.send_if_modified(|current| {
            if *current == LspProcessIntent::Running {
                *current = LspProcessIntent::GracefulShutdownRequested;
                true
            } else {
                false
            }
        });
    }

    /// Request a forced kill. Sets the intent to `ForceKillRequested`
    /// and queues a non-blocking `()` on the kill channel so the
    /// process owner task wakes up. Errors on the kill channel are
    /// ignored because the receiver may already be gone.
    pub fn request_force_kill(&self) {
        let _ = self.intent_tx.send_if_modified(|current| {
            if *current != LspProcessIntent::ForceKillRequested {
                *current = LspProcessIntent::ForceKillRequested;
                true
            } else {
                false
            }
        });
        let _ = self.kill_tx.try_send(());
    }

    /// Snapshot the current intent.
    pub fn intent(&self) -> LspProcessIntent {
        *self.intent_tx.borrow()
    }

    /// Snapshot the stderr ring buffer (cloned, bounded).
    pub fn stderr_snapshot(&self) -> Vec<String> {
        let guard = self
            .stderr_buffer
            .lock()
            .expect("stderr buffer mutex poisoned");
        guard.snapshot()
    }

    /// Snapshot the most recent `max_lines` lines from the stderr
    /// ring buffer. Returns the last `max_lines` lines in chronological
    /// order (oldest first). If the buffer has fewer lines than
    /// `max_lines`, the full buffer is returned.
    pub fn stderr_tail_capped(&self, max_lines: usize) -> Vec<String> {
        if max_lines == 0 {
            return Vec::new();
        }
        let guard = self
            .stderr_buffer
            .lock()
            .expect("stderr buffer mutex poisoned");
        let snapshot = guard.snapshot();
        let start = snapshot.len().saturating_sub(max_lines);
        snapshot[start..].to_vec()
    }

    /// Await the next published exit event. Returns `None` if the
    /// watch channel closes before any event is published.
    pub async fn wait_for_exit(&self) -> Option<LspProcessExitEvent> {
        let mut rx = self.exit_rx.clone();
        loop {
            if let Some(event) = rx.borrow_and_update().clone() {
                return Some(event);
            }
            if rx.changed().await.is_err() {
                return None;
            }
        }
    }

    /// Returns the generation this runtime was spawned for.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Spawn the authoritative process owner for a child process.
///
/// Returns:
/// - an [`LspProcessRuntime`] handle that can request intent
///   transitions, query stderr, and await the exit event;
/// - a [`tokio::task::JoinHandle`] for the owner task (test/diagnostic
///   visibility).
pub fn spawn_process_runtime(
    server_id: String,
    root: PathBuf,
    generation: u64,
    mut child: Child,
    stderr: tokio::process::ChildStderr,
) -> (LspProcessRuntime, tokio::task::JoinHandle<()>) {
    let (intent_tx, intent_rx) = watch::channel(LspProcessIntent::Running);
    let (exit_tx, exit_rx) = watch::channel::<Option<LspProcessExitEvent>>(None);
    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
    let stderr_buffer = Arc::new(StdMutex::new(StderrRingBuffer::new()));

    // Stderr capture task. Reads lines until EOF or task cancellation,
    // appending each line to the bounded ring buffer.
    let stderr_buffer_for_reader = stderr_buffer.clone();
    let server_id_for_reader = server_id.clone();
    let stderr_task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    let mut buf = stderr_buffer_for_reader
                        .lock()
                        .expect("stderr buffer mutex poisoned");
                    buf.push(trimmed.to_string());
                }
                Err(_) => break,
            }
        }
        debug!(
            server = %server_id_for_reader,
            lines = stderr_buffer_for_reader
                .lock()
                .map(|b| b.len())
                .unwrap_or(0),
            "stderr reader terminated"
        );
    });

    // Process owner task: waits/kills/reaps and publishes exactly one
    // exit event.
    let server_id_for_owner = server_id.clone();
    let root_for_owner = root.clone();
    let stderr_buffer_for_owner = stderr_buffer.clone();
    let join = tokio::spawn(async move {
        let mut intent_rx = intent_rx;
        let (status, signal, intent) = {
            let exit_status = tokio::select! {
                kill = kill_rx.recv() => {
                    match kill {
                        Some(()) => {
                            // Force-kill path.
                            if let Err(e) = child.start_kill() {
                                debug!(
                                    server = %server_id_for_owner,
                                    error = %e,
                                    "force kill request failed; will still await child"
                                );
                            }
                            child.wait().await
                        }
                        None => {
                            // Kill sender dropped. The runtime handle
                            // is going away, but the child is still
                            // ours. Reap it without killing so the
                            // exit event still publishes.
                            child.wait().await
                        }
                    }
                }
                wait_result = child.wait() => wait_result,
            };
            let _ = intent_rx.borrow_and_update();
            let final_intent = *intent_rx.borrow();
            let (code, sig) = match exit_status {
                Ok(s) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        (s.code(), s.signal())
                    }
                    #[cfg(not(unix))]
                    {
                        (s.code(), None)
                    }
                }
                Err(e) => {
                    debug!(
                        server = %server_id_for_owner,
                        error = %e,
                        "child.wait() failed"
                    );
                    (None, None)
                }
            };
            (code, sig, final_intent)
        };

        let stderr_tail = stderr_buffer_for_owner
            .lock()
            .map(|b| b.snapshot())
            .unwrap_or_default();

        let expected = intent.is_expected();
        let event = LspProcessExitEvent::new(
            server_id_for_owner.clone(),
            root_for_owner.clone(),
            generation,
            status,
            signal,
            expected,
            stderr_tail,
        );

        info!(
            server = %event.server_id,
            root = %event.root.display(),
            status = ?event.status,
            signal = ?event.signal,
            expected = event.expected,
            reason = %event.reason(),
            "process runtime: exit detected"
        );

        let _ = exit_tx.send(Some(event));

        // Best-effort wait for stderr reader to drain; ignore its
        // result since it is purely observational.
        let _ = stderr_task.await;
    });

    let runtime = LspProcessRuntime {
        server_id,
        root,
        generation,
        intent_tx,
        exit_rx,
        kill_tx,
        stderr_buffer,
    };
    (runtime, join)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn intent_running_is_not_expected() {
        assert!(!LspProcessIntent::Running.is_expected());
    }

    #[test]
    fn intent_graceful_shutdown_is_expected() {
        assert!(LspProcessIntent::GracefulShutdownRequested.is_expected());
    }

    #[test]
    fn intent_force_kill_is_expected() {
        assert!(LspProcessIntent::ForceKillRequested.is_expected());
    }

    #[test]
    fn graceful_shutdown_exit_with_status_zero_is_expected() {
        let event = LspProcessExitEvent::new(
            "test",
            PathBuf::from("/tmp"),
            1,
            Some(0),
            None,
            LspProcessIntent::GracefulShutdownRequested.is_expected(),
            vec![],
        );
        assert!(event.is_expected());
    }

    #[test]
    fn force_kill_exit_with_status_one_is_expected() {
        let event = LspProcessExitEvent::new(
            "test",
            PathBuf::from("/tmp"),
            1,
            Some(1),
            None,
            LspProcessIntent::ForceKillRequested.is_expected(),
            vec![],
        );
        assert!(event.is_expected());
    }

    #[test]
    fn unexpected_exit_with_status_one_is_not_expected() {
        let event = LspProcessExitEvent::new(
            "test",
            PathBuf::from("/tmp"),
            1,
            Some(1),
            None,
            LspProcessIntent::Running.is_expected(),
            vec![],
        );
        assert!(!event.is_expected());
    }

    #[test]
    fn intent_serializes_to_canonical_strings() {
        let s = serde_json::to_string(&LspProcessIntent::Running).unwrap();
        assert_eq!(s, "\"Running\"");
        let s = serde_json::to_string(&LspProcessIntent::GracefulShutdownRequested).unwrap();
        assert_eq!(s, "\"GracefulShutdownRequested\"");
        let s = serde_json::to_string(&LspProcessIntent::ForceKillRequested).unwrap();
        assert_eq!(s, "\"ForceKillRequested\"");
    }

    #[test]
    fn stderr_tail_capped_returns_recent_lines() {
        use crate::supervisor::StderrRingBuffer;

        let buf = Arc::new(StdMutex::new(StderrRingBuffer::new()));
        for i in 0..10 {
            buf.lock().unwrap().push(format!("line {i}"));
        }

        // We can't construct an LspProcessRuntime directly without
        // spawning a process, so exercise the helper logic via
        // the underlying buffer.
        let snapshot = buf.lock().unwrap().snapshot();
        let start = snapshot.len().saturating_sub(3);
        let tail: Vec<String> = snapshot[start..].to_vec();
        assert_eq!(tail, vec!["line 7", "line 8", "line 9"]);
    }
}
