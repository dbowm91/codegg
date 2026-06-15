//! Process exit observation and supervisor for LSP servers.
//!
//! The supervisor observes child process exit, stdout EOF, transport
//! failure, and explicit shutdown. It owns the single authoritative
//! process monitor to avoid double-waiting on child handles.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Authoritative event emitted when a child process exits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspProcessExitEvent {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    /// Process exit code (None if killed by signal).
    pub status: Option<i32>,
    /// Signal that killed the process (platform-specific, None on Windows).
    pub signal: Option<i32>,
    /// Whether this exit was expected (graceful shutdown in progress).
    pub expected: bool,
    /// Bounded stderr tail captured before exit.
    pub stderr_tail: Vec<String>,
    pub timestamp: SystemTime,
}

impl LspProcessExitEvent {
    pub fn new(
        server_id: impl Into<String>,
        root: PathBuf,
        generation: u64,
        status: Option<i32>,
        signal: Option<i32>,
        expected: bool,
        stderr_tail: Vec<String>,
    ) -> Self {
        Self {
            server_id: server_id.into(),
            root,
            generation,
            status,
            signal,
            expected,
            stderr_tail,
            timestamp: SystemTime::now(),
        }
    }

    /// Returns true if the exit indicates a crash (non-zero, unexpected).
    pub fn is_crash(&self) -> bool {
        !self.expected && self.status != Some(0)
    }

    /// Human-readable reason for the exit.
    pub fn reason(&self) -> String {
        if self.expected {
            return "graceful shutdown".to_string();
        }
        match (self.status, self.signal) {
            (Some(code), _) => format!("exited with code {code}"),
            (_, Some(sig)) => format!("killed by signal {sig}"),
            _ => "exited unexpectedly".to_string(),
        }
    }
}

/// Bounded stderr ring buffer for a single LSP client.
///
/// Retains the last `MAX_LINES` lines and at most `MAX_BYTES` total.
#[derive(Debug, Clone)]
pub struct StderrRingBuffer {
    lines: Vec<String>,
    total_bytes: usize,
}

const MAX_LINES: usize = 100;
const MAX_BYTES: usize = 64 * 1024;

impl StderrRingBuffer {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            total_bytes: 0,
        }
    }

    /// Push a line, evicting oldest if over bounds.
    pub fn push(&mut self, line: String) {
        let line_bytes = line.len();
        self.lines.push(line);
        self.total_bytes += line_bytes;
        // Evict oldest lines if over bounds.
        while self.lines.len() > MAX_LINES || self.total_bytes > MAX_BYTES {
            if let Some(oldest) = self.lines.first() {
                self.total_bytes = self.total_bytes.saturating_sub(oldest.len());
                self.lines.remove(0);
            } else {
                break;
            }
        }
    }

    /// Return a snapshot of the current stderr lines.
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.clone()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Number of lines currently stored.
    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

impl Default for StderrRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_event_is_crash() {
        let ev = LspProcessExitEvent::new(
            "test",
            PathBuf::from("/tmp"),
            1,
            Some(1),
            None,
            false,
            vec![],
        );
        assert!(ev.is_crash());
    }

    #[test]
    fn exit_event_expected_not_crash() {
        let ev = LspProcessExitEvent::new(
            "test",
            PathBuf::from("/tmp"),
            1,
            Some(0),
            None,
            true,
            vec![],
        );
        assert!(!ev.is_crash());
    }

    #[test]
    fn exit_event_zero_exit_not_crash() {
        let ev = LspProcessExitEvent::new(
            "test",
            PathBuf::from("/tmp"),
            1,
            Some(0),
            None,
            false,
            vec![],
        );
        assert!(!ev.is_crash());
    }

    #[test]
    fn stderr_ring_buffer_evicts_oldest() {
        let mut buf = StderrRingBuffer::new();
        for i in 0..150 {
            buf.push(format!("line {i}"));
        }
        assert_eq!(buf.len(), MAX_LINES);
        // Oldest lines should be evicted.
        assert_eq!(buf.snapshot().first().unwrap(), "line 50");
    }

    #[test]
    fn stderr_ring_buffer_respects_bytes() {
        let mut buf = StderrRingBuffer::new();
        // Each line is ~100 bytes, 1000 lines = ~100KB > 64KB
        for i in 0..1000 {
            buf.push(format!("line {i:04}")); // 8 chars + padding
        }
        assert!(buf.total_bytes <= MAX_BYTES);
    }

    #[test]
    fn stderr_ring_buffer_empty_default() {
        let buf = StderrRingBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }
}
