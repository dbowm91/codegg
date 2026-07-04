//! Shell command output projection model.
//!
//! Phase 1 of the shell output projection roadmap
//! (`plans/shell_output_projection_phase_01_command_event_model.md`)
//! and the substrate for Phase 2
//! (`plans/shell_output_projection_phase_02_projection_trait.md`).
//!
//! This module introduces a structured command event that is the foundation
//! for the projection pipeline. Every shell command run produces a
//! [`CommandRun`] domain object that captures the command's identity, exit
//! state, captured output handles, and metadata. Raw stdout/stderr are
//! retained out-of-band in [`CommandOutputStore`] so that projection,
//! expansion, redaction, and TUI code can reference them by stable handles
//! without rerunning the command.
//!
//! This module is **additive** — it does not modify the existing
//! human-shell runtime or its ephemeral transcript in
//! [`crate::shell::store`]. The two systems run side by side: the legacy
//! human-shell store keeps its bounded head/tail transcript for the TUI;
//! the new command event store keeps the durable raw bytes used by
//! projection, expansion, and future native projectors.
//!
//! Phase 2 adds the projector trait, built-in projectors, and the
//! centralised selector in [`crate::shell::projector`]. The
//! [`default_command_projection`] entry point continues to exist; it now
//! delegates to the selector so every model-visible projection flows
//! through the same selection logic.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Monotonic, session-scoped command run identifier.
///
/// IDs are assigned by [`CommandOutputStore::alloc_id`] and are unique and
/// monotonically increasing within a single store instance. They are not
/// required to be globally unique across processes or sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommandRunId(pub u64);

impl std::fmt::Display for CommandRunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for CommandRunId {
    fn from(v: u64) -> Self {
        CommandRunId(v)
    }
}

impl From<CommandRunId> for u64 {
    fn from(v: CommandRunId) -> Self {
        v.0
    }
}

impl From<crate::shell::types::ShellCommandId> for CommandRunId {
    fn from(v: crate::shell::types::ShellCommandId) -> Self {
        CommandRunId(v.0)
    }
}

/// Which captured output stream a handle refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandOutputStream {
    Stdout,
    Stderr,
    Combined,
}

impl CommandOutputStream {
    pub fn as_str(self) -> &'static str {
        match self {
            CommandOutputStream::Stdout => "stdout",
            CommandOutputStream::Stderr => "stderr",
            CommandOutputStream::Combined => "combined",
        }
    }
}

/// Stable handle to a captured output stream within a session.
///
/// Handles are produced by [`CommandOutputStore::insert`] and resolved by
/// [`CommandOutputStore::get_stream`] / [`CommandOutputStore::get_range`].
/// The string form (`cmd://<id>/<stream>`) is the canonical expansion
/// handle used by later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputHandle {
    pub command_id: CommandRunId,
    pub stream: CommandOutputStream,
}

impl OutputHandle {
    pub fn new(command_id: CommandRunId, stream: CommandOutputStream) -> Self {
        Self { command_id, stream }
    }

    /// Canonical handle string, e.g. `cmd://42/stdout`.
    pub fn as_url(&self) -> String {
        format!("cmd://{}/{}", self.command_id.0, self.stream.as_str())
    }
}

impl std::fmt::Display for OutputHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_url())
    }
}

/// Reasons a command run terminated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandExit {
    /// Process exited normally with an exit code (zero or non-zero).
    Code(i32),
    /// Process was terminated by an OS signal.
    Signal { signal: i32 },
    /// Command exceeded its timeout.
    Timeout,
    /// Command was cancelled by the user (e.g. via shell kill).
    Cancelled,
    /// Command could not be spawned (executable not found, permission, etc.).
    SpawnFailed { message: String },
    /// An internal codegg error prevented the command from completing.
    InternalError { message: String },
}

impl CommandExit {
    /// Whether this exit state is considered a "failure" by projection policies.
    pub fn is_failure(&self) -> bool {
        match self {
            CommandExit::Code(code) => *code != 0,
            CommandExit::Signal { .. }
            | CommandExit::Timeout
            | CommandExit::Cancelled
            | CommandExit::SpawnFailed { .. }
            | CommandExit::InternalError { .. } => true,
        }
    }

    /// Short label suitable for projection metadata.
    pub fn label(&self) -> String {
        match self {
            CommandExit::Code(0) => "exit 0".to_string(),
            CommandExit::Code(code) => format!("exit {}", code),
            CommandExit::Signal { signal } => format!("signal {}", signal),
            CommandExit::Timeout => "timeout".to_string(),
            CommandExit::Cancelled => "cancelled".to_string(),
            CommandExit::SpawnFailed { .. } => "spawn failed".to_string(),
            CommandExit::InternalError { .. } => "internal error".to_string(),
        }
    }
}

/// Marker for whether captured output was actually valid UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputEncoding {
    /// All retained bytes decoded as valid UTF-8.
    Utf8,
    /// At least one byte in the retained prefix/tail was invalid UTF-8.
    NonUtf8,
}

/// Indicates whether the captured raw output represents the full command output
/// or only a bounded prefix/tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCompleteness {
    /// All bytes were retained verbatim.
    Complete,
    /// Output exceeded configured caps; only a bounded prefix/tail is retained.
    Partial,
}

/// Per-stream raw capture state inside [`CommandRun`].
#[derive(Debug, Clone)]
pub struct RawStream {
    /// Total bytes observed on the stream during execution.
    pub total_bytes: u64,
    /// Bytes retained verbatim in [`CommandOutputStore`].
    pub retained_bytes: u64,
    /// Number of lines counted during execution (lazy, may be `None`).
    pub total_lines: Option<u64>,
    /// Stable handle into [`CommandOutputStore`].
    pub handle: Option<OutputHandle>,
    /// Encoding marker for the retained bytes.
    pub encoding: OutputEncoding,
    /// Whether the captured bytes are the complete stream.
    pub completeness: OutputCompleteness,
}

impl RawStream {
    #[allow(dead_code)]
    fn empty() -> Self {
        Self {
            total_bytes: 0,
            retained_bytes: 0,
            total_lines: None,
            handle: None,
            encoding: OutputEncoding::Utf8,
            completeness: OutputCompleteness::Complete,
        }
    }
}

/// Placeholder for the projection state attached to a [`CommandRun`].
///
/// Phase 2 introduces the real [`crate::shell::projector::ProjectionResult`]
/// descriptor; the per-run handle on `CommandRun` continues to be a small
/// marker for backwards compatibility, while full per-projection metadata
/// is returned by the projector trait in [`crate::shell::projector`].
#[derive(Debug, Clone, Default)]
pub struct ProjectionHandle;

/// Placeholder for redaction state attached to a [`CommandRun`].
///
/// Phase 8 will replace this with a real redaction descriptor. Phase 1
/// only needs the field to exist so later code can wire into it without
/// changing the domain object shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedactionState {
    #[default]
    NotApplied,
    Applied,
}

/// Structured command execution event.
///
/// This is the durable domain object that later projection, TUI, model
/// tooling, and expansion paths reference. The command itself may have
/// completed, timed out, failed to spawn, or been cancelled — all states
/// are representable.
#[derive(Debug, Clone)]
pub struct CommandRun {
    pub id: CommandRunId,
    pub command: String,
    pub argv: Option<Vec<String>>,
    pub cwd: PathBuf,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub exit: CommandExit,
    pub stdout: RawStream,
    pub stderr: RawStream,
    pub combined: Option<RawStream>,
    pub projection: Option<ProjectionHandle>,
    pub redaction: RedactionState,
}

impl CommandRun {
    /// Convenience: returns true if the run terminated in a non-success state.
    pub fn is_failure(&self) -> bool {
        self.exit.is_failure()
    }

    /// Convenience: total bytes observed across stdout and stderr.
    pub fn total_bytes(&self) -> u64 {
        self.stdout.total_bytes + self.stderr.total_bytes
    }

    /// Convenience: total retained bytes across stdout and stderr.
    pub fn total_retained_bytes(&self) -> u64 {
        self.stdout.retained_bytes + self.stderr.retained_bytes
    }

    /// Convenience: returns true if any stream had its retention capped.
    pub fn is_partial(&self) -> bool {
        matches!(self.stdout.completeness, OutputCompleteness::Partial)
            || matches!(self.stderr.completeness, OutputCompleteness::Partial)
    }

    /// Stable stdout handle, if any.
    pub fn stdout_handle(&self) -> Option<OutputHandle> {
        self.stdout.handle
    }

    /// Stable stderr handle, if any.
    pub fn stderr_handle(&self) -> Option<OutputHandle> {
        self.stderr.handle
    }

    /// Stable combined handle, if any.
    pub fn combined_handle(&self) -> Option<OutputHandle> {
        self.combined.as_ref().and_then(|s| s.handle)
    }
}

/// Configurable caps for [`CommandOutputStore`] raw retention.
///
/// Defaults follow the plan:
/// `COMMAND_OUTPUT_MAX_RETAINED_BYTES = 64 MiB`,
/// `COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES = 32 MiB`.
#[derive(Debug, Clone, Copy)]
pub struct CommandOutputStoreLimits {
    /// Maximum bytes retained across all commands.
    pub max_total_retained_bytes: usize,
    /// Maximum bytes retained per single stream of a single command.
    pub max_single_stream_bytes: usize,
    /// Maximum number of completed commands retained.
    pub max_history_entries: usize,
}

impl Default for CommandOutputStoreLimits {
    fn default() -> Self {
        Self {
            max_total_retained_bytes: COMMAND_OUTPUT_MAX_RETAINED_BYTES,
            max_single_stream_bytes: COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES,
            max_history_entries: COMMAND_OUTPUT_MAX_HISTORY_ENTRIES,
        }
    }
}

/// Per-command record retained by [`CommandOutputStore`].
#[derive(Debug, Clone)]
pub struct StoredCommandRun {
    pub run: CommandRun,
    /// Retained stdout bytes (possibly truncated to `max_single_stream_bytes`).
    pub stdout_bytes: Vec<u8>,
    /// Retained stderr bytes (possibly truncated to `max_single_stream_bytes`).
    pub stderr_bytes: Vec<u8>,
}

/// Default cap on total retained raw bytes across all commands.
///
/// 64 MiB is large enough for typical build/test output and small enough
/// to keep memory bounded on long-running sessions.
pub const COMMAND_OUTPUT_MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

/// Default cap on retained bytes for a single stream of a single command.
pub const COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES: usize = 32 * 1024 * 1024;

/// Default cap on the number of completed commands retained.
pub const COMMAND_OUTPUT_MAX_HISTORY_ENTRIES: usize = 100;

/// Raw byte store for shell command output.
///
/// The store keeps bounded raw stdout/stderr per [`CommandRunId`]. It
/// serves two roles:
///
/// 1. Durability — raw output survives after the producing process exits
///    so projection, expansion, and redaction code can read it.
/// 2. Boundedness — explicit caps prevent unbounded memory growth even
///    when commands emit very large output.
///
/// The store does NOT synthesize "combined" output ordering; if the
/// execution layer does not supply a merged stream, downstream code must
/// synthesize it explicitly and label it as such.
#[derive(Debug)]
pub struct CommandOutputStore {
    limits: CommandOutputStoreLimits,
    next_id: AtomicU64,
    runs: HashMap<CommandRunId, StoredCommandRun>,
    /// LRU order from oldest to newest for eviction.
    insertion_order: Vec<CommandRunId>,
    /// Total retained bytes across all runs and streams.
    total_retained_bytes: usize,
}

impl CommandOutputStore {
    pub fn new() -> Self {
        Self::with_limits(CommandOutputStoreLimits::default())
    }

    pub fn with_limits(limits: CommandOutputStoreLimits) -> Self {
        Self {
            limits,
            next_id: AtomicU64::new(1),
            runs: HashMap::new(),
            insertion_order: Vec::new(),
            total_retained_bytes: 0,
        }
    }

    pub fn limits(&self) -> &CommandOutputStoreLimits {
        &self.limits
    }

    /// Allocates the next monotonic command run ID.
    pub fn alloc_id(&self) -> CommandRunId {
        CommandRunId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Inserts raw stdout/stderr bytes for a command and returns the
    /// resulting output handles.
    ///
    /// Per-stream bytes are truncated to `limits.max_single_stream_bytes`
    /// when they exceed the cap; in that case the stream is marked
    /// [`OutputCompleteness::Partial`] on the returned [`CommandRun`].
    /// UTF-8 validity is checked on the retained prefix only — total byte
    /// counts reflect what was observed on the stream, not what was
    /// retained.
    pub fn insert(
        &mut self,
        command_id: CommandRunId,
        command: String,
        cwd: PathBuf,
        started_at: SystemTime,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> CommandRun {
        self.insert_with_argv(command_id, command, None, cwd, started_at, stdout, stderr)
    }

    /// Like [`Self::insert`] but allows the caller to record a parsed argv.
    pub fn insert_with_argv(
        &mut self,
        command_id: CommandRunId,
        command: String,
        argv: Option<Vec<String>>,
        cwd: PathBuf,
        started_at: SystemTime,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> CommandRun {
        let stdout_total = stdout.len() as u64;
        let stderr_total = stderr.len() as u64;

        let (stdout_retained, stdout_complete) =
            Self::cap_stream(&stdout, self.limits.max_single_stream_bytes);
        let (stderr_retained, stderr_complete) =
            Self::cap_stream(&stderr, self.limits.max_single_stream_bytes);

        let stdout_encoding = Self::detect_encoding(&stdout_retained);
        let stderr_encoding = Self::detect_encoding(&stderr_retained);

        let stdout_handle = OutputHandle::new(command_id, CommandOutputStream::Stdout);
        let stderr_handle = OutputHandle::new(command_id, CommandOutputStream::Stderr);

        let stdout_stream = RawStream {
            total_bytes: stdout_total,
            retained_bytes: stdout_retained.len() as u64,
            total_lines: Some(stdout_retained.iter().filter(|&&b| b == b'\n').count() as u64),
            handle: Some(stdout_handle),
            encoding: stdout_encoding,
            completeness: stdout_complete,
        };

        let stderr_stream = RawStream {
            total_bytes: stderr_total,
            retained_bytes: stderr_retained.len() as u64,
            total_lines: Some(stderr_retained.iter().filter(|&&b| b == b'\n').count() as u64),
            handle: Some(stderr_handle),
            encoding: stderr_encoding,
            completeness: stderr_complete,
        };

        let run = CommandRun {
            id: command_id,
            command,
            argv,
            cwd,
            started_at,
            duration: Duration::ZERO,
            exit: CommandExit::Code(0),
            stdout: stdout_stream,
            stderr: stderr_stream,
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        };

        let stored = StoredCommandRun {
            run: run.clone(),
            stdout_bytes: stdout_retained,
            stderr_bytes: stderr_retained,
        };

        let retained = stored.stdout_bytes.len() + stored.stderr_bytes.len();
        self.runs.insert(command_id, stored);
        self.insertion_order.push(command_id);
        self.total_retained_bytes += retained;
        self.evict();

        run
    }

    /// Record the terminal exit state for a previously-inserted command.
    pub fn record_exit(&mut self, command_id: CommandRunId, exit: CommandExit, duration: Duration) {
        if let Some(stored) = self.runs.get_mut(&command_id) {
            stored.run.exit = exit;
            stored.run.duration = duration;
        }
    }

    /// Returns the [`CommandRun`] metadata for a command, if any.
    pub fn get_run(&self, command_id: CommandRunId) -> Option<&CommandRun> {
        self.runs.get(&command_id).map(|s| &s.run)
    }

    /// Returns retained bytes for a stream handle, or `None` if the
    /// handle is unknown or refers to a stream that was never captured.
    pub fn get_stream(&self, handle: OutputHandle) -> Option<&[u8]> {
        let stored = self.runs.get(&handle.command_id)?;
        match handle.stream {
            CommandOutputStream::Stdout => Some(&stored.stdout_bytes),
            CommandOutputStream::Stderr => Some(&stored.stderr_bytes),
            CommandOutputStream::Combined => None,
        }
    }

    /// Returns a slice of retained bytes for a stream handle.
    ///
    /// Out-of-range requests return `None` so callers can distinguish
    /// invalid range lookups from empty streams. A range that is partially
    /// out of bounds is clamped to the available length.
    pub fn get_range(&self, handle: OutputHandle, range: Range<usize>) -> Option<&[u8]> {
        let stream = self.get_stream(handle)?;
        if range.start > stream.len() {
            return None;
        }
        let end = range.end.min(stream.len());
        if range.start > end {
            return None;
        }
        Some(&stream[range.start..end])
    }

    /// Returns the retained byte length for a stream handle.
    pub fn byte_len(&self, handle: OutputHandle) -> Option<usize> {
        self.get_stream(handle).map(|s| s.len())
    }

    /// Resolve a canonical handle URL (`cmd://<id>/<stream>`) to a handle.
    pub fn parse_handle(&self, url: &str) -> Option<OutputHandle> {
        let rest = url.strip_prefix("cmd://")?;
        let mut parts = rest.split('/');
        let id: u64 = parts.next()?.parse().ok()?;
        let stream = match parts.next()? {
            "stdout" => CommandOutputStream::Stdout,
            "stderr" => CommandOutputStream::Stderr,
            "combined" => CommandOutputStream::Combined,
            _ => return None,
        };
        if parts.next().is_some() {
            return None;
        }
        let command_id = CommandRunId(id);
        if !self.runs.contains_key(&command_id) {
            return None;
        }
        Some(OutputHandle::new(command_id, stream))
    }

    /// Returns the number of completed command runs retained.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Returns true if no command runs are retained.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Total retained bytes across all streams and commands.
    pub fn total_retained_bytes(&self) -> usize {
        self.total_retained_bytes
    }

    /// Returns command IDs in insertion order (oldest first).
    pub fn command_ids(&self) -> &[CommandRunId] {
        &self.insertion_order
    }

    fn cap_stream(stream: &[u8], cap: usize) -> (Vec<u8>, OutputCompleteness) {
        if stream.len() <= cap {
            (stream.to_vec(), OutputCompleteness::Complete)
        } else {
            (stream[..cap].to_vec(), OutputCompleteness::Partial)
        }
    }

    fn detect_encoding(bytes: &[u8]) -> OutputEncoding {
        if std::str::from_utf8(bytes).is_ok() {
            OutputEncoding::Utf8
        } else {
            OutputEncoding::NonUtf8
        }
    }

    fn evict(&mut self) {
        while self.runs.len() > self.limits.max_history_entries {
            if let Some(oldest) = self.insertion_order.first().copied() {
                self.remove(oldest);
            } else {
                break;
            }
        }
        while self.total_retained_bytes > self.limits.max_total_retained_bytes
            && self.runs.len() > 1
        {
            if let Some(oldest) = self.insertion_order.first().copied() {
                self.remove(oldest);
            } else {
                break;
            }
        }
    }

    fn remove(&mut self, id: CommandRunId) {
        if let Some(stored) = self.runs.remove(&id) {
            self.total_retained_bytes = self
                .total_retained_bytes
                .saturating_sub(stored.stdout_bytes.len() + stored.stderr_bytes.len());
        }
        if let Some(pos) = self.insertion_order.iter().position(|x| *x == id) {
            self.insertion_order.remove(pos);
        }
    }
}

impl Default for CommandOutputStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Backwards-compatible projection entry point for [`CommandRun`].
///
/// Phase 1 introduced this as a single seam through which all
/// model-visible command output flows. Phase 2 keeps the function
/// signature stable but re-implements it on top of the projector trait
/// in [`crate::shell::projector`]; every call now flows through the
/// [`crate::shell::projector::ProjectionSelector`] and selects between
/// [`crate::shell::projector::RawProjector`],
/// [`crate::shell::projector::ErrorRetentionProjector`], and
/// [`crate::shell::projector::TruncatedProjector`] based on run state
/// and budget.
///
/// The returned string includes:
/// - command ID, command string, cwd, exit label, duration
/// - the projected stdout and stderr text (raw or truncated)
/// - raw retention handles (`cmd://<id>/stdout`, `cmd://<id>/stderr`)
///
/// The function does NOT invoke RTK or external backends; that
/// integration is added in later phases.
pub fn default_command_projection(run: &CommandRun, store: &CommandOutputStore) -> String {
    let budget = DEFAULT_PROJECTION_BUDGET_BYTES;
    default_command_projection_with_budget(run, store, budget)
}

/// Default projection budget for the Phase 1 placeholder.
pub const DEFAULT_PROJECTION_BUDGET_BYTES: usize = 8 * 1024;

/// Like [`default_command_projection`] but with an explicit per-output budget.
///
/// Re-exported for backwards compatibility; the real implementation
/// lives in [`crate::shell::projector`] and is selected via the
/// [`crate::shell::projector::ProjectionSelector`].
pub use crate::shell::projector::default_command_projection_with_budget;

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/tmp")
    }

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH
    }

    #[test]
    fn ids_are_monotonic_and_unique() {
        let store = CommandOutputStore::new();
        let a = store.alloc_id();
        let b = store.alloc_id();
        let c = store.alloc_id();
        assert!(a < b);
        assert!(b < c);
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn streams_are_stored_separately() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "echo hi; echo err >&2".to_string(),
            cwd(),
            now(),
            b"hi\n".to_vec(),
            b"err\n".to_vec(),
        );
        let stdout_handle = run.stdout_handle().unwrap();
        let stderr_handle = run.stderr_handle().unwrap();
        assert_eq!(store.get_stream(stdout_handle), Some(b"hi\n".as_slice()));
        assert_eq!(store.get_stream(stderr_handle), Some(b"err\n".as_slice()));
        assert_eq!(run.stdout.total_bytes, 3);
        assert_eq!(run.stderr.total_bytes, 4);
    }

    #[test]
    fn handle_resolves_to_correct_bytes() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"out".to_vec(),
            b"err".to_vec(),
        );
        let stdout = run.stdout_handle().unwrap();
        let stderr = run.stderr_handle().unwrap();
        assert_eq!(store.get_stream(stdout).unwrap(), b"out");
        assert_eq!(store.get_stream(stderr).unwrap(), b"err");
    }

    #[test]
    fn range_lookup_returns_slice() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"hello world".to_vec(),
            Vec::new(),
        );
        let h = run.stdout_handle().unwrap();
        assert_eq!(store.get_range(h, 0..5).unwrap(), b"hello");
        assert_eq!(store.get_range(h, 6..11).unwrap(), b"world");
    }

    #[test]
    fn range_lookup_clamps_overshoot() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"hello".to_vec(),
            Vec::new(),
        );
        let h = run.stdout_handle().unwrap();
        // range.end > stream.len() is clamped to stream.len()
        assert_eq!(store.get_range(h, 0..100).unwrap(), b"hello");
    }

    #[test]
    fn range_lookup_rejects_invalid() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"hi".to_vec(),
            Vec::new(),
        );
        let h = run.stdout_handle().unwrap();
        // start > len => None
        assert!(store.get_range(h, 100..200).is_none());
        // valid in-bounds range returns bytes
        assert_eq!(store.get_range(h, 1..2).unwrap(), b"i");
        // end clamped to len => partial range returns available bytes
        assert_eq!(store.get_range(h, 1..200).unwrap(), b"i");
    }

    #[test]
    fn range_lookup_unknown_handle_is_none() {
        let store = CommandOutputStore::new();
        let bogus = OutputHandle::new(CommandRunId(9999), CommandOutputStream::Stdout);
        assert!(store.get_range(bogus, 0..1).is_none());
    }

    #[test]
    fn exit_state_preserves_nonzero_exit_code() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(id, CommandExit::Code(101), Duration::from_secs(2));
        let r = store.get_run(id).unwrap();
        assert_eq!(r.exit, CommandExit::Code(101));
        assert_eq!(r.duration, Duration::from_secs(2));
        assert!(r.is_failure());
    }

    #[test]
    fn exit_state_represents_spawn_failure() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(
            id,
            CommandExit::SpawnFailed {
                message: "no such file".to_string(),
            },
            Duration::ZERO,
        );
        let r = store.get_run(id).unwrap();
        assert!(matches!(r.exit, CommandExit::SpawnFailed { .. }));
        assert!(r.is_failure());
    }

    #[test]
    fn exit_state_represents_timeout() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(id, CommandExit::Timeout, Duration::from_secs(300));
        let r = store.get_run(id).unwrap();
        assert_eq!(r.exit, CommandExit::Timeout);
        assert!(r.is_failure());
    }

    #[test]
    fn exit_state_represents_signal() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(
            id,
            CommandExit::Signal { signal: 9 },
            Duration::from_secs(1),
        );
        let r = store.get_run(id).unwrap();
        assert_eq!(r.exit, CommandExit::Signal { signal: 9 });
    }

    #[test]
    fn exit_state_represents_cancellation() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(id, CommandExit::Cancelled, Duration::from_secs(1));
        let r = store.get_run(id).unwrap();
        assert_eq!(r.exit, CommandExit::Cancelled);
    }

    #[test]
    fn exit_state_represents_internal_error() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(
            id,
            CommandExit::InternalError {
                message: "boom".to_string(),
            },
            Duration::ZERO,
        );
        let r = store.get_run(id).unwrap();
        assert!(matches!(r.exit, CommandExit::InternalError { .. }));
    }

    #[test]
    fn zero_exit_is_not_failure() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        store.record_exit(id, CommandExit::Code(0), Duration::from_secs(1));
        let r = store.get_run(id).unwrap();
        assert!(!r.is_failure());
    }

    #[test]
    fn oversized_output_is_marked_partial() {
        let mut store = CommandOutputStore::with_limits(CommandOutputStoreLimits {
            max_total_retained_bytes: 64 * 1024 * 1024,
            max_single_stream_bytes: 16,
            max_history_entries: 100,
        });
        let id = store.alloc_id();
        let stdout = vec![b'x'; 1024];
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            stdout.clone(),
            Vec::new(),
        );
        assert_eq!(run.stdout.total_bytes, 1024);
        assert_eq!(run.stdout.retained_bytes, 16);
        assert_eq!(run.stdout.completeness, OutputCompleteness::Partial);
        assert!(run.is_partial());
        let h = run.stdout_handle().unwrap();
        assert_eq!(store.byte_len(h), Some(16));
        // the retained prefix must match the first 16 bytes of the input
        assert_eq!(store.get_stream(h).unwrap(), &stdout[..16]);
    }

    #[test]
    fn total_retained_bytes_is_bounded() {
        let mut store = CommandOutputStore::with_limits(CommandOutputStoreLimits {
            max_total_retained_bytes: 100,
            max_single_stream_bytes: 100,
            max_history_entries: 100,
        });
        let id1 = store.alloc_id();
        let _ = store.insert(
            id1,
            "big1".to_string(),
            cwd(),
            now(),
            vec![b'x'; 60],
            Vec::new(),
        );
        let id2 = store.alloc_id();
        let _ = store.insert(
            id2,
            "big2".to_string(),
            cwd(),
            now(),
            vec![b'y'; 60],
            Vec::new(),
        );
        assert!(store.total_retained_bytes() <= 100);
        // oldest entry should be evicted
        assert!(store.get_run(id1).is_none());
        assert!(store.get_run(id2).is_some());
    }

    #[test]
    fn history_eviction_drops_oldest() {
        let mut store = CommandOutputStore::with_limits(CommandOutputStoreLimits {
            max_total_retained_bytes: 64 * 1024 * 1024,
            max_single_stream_bytes: 64 * 1024,
            max_history_entries: 3,
        });
        let mut ids = Vec::new();
        for i in 0..5 {
            let id = store.alloc_id();
            ids.push(id);
            store.insert(
                id,
                format!("cmd{}", i),
                cwd(),
                now(),
                Vec::new(),
                Vec::new(),
            );
        }
        // ids 0,1 evicted; 2,3,4 retained
        assert!(store.get_run(ids[0]).is_none());
        assert!(store.get_run(ids[1]).is_none());
        assert!(store.get_run(ids[2]).is_some());
        assert!(store.get_run(ids[3]).is_some());
        assert!(store.get_run(ids[4]).is_some());
    }

    #[test]
    fn parse_handle_round_trip() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        let stdout = run.stdout_handle().unwrap();
        let url = stdout.as_url();
        let parsed = store.parse_handle(&url).unwrap();
        assert_eq!(parsed, stdout);
    }

    #[test]
    fn parse_handle_rejects_unknown_id() {
        let store = CommandOutputStore::new();
        assert!(store.parse_handle("cmd://9999/stdout").is_none());
    }

    #[test]
    fn parse_handle_rejects_bad_stream() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        assert!(store
            .parse_handle(&format!("cmd://{}/bogus", id.0))
            .is_none());
    }

    #[test]
    fn parse_handle_rejects_combined_when_unsupported() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        let parsed = store
            .parse_handle(&format!("cmd://{}/combined", id.0))
            .unwrap();
        assert_eq!(parsed.stream, CommandOutputStream::Combined);
        // combined stream is not retained by the store
        assert!(store.get_stream(parsed).is_none());
    }

    #[test]
    fn parse_handle_rejects_extra_segments() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        assert!(store
            .parse_handle(&format!("cmd://{}/stdout/extra", id.0))
            .is_none());
    }

    #[test]
    fn parse_handle_rejects_malformed() {
        let store = CommandOutputStore::new();
        assert!(store.parse_handle("not-a-url").is_none());
        assert!(store.parse_handle("cmd://abc/stdout").is_none());
        assert!(store.parse_handle("").is_none());
    }

    #[test]
    fn non_utf8_output_is_marked() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            vec![0xFF, 0xFE, b'a'],
            Vec::new(),
        );
        assert_eq!(run.stdout.encoding, OutputEncoding::NonUtf8);
    }

    #[test]
    fn utf8_output_is_marked_utf8() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"hi\n".to_vec(),
            Vec::new(),
        );
        assert_eq!(run.stdout.encoding, OutputEncoding::Utf8);
    }

    #[test]
    fn line_counts_are_recorded() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"a\nb\nc\n".to_vec(),
            b"d\n".to_vec(),
        );
        assert_eq!(run.stdout.total_lines, Some(3));
        assert_eq!(run.stderr.total_lines, Some(1));
    }

    #[test]
    fn argv_is_optional_and_recorded() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert_with_argv(
            id,
            "git status".to_string(),
            Some(vec!["git".to_string(), "status".to_string()]),
            cwd(),
            now(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            run.argv,
            Some(vec!["git".to_string(), "status".to_string()])
        );
    }

    #[test]
    fn default_command_projection_returns_text() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _inserted = store.insert(
            id,
            "echo hello".to_string(),
            cwd(),
            now(),
            b"hello\n".to_vec(),
            b"".to_vec(),
        );
        store.record_exit(id, CommandExit::Code(0), Duration::from_millis(200));
        let run = store.get_run(id).unwrap().clone();
        let s = default_command_projection(&run, &store);
        assert!(s.contains("echo hello"));
        assert!(s.contains("exit 0"));
        assert!(s.contains("hello"));
        assert!(s.contains(&format!("cmd://{}/stdout", id.0)));
    }

    #[test]
    fn default_command_projection_handles_failure() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let _inserted = store.insert(
            id,
            "false".to_string(),
            cwd(),
            now(),
            Vec::new(),
            b"oops\n".to_vec(),
        );
        store.record_exit(id, CommandExit::Code(1), Duration::from_millis(50));
        let run = store.get_run(id).unwrap().clone();
        let s = default_command_projection(&run, &store);
        assert!(s.contains("exit 1"));
        assert!(s.contains("oops"));
    }

    #[test]
    fn default_command_projection_respects_budget() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let big: Vec<u8> = (0..2000).map(|i| b'a' + (i % 26) as u8).collect();
        let _inserted = store.insert(id, "big".to_string(), cwd(), now(), big.clone(), Vec::new());
        store.record_exit(id, CommandExit::Code(0), Duration::from_millis(10));
        let run = store.get_run(id).unwrap().clone();
        let s = default_command_projection_with_budget(&run, &store, 64);
        assert!(s.contains("omitted") || s.contains("[truncated"));
        assert!(s.contains("2000 bytes"));
    }

    #[test]
    fn default_command_projection_handles_missing_stream() {
        // Synthetic: a run with no retained handles.
        let run = CommandRun {
            id: CommandRunId(99),
            command: "ghost".to_string(),
            argv: None,
            cwd: PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            duration: Duration::ZERO,
            exit: CommandExit::Code(0),
            stdout: RawStream::empty(),
            stderr: RawStream::empty(),
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        };
        let store = CommandOutputStore::new();
        let s = default_command_projection(&run, &store);
        assert!(s.contains("[command 99]"));
        assert!(s.contains("ghost"));
    }

    #[test]
    fn output_completeness_complete_when_under_cap() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(
            id,
            "c".to_string(),
            cwd(),
            now(),
            b"small".to_vec(),
            Vec::new(),
        );
        assert_eq!(run.stdout.completeness, OutputCompleteness::Complete);
        assert!(!run.is_partial());
    }

    #[test]
    fn len_and_is_empty() {
        let mut store = CommandOutputStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        let id = store.alloc_id();
        store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());
    }

    #[test]
    fn combined_handle_is_none_by_default() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        assert!(run.combined_handle().is_none());
    }

    #[test]
    fn limits_default_is_sane() {
        let l = CommandOutputStoreLimits::default();
        const _: () =
            assert!(COMMAND_OUTPUT_MAX_RETAINED_BYTES >= COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES);
        const _: () = assert!(COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES > 0);
        assert!(l.max_total_retained_bytes >= l.max_single_stream_bytes);
        assert!(l.max_history_entries > 0);
    }

    #[test]
    fn redaction_state_defaults_to_not_applied() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        assert_eq!(run.redaction, RedactionState::NotApplied);
    }

    #[test]
    fn projection_handle_is_none_initially() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        assert!(run.projection.is_none());
    }

    #[test]
    fn command_run_clone_is_independent() {
        let mut store = CommandOutputStore::new();
        let id = store.alloc_id();
        let run = store.insert(id, "c".to_string(), cwd(), now(), Vec::new(), Vec::new());
        let mut cloned = run.clone();
        cloned.command = "mutated".to_string();
        let stored = store.get_run(id).unwrap();
        assert_eq!(stored.command, "c");
    }

    #[test]
    fn exit_label_strings() {
        assert_eq!(CommandExit::Code(0).label(), "exit 0");
        assert_eq!(CommandExit::Code(101).label(), "exit 101");
        assert_eq!(CommandExit::Signal { signal: 9 }.label(), "signal 9");
        assert_eq!(CommandExit::Timeout.label(), "timeout");
        assert_eq!(CommandExit::Cancelled.label(), "cancelled");
        assert_eq!(
            CommandExit::SpawnFailed {
                message: "x".into()
            }
            .label(),
            "spawn failed"
        );
        assert_eq!(
            CommandExit::InternalError {
                message: "x".into()
            }
            .label(),
            "internal error"
        );
    }

    #[test]
    fn stream_label_strings() {
        assert_eq!(CommandOutputStream::Stdout.as_str(), "stdout");
        assert_eq!(CommandOutputStream::Stderr.as_str(), "stderr");
        assert_eq!(CommandOutputStream::Combined.as_str(), "combined");
    }

    #[test]
    fn handle_url_format() {
        let h = OutputHandle::new(CommandRunId(42), CommandOutputStream::Stdout);
        assert_eq!(h.as_url(), "cmd://42/stdout");
        assert_eq!(h.to_string(), "cmd://42/stdout");
    }
}
