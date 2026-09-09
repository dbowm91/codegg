//! Bounded attach/resume protocol for interactive process sessions (M002).
//!
//! This module owns the versioned wire contract that exposes the M001
//! scheduler-owned PTY engine (`crate::interactive_process` in the root
//! crate) through daemon operations: capability negotiation,
//! create/list/attach/detach/input/resize/terminate/remove/resume.
//!
//! ```text
//! client --(transport-bound client_id)--> daemon handler family
//!   create { workspace_id, argv, ... }  -> handle (ephemeral UUID)
//!   attach { handle, from_seq }         -> attachment_id + output chunk
//!   input/resize/terminate/remove { attachment_id, ... }
//!   resume { attachment_id, from_seq }  -> chunk | typed resync
//!   detach { attachment_id } / disconnect -> attachment dropped, process lives
//! ```
//!
//! # Ownership rules (wire-level)
//!
//! - Request payloads NEVER carry a client identity, principal, role, or
//!   capability. Attachment ownership is derived from the trusted
//!   transport `client_id` passed alongside the request by the daemon.
//! - Mutating operations (input/resize/terminate/remove) name an
//!   `attachment_id`, never a process handle. The daemon resolves the
//!   handle server-side from the caller-owned attachment, so one client
//!   cannot drive another client's process by copying IDs.
//! - Output is a shared bounded ring (M001 `SequenceRing`); reads are
//!   on-demand from a sequence cursor. There is no per-attachment queue
//!   and no background task per attachment. Lag is typed
//!   ([`InteractiveResync`]) rather than silently shifted.
//! - Handles are ephemeral: a daemon restart invalidates them and resume
//!   answers with [`InteractiveResyncReason::HandleGone`].
//!
//! # Bounds
//!
//! Every variable-length field is validated by [`InteractiveProcessLimits`]
//! before admission. Older clients that never advertise
//! [`INTERACTIVE_PROCESS_CAPABILITY`] simply never send these operations;
//! unknown capability strings degrade to `supported: false`, never to an
//! error that blocks unrelated traffic.

use serde::{Deserialize, Serialize};

/// Stable identifier advertised during capability negotiation.
pub const INTERACTIVE_PROCESS_CAPABILITY: &str = "interactive_process.v1";

/// Current attach/resume protocol version.
pub const INTERACTIVE_PROCESS_PROTOCOL_VERSION: u32 = 1;

/// Minimum protocol version this build interoperates with.
pub const INTERACTIVE_PROCESS_PROTOCOL_VERSION_MIN: u32 = 1;

/// Maximum argv entries per create request (mirrors the M001 engine bound).
pub const MAX_INTERACTIVE_ARGV_ENTRIES: usize = 64;

/// Maximum bytes per argv entry (mirrors `MAX_ENV_ENTRY_BYTES`, which the
/// M001 engine reuses for argv entries).
pub const MAX_INTERACTIVE_ARGV_ENTRY_BYTES: usize = 32 * 1024;

/// Maximum environment overrides per create request (mirrors M001).
pub const MAX_INTERACTIVE_ENV_OVERRIDES: usize = 64;

/// Maximum bytes per environment name or value (mirrors M001).
pub const MAX_INTERACTIVE_ENV_ENTRY_BYTES: usize = 32 * 1024;

/// Maximum length of a process-handle or attachment identifier.
pub const MAX_INTERACTIVE_ID_LENGTH: usize = 128;

/// Maximum length of a workspace identifier locator.
pub const MAX_INTERACTIVE_WORKSPACE_ID_LENGTH: usize = 256;

/// Maximum length of a workspace-relative cwd locator.
pub const MAX_INTERACTIVE_CWD_LENGTH: usize = 4096;

/// Maximum attachments owned by one transport client.
pub const MAX_ATTACHMENTS_PER_CLIENT: usize = 16;

/// Maximum attachments on one process handle.
pub const MAX_ATTACHMENTS_PER_PROCESS: usize = 16;

/// Maximum attachments daemon-wide.
pub const MAX_ATTACHMENTS_PER_DAEMON: usize = 256;

/// Maximum process entries returned by one list call.
pub const MAX_INTERACTIVE_LIST_ITEMS: usize = 64;

/// Default output bytes per attach/resume read.
pub const DEFAULT_INTERACTIVE_CHUNK_BYTES: usize = 64 * 1024;

/// Hard cap per attach/resume read (mirrors M001 `MAX_READ_BYTES`).
pub const MAX_INTERACTIVE_CHUNK_BYTES: usize = 256 * 1024;

/// Hard cap per input write (mirrors M001 `MAX_INPUT_WRITE_BYTES`).
pub const MAX_INTERACTIVE_INPUT_BYTES: usize = 32 * 1024;

/// Capability declaration for the attach/resume contract.
///
/// Carried inside capability negotiation. The negotiated version is the
/// intersection of the client and daemon ranges; disjoint ranges mean the
/// client must stay on poll-free behavior (no interactive operations)
/// rather than guessing the wire shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveProcessCapabilities {
    /// Lower bound (inclusive) of supported protocol versions.
    pub min_version: u32,
    /// Upper bound (inclusive) of supported protocol versions.
    pub max_version: u32,
    /// `true` when the side applies ordered output bytes on top of a
    /// cursor to reach an equivalent scrollback view.
    #[serde(default = "default_true")]
    pub supports_incremental_output: bool,
    /// `true` when the side tolerates unknown optional fields without
    /// emitting diagnostics. Required-field mismatches still produce a
    /// typed resync or validation error.
    #[serde(default = "default_true")]
    pub supports_unknown_fields: bool,
}

fn default_true() -> bool {
    true
}

impl Default for InteractiveProcessCapabilities {
    fn default() -> Self {
        Self {
            min_version: INTERACTIVE_PROCESS_PROTOCOL_VERSION_MIN,
            max_version: INTERACTIVE_PROCESS_PROTOCOL_VERSION,
            supports_incremental_output: true,
            supports_unknown_fields: true,
        }
    }
}

impl InteractiveProcessCapabilities {
    /// Capability advertised by this build.
    pub fn current() -> Self {
        Self::default()
    }

    /// Negotiate a version between two capability declarations.
    ///
    /// Returns the highest version both sides support, or `None` when the
    /// ranges are disjoint. `None` is a normal compatibility answer, not
    /// an error: the caller reports `supported: false` and sends no
    /// interactive operations.
    pub fn negotiate(client: &Self, daemon: &Self) -> Option<u32> {
        let low = client.min_version.max(daemon.min_version);
        let high = client.max_version.min(daemon.max_version);
        if low > high {
            None
        } else {
            Some(high)
        }
    }

    /// `true` when `version` falls inside the declared range.
    pub fn supports(&self, version: u32) -> bool {
        version >= self.min_version && version <= self.max_version
    }
}

/// One environment override entry (separate struct so JSON stays an object
/// list rather than an untyped pair array).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveEnvOverride {
    pub name: String,
    pub value: String,
}

/// Create request for one interactive process.
///
/// The payload names a workspace and an argv only. It deliberately has no
/// `client_id`, `principal`, `role`, or capability field: ownership is
/// derived from the trusted transport connection by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveProcessCreateRequest {
    pub workspace_id: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env_overrides: Vec<InteractiveEnvOverride>,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
    #[serde(default)]
    pub scrollback_bytes: Option<usize>,
}

impl InteractiveProcessCreateRequest {
    /// Validate wire bounds before scheduler admission. Invalid requests
    /// never consume admission capacity.
    pub fn validate(&self) -> Result<(), InteractiveProtocolError> {
        if self.workspace_id.is_empty()
            || self.workspace_id.len() > MAX_INTERACTIVE_WORKSPACE_ID_LENGTH
        {
            return Err(InteractiveProtocolError::InvalidWorkspaceId);
        }
        if self.argv.is_empty() || self.argv.len() > MAX_INTERACTIVE_ARGV_ENTRIES {
            return Err(InteractiveProtocolError::InvalidArgv);
        }
        for entry in &self.argv {
            if entry.is_empty()
                || entry.len() > MAX_INTERACTIVE_ARGV_ENTRY_BYTES
                || entry.contains('\0')
            {
                return Err(InteractiveProtocolError::InvalidArgv);
            }
        }
        if let Some(cwd) = self.cwd.as_ref() {
            if cwd.is_empty() || cwd.len() > MAX_INTERACTIVE_CWD_LENGTH || cwd.contains('\0') {
                return Err(InteractiveProtocolError::InvalidCwd);
            }
        }
        if self.env_overrides.len() > MAX_INTERACTIVE_ENV_OVERRIDES {
            return Err(InteractiveProtocolError::TooManyEnvOverrides);
        }
        for entry in &self.env_overrides {
            if entry.name.is_empty()
                || entry.name.len() > MAX_INTERACTIVE_ENV_ENTRY_BYTES
                || entry.value.len() > MAX_INTERACTIVE_ENV_ENTRY_BYTES
                || entry.name.contains('\0')
                || entry.value.contains('\0')
            {
                return Err(InteractiveProtocolError::InvalidEnvEntry);
            }
        }
        Ok(())
    }
}

/// Point-in-time process metadata (no output bytes, no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveProcessMetadata {
    pub handle: String,
    pub workspace_id: String,
    pub command: String,
    pub state: String,
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub child_pid: Option<u32>,
    pub next_seq: u64,
    pub retained_bytes: usize,
    pub total_bytes: u64,
    pub truncated: bool,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub exit_signal: Option<i32>,
}

/// One bounded output read plus its resumption cursor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveOutputChunk {
    pub handle: String,
    /// Sequence the read started from (echo of the request cursor).
    pub from_seq: u64,
    /// Cursor to pass as `from_seq` on the next resume.
    pub next_seq: u64,
    /// `true` when `from_seq` predated retention: the returned bytes are
    /// the oldest retained bytes, not the requested range. Callers must
    /// treat the stream as resynchronized (see `resync` on attach and
    /// `InteractiveProcessResyncRequired` on resume).
    #[serde(default)]
    pub gap: bool,
    /// Raw output bytes, base64-encoded so arbitrary terminal bytes
    /// survive JSON transport. Bounded by [`MAX_INTERACTIVE_CHUNK_BYTES`].
    pub data_b64: String,
}

/// Why a cursor can no longer be resumed incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveResyncReason {
    /// `from_seq` predates bounded retention; the oldest retained bytes
    /// were returned instead.
    HistoryExpired,
    /// `from_seq` is ahead of the process's produced bytes; no output was
    /// returned and the caller should resume from `next_seq`.
    CursorAhead,
    /// The handle is unknown (removed, or the daemon restarted and
    /// ephemeral handles were invalidated).
    HandleGone,
    /// The requested protocol version is outside the negotiated range.
    VersionMismatch,
}

/// Typed resync answer: where the stream is now and what to do next.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveResync {
    pub reason: InteractiveResyncReason,
    pub handle: String,
    /// Oldest retained sequence (resume from here for the oldest view).
    pub base_seq: u64,
    /// Newest sequence (resume from here to follow live output).
    pub next_seq: u64,
    /// Current process metadata when the handle is still known.
    #[serde(default)]
    pub snapshot: Option<InteractiveProcessMetadata>,
}

/// Wire validation failures. These are answered as typed
/// `CoreResponse::Error` codes (`interactive_invalid_*`), never panics.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InteractiveProtocolError {
    #[error("invalid workspace identifier")]
    InvalidWorkspaceId,
    #[error("invalid argv")]
    InvalidArgv,
    #[error("invalid cwd")]
    InvalidCwd,
    #[error("too many environment overrides")]
    TooManyEnvOverrides,
    #[error("invalid environment entry")]
    InvalidEnvEntry,
    #[error("invalid handle")]
    InvalidHandle,
    #[error("invalid attachment identifier")]
    InvalidAttachmentId,
    #[error("invalid output cursor")]
    InvalidCursor,
    #[error("output read exceeds the per-read bound")]
    ReadTooLarge,
    #[error("input exceeds the per-write bound")]
    InputTooLarge,
    #[error("invalid terminal size")]
    InvalidSize,
}

/// Validate an opaque handle/attachment identifier from the wire.
pub fn validate_interactive_id(value: &str) -> Result<(), InteractiveProtocolError> {
    validate_interactive_id_with(value, InteractiveProtocolError::InvalidHandle)
}

/// Validate an attachment identifier from the wire.
pub fn validate_attachment_id(value: &str) -> Result<(), InteractiveProtocolError> {
    validate_interactive_id_with(value, InteractiveProtocolError::InvalidAttachmentId)
}

fn validate_interactive_id_with(
    value: &str,
    error: InteractiveProtocolError,
) -> Result<(), InteractiveProtocolError> {
    if value.is_empty() || value.len() > MAX_INTERACTIVE_ID_LENGTH {
        return Err(error);
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_create() -> InteractiveProcessCreateRequest {
        InteractiveProcessCreateRequest {
            workspace_id: "ws-1".to_string(),
            argv: vec!["sh".to_string()],
            cwd: None,
            env_overrides: Vec::new(),
            cols: Some(80),
            rows: Some(24),
            scrollback_bytes: None,
        }
    }

    #[test]
    fn negotiate_picks_intersection_high() {
        let client = InteractiveProcessCapabilities {
            min_version: 1,
            max_version: 2,
            ..Default::default()
        };
        let daemon = InteractiveProcessCapabilities::current();
        assert_eq!(
            InteractiveProcessCapabilities::negotiate(&client, &daemon),
            Some(1)
        );
    }

    #[test]
    fn negotiate_returns_none_for_disjoint_ranges() {
        let client = InteractiveProcessCapabilities {
            min_version: 2,
            max_version: 3,
            ..Default::default()
        };
        let daemon = InteractiveProcessCapabilities::current();
        assert_eq!(
            InteractiveProcessCapabilities::negotiate(&client, &daemon),
            None
        );
    }

    #[test]
    fn current_capability_supports_current_version() {
        let caps = InteractiveProcessCapabilities::current();
        assert!(caps.supports(INTERACTIVE_PROCESS_PROTOCOL_VERSION));
        assert!(caps.supports(INTERACTIVE_PROCESS_PROTOCOL_VERSION_MIN));
        assert!(!caps.supports(INTERACTIVE_PROCESS_PROTOCOL_VERSION + 1));
    }

    #[test]
    fn create_validation_accepts_minimal_request() {
        assert!(valid_create().validate().is_ok());
    }

    #[test]
    fn create_validation_rejects_empty_and_oversized_argv() {
        let mut request = valid_create();
        request.argv.clear();
        assert_eq!(
            request.validate(),
            Err(InteractiveProtocolError::InvalidArgv)
        );
        request.argv = vec!["x".repeat(MAX_INTERACTIVE_ARGV_ENTRY_BYTES + 1)];
        assert_eq!(
            request.validate(),
            Err(InteractiveProtocolError::InvalidArgv)
        );
    }

    #[test]
    fn create_validation_rejects_nul_and_env_overflow() {
        let mut request = valid_create();
        request.argv = vec!["sh\0-c".to_string()];
        assert_eq!(
            request.validate(),
            Err(InteractiveProtocolError::InvalidArgv)
        );
        let mut request = valid_create();
        request.env_overrides = (0..MAX_INTERACTIVE_ENV_OVERRIDES + 1)
            .map(|index| InteractiveEnvOverride {
                name: format!("VAR_{index}"),
                value: "1".to_string(),
            })
            .collect();
        assert_eq!(
            request.validate(),
            Err(InteractiveProtocolError::TooManyEnvOverrides)
        );
    }

    #[test]
    fn id_validation_rejects_empty_oversized_and_non_token() {
        assert!(validate_interactive_id("").is_err());
        assert!(validate_interactive_id(&"a".repeat(MAX_INTERACTIVE_ID_LENGTH + 1)).is_err());
        assert!(validate_interactive_id("../escape").is_err());
        assert!(validate_interactive_id("client 1").is_err());
        assert!(validate_interactive_id("550e8400-e29b-41d4-a716-446655440000").is_ok());
    }

    #[test]
    fn create_request_has_no_owner_or_capability_field() {
        // Ownership must be transport-derived: the wire shape must not
        // accept a client identity, principal, role, or capability.
        let value = serde_json::to_value(valid_create()).expect("serialize");
        let object = value.as_object().expect("object");
        for forbidden in ["client_id", "principal", "role", "capability", "owner"] {
            assert!(
                !object.contains_key(forbidden),
                "create payload must not carry {forbidden}"
            );
        }
    }

    #[test]
    fn unknown_capability_json_still_decodes_with_defaults() {
        // Older clients ignore unknown optional fields.
        let caps: InteractiveProcessCapabilities =
            serde_json::from_str(r#"{"min_version":1,"max_version":1,"future_flag":true}"#)
                .expect("decode");
        assert_eq!(caps, InteractiveProcessCapabilities::current());
    }
}
