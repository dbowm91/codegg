//! Bridge between the existing human-shell event stream and the
//! Phase 1 [`crate::shell::CommandOutputStore`].
//!
//! The human-shell runtime streams raw bytes via
//! [`crate::shell::ShellEvent`] values that are already consumed by the
//! TUI command handler in
//! `src/tui/commands/shell.rs::handle_shell_event`. This module provides
//! a small accumulator that mirrors those events into the durable
//! command event store so that projection, expansion, and redaction
//! code can resolve `cmd://<id>/stdout` and `cmd://<id>/stderr` handles
//! without rerunning the command.
//!
//! The bridge is intentionally additive: it does NOT modify the
//! existing human-shell `ShellOutputStore`, the `ShellEvent` enum, or
//! the runtime. It is a sidecar that watches events and finalizes a
//! `CommandRun` when the command reaches a terminal state.
//!
//! Phase 1 does not stream bytes into the bridge in real time — it
//! buffers stdout/stderr per command id and finalizes the run on
//! Exited/TimedOut/FailedToStart. Later phases can stream chunks into
//! the store incrementally without changing the public API.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::shell::projection::{CommandExit, CommandOutputStore, CommandRunId};
use crate::shell::ShellEvent;

/// In-flight accumulator for one command.
#[derive(Debug, Default)]
struct InFlightCommand {
    command: String,
    cwd: PathBuf,
    started_at: Option<SystemTime>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    finalized: bool,
}

/// Sidecar accumulator that watches [`ShellEvent`]s and feeds them
/// into a [`CommandOutputStore`].
pub struct ShellCommandRunBridge {
    in_flight: HashMap<CommandRunId, InFlightCommand>,
}

impl ShellCommandRunBridge {
    pub fn new() -> Self {
        Self {
            in_flight: HashMap::new(),
        }
    }

    pub fn observe(&mut self, store: &mut CommandOutputStore, event: &ShellEvent) -> CommandRunId {
        match event {
            ShellEvent::Started { id, command, cwd } => {
                let cmd_id = CommandRunId::from(*id);
                let entry = self.in_flight.entry(cmd_id).or_default();
                entry.command = command.clone();
                entry.cwd = cwd.clone();
                entry.started_at.get_or_insert(SystemTime::now());
                entry.finalized = false;
                cmd_id
            }
            ShellEvent::Stdout { id, bytes } => {
                let cmd_id = CommandRunId::from(*id);
                let entry = self.in_flight.entry(cmd_id).or_default();
                entry.stdout.extend_from_slice(bytes);
                cmd_id
            }
            ShellEvent::Stderr { id, bytes } => {
                let cmd_id = CommandRunId::from(*id);
                let entry = self.in_flight.entry(cmd_id).or_default();
                entry.stderr.extend_from_slice(bytes);
                cmd_id
            }
            ShellEvent::Exited {
                id,
                status,
                elapsed,
            } => {
                let cmd_id = CommandRunId::from(*id);
                self.finalize(
                    store,
                    cmd_id,
                    CommandExit::Code(status.unwrap_or(-1)),
                    *elapsed,
                );
                cmd_id
            }
            ShellEvent::TimedOut { id, elapsed } => {
                let cmd_id = CommandRunId::from(*id);
                self.finalize(store, cmd_id, CommandExit::Timeout, *elapsed);
                cmd_id
            }
            ShellEvent::FailedToStart { id, error } => {
                let cmd_id = CommandRunId::from(*id);
                // If we never saw Started for this id (e.g. the runtime
                // failed before emitting Started), synthesize an empty
                // entry so the projection pipeline still has a record.
                let entry = self.in_flight.entry(cmd_id).or_default();
                if entry.command.is_empty() {
                    entry.command = format!("<failed to start id={}>", id.0);
                }
                if entry.started_at.is_none() {
                    entry.started_at = Some(SystemTime::now());
                }
                entry.finalized = false;
                self.finalize(
                    store,
                    cmd_id,
                    CommandExit::SpawnFailed {
                        message: error.clone(),
                    },
                    Duration::ZERO,
                );
                cmd_id
            }
        }
    }

    fn finalize(
        &mut self,
        store: &mut CommandOutputStore,
        id: CommandRunId,
        exit: CommandExit,
        elapsed: Duration,
    ) {
        let Some(mut entry) = self.in_flight.remove(&id) else {
            return;
        };
        if entry.finalized {
            return;
        }
        entry.finalized = true;
        let started_at = entry.started_at.unwrap_or_else(SystemTime::now);
        let _ = store.insert(
            id,
            std::mem::take(&mut entry.command),
            std::mem::take(&mut entry.cwd),
            started_at,
            std::mem::take(&mut entry.stdout),
            std::mem::take(&mut entry.stderr),
        );
        store.record_exit(id, exit, elapsed);
    }
}

impl Default for ShellCommandRunBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::types::ShellCommandId;
    use std::path::PathBuf;
    use std::time::Duration;

    fn shell_id(n: u64) -> ShellCommandId {
        ShellCommandId(n)
    }

    fn run_id(n: u64) -> CommandRunId {
        CommandRunId(n)
    }

    #[test]
    fn started_then_exited_records_run() {
        let mut store = CommandOutputStore::new();
        let mut bridge = ShellCommandRunBridge::new();
        let id = shell_id(1);
        bridge.observe(
            &mut store,
            &ShellEvent::Started {
                id,
                command: "echo hi".to_string(),
                cwd: PathBuf::from("/tmp"),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Stdout {
                id,
                bytes: b"hi\n".to_vec(),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Exited {
                id,
                status: Some(0),
                elapsed: Duration::from_millis(50),
            },
        );
        let run = store.get_run(run_id(1)).expect("run should be recorded");
        assert_eq!(run.command, "echo hi");
        assert_eq!(run.stdout.total_bytes, 3);
        assert_eq!(run.exit, CommandExit::Code(0));
    }

    #[test]
    fn timeout_finalizes_with_timeout_exit() {
        let mut store = CommandOutputStore::new();
        let mut bridge = ShellCommandRunBridge::new();
        let id = shell_id(2);
        bridge.observe(
            &mut store,
            &ShellEvent::Started {
                id,
                command: "sleep 100".to_string(),
                cwd: PathBuf::from("/tmp"),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::TimedOut {
                id,
                elapsed: Duration::from_secs(10),
            },
        );
        let run = store.get_run(run_id(2)).expect("run should be recorded");
        assert_eq!(run.exit, CommandExit::Timeout);
    }

    #[test]
    fn failed_to_start_finalizes_with_spawn_failed() {
        let mut store = CommandOutputStore::new();
        let mut bridge = ShellCommandRunBridge::new();
        let id = shell_id(3);
        bridge.observe(
            &mut store,
            &ShellEvent::FailedToStart {
                id,
                error: "no such file".to_string(),
            },
        );
        let run = store.get_run(run_id(3)).expect("run should be recorded");
        assert!(matches!(run.exit, CommandExit::SpawnFailed { .. }));
    }

    #[test]
    fn double_finalize_is_idempotent() {
        let mut store = CommandOutputStore::new();
        let mut bridge = ShellCommandRunBridge::new();
        let id = shell_id(4);
        bridge.observe(
            &mut store,
            &ShellEvent::Started {
                id,
                command: "c".to_string(),
                cwd: PathBuf::from("/tmp"),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Exited {
                id,
                status: Some(0),
                elapsed: Duration::from_millis(1),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Exited {
                id,
                status: Some(0),
                elapsed: Duration::from_millis(2),
            },
        );
        let run = store.get_run(run_id(4)).unwrap();
        assert_eq!(run.duration, Duration::from_millis(1));
    }

    #[test]
    fn stderr_accumulates_independently() {
        let mut store = CommandOutputStore::new();
        let mut bridge = ShellCommandRunBridge::new();
        let id = shell_id(5);
        bridge.observe(
            &mut store,
            &ShellEvent::Started {
                id,
                command: "c".to_string(),
                cwd: PathBuf::from("/tmp"),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Stdout {
                id,
                bytes: b"out1".to_vec(),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Stderr {
                id,
                bytes: b"err1".to_vec(),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Stdout {
                id,
                bytes: b"out2".to_vec(),
            },
        );
        bridge.observe(
            &mut store,
            &ShellEvent::Exited {
                id,
                status: Some(0),
                elapsed: Duration::from_millis(1),
            },
        );
        let run = store.get_run(run_id(5)).unwrap();
        assert_eq!(run.stdout.total_bytes, 8);
        assert_eq!(run.stderr.total_bytes, 4);
    }

    #[test]
    fn bridge_drops_unknown_id_on_exited() {
        let mut store = CommandOutputStore::new();
        let mut bridge = ShellCommandRunBridge::new();
        let id = shell_id(99);
        bridge.observe(
            &mut store,
            &ShellEvent::Exited {
                id,
                status: Some(0),
                elapsed: Duration::from_millis(1),
            },
        );
        assert!(store.get_run(run_id(99)).is_none());
    }
}
