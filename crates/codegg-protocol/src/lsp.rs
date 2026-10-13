//! Protocol DTOs for LSP mutation application and the M006-B read surface.
//!
//! The mutation half (`LspPreviewApply*`) is the ADR-0008 write path and is
//! unchanged. The read half below is ADR-0012: a bounded, authorized way for
//! a daemon-connected client to ask semantic questions about a project's
//! files, and to receive diagnostics that a project-scoped subscriber can
//! resynchronize from.
//!
//! Two rules shape these types:
//!
//! - A read is **warm-only**. [`LspReadStatus::NotReady`] is a first-class
//!   outcome, deliberately distinct from both an error and an empty success.
//!   A client that cannot tell "nothing to report" from "no server yet" will
//!   show a warming workspace as clean code.
//! - Diagnostics are a **replace-set**, never a delta. Every payload carries
//!   the complete current set for one file plus a monotonic `sequence` and a
//!   content `digest`, so a client that missed envelopes can detect the gap
//!   and re-pull rather than trusting a partial view.

use serde::{Deserialize, Serialize};

/// Maximum bytes accepted for a free-text symbol query.
pub const MAX_LSP_SYMBOL_QUERY_BYTES: usize = 200;
/// Maximum characters returned for a hover payload.
pub const MAX_LSP_HOVER_CHARS: usize = 2000;
/// Maximum locations returned by definition or references.
pub const MAX_LSP_LOCATIONS: usize = 100;
/// Maximum symbols returned by a document or workspace symbol read.
pub const MAX_LSP_SYMBOLS: usize = 300;
/// Maximum semantic tokens returned for one file.
pub const MAX_LSP_SEMANTIC_TOKENS: usize = 1000;
/// Maximum diagnostic entries returned or published for one file.
pub const MAX_LSP_DIAGNOSTICS_PER_FILE: usize = 100;

/// One already-normalized text patch in a reviewed LSP preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPreviewPatchDto {
    pub path: String,
    pub patch: String,
    pub original_hash: String,
}

/// Explicitly authorized request to apply one reviewed LSP preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPreviewApplyRequestDto {
    pub preview_id: String,
    pub preview_revision: u64,
    pub preview_digest: String,
    pub kind: String,
    pub title: String,
    pub provenance: String,
    pub workspace_id: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub patches: Vec<LspPreviewPatchDto>,
}

/// Result of a controlled LSP preview application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPreviewApplyResultDto {
    pub preview_id: String,
    pub preview_revision: u64,
    pub preview_digest: String,
    pub kind: String,
    pub title: String,
    pub written_files: Vec<String>,
    pub checkpoint_id: String,
    #[serde(default)]
    pub synchronization_errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// M006-B read surface
// ---------------------------------------------------------------------------

/// The read operations ADR-0012 exposes in its first slice.
///
/// `TypeDefinition` is intentionally absent: egglsp does not implement it, so
/// offering it here would be a promise the daemon cannot keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspReadOperation {
    Hover,
    Definition,
    References,
    DocumentSymbols,
    WorkspaceSymbols,
    SemanticTokens,
}

impl LspReadOperation {
    /// Whether this operation is anchored to a position in a file.
    ///
    /// Point operations carry `line`/`column` and never `query`; workspace
    /// symbols carry `query` and no position. Keeping the two shapes distinct
    /// is what stops a malformed request from being silently reinterpreted.
    pub fn is_point(&self) -> bool {
        matches!(
            self,
            Self::Hover
                | Self::Definition
                | Self::References
                | Self::DocumentSymbols
                | Self::SemanticTokens
        )
    }
}

/// Why a read could not be answered.
///
/// `NotReady` is not an error. ADR-0012 §3 requires that a read against a
/// cold server report this rather than launch a server, and requires it to be
/// distinguishable from an empty result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspReadStatus {
    Ready,
    NotReady,
}

/// A rejected read request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspReadInvalid {
    /// The operation's fields do not match its shape (a query on a point
    /// operation, or a position on workspace symbols).
    IncoherentFields,
    /// The path was empty, absolute, or escaped the workspace.
    InvalidPath,
    /// The symbol query exceeded [`MAX_LSP_SYMBOL_QUERY_BYTES`].
    QueryTooLong,
    /// The document attachment locator was empty. Carried here so the
    /// completion read shares the read family's rejection taxonomy rather
    /// than inventing a parallel one.
    InvalidAttachment,
}

/// Authorized request for one bounded LSP read.
///
/// `session_id` is a locator only. It authorizes through the owning session's
/// project and carries no authority material of its own, mirroring
/// `LspPreviewApplyRequestDto`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspReadRequestDto {
    pub operation: LspReadOperation,
    pub session_id: String,
    /// Workspace-relative path. Required for point operations.
    pub path: String,
    /// Zero-based line, required for point operations that take a position.
    #[serde(default)]
    pub line: Option<u32>,
    /// Zero-based column, required for point operations that take a position.
    #[serde(default)]
    pub column: Option<u32>,
    /// Free-text query, required for and only for workspace symbols.
    #[serde(default)]
    pub query: Option<String>,
}

impl LspReadRequestDto {
    /// Whether this request is coherent for its operation.
    ///
    /// Validation happens before any service call so a malformed request can
    /// never reach a language server.
    pub fn validate(&self) -> Result<(), LspReadInvalid> {
        if self.operation.is_point() {
            if self.path.is_empty() {
                return Err(LspReadInvalid::InvalidPath);
            }
            if self.query.is_some() {
                return Err(LspReadInvalid::IncoherentFields);
            }
            if self.operation != LspReadOperation::DocumentSymbols && self.line.is_none() {
                return Err(LspReadInvalid::IncoherentFields);
            }
        } else {
            if self.line.is_some() || self.column.is_some() || !self.path.is_empty() {
                return Err(LspReadInvalid::IncoherentFields);
            }
            let query = self.query.as_deref().unwrap_or_default();
            if query.is_empty() {
                return Err(LspReadInvalid::IncoherentFields);
            }
            if query.len() > MAX_LSP_SYMBOL_QUERY_BYTES {
                return Err(LspReadInvalid::QueryTooLong);
            }
        }
        Ok(())
    }
}

/// A source range in a project file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspRangeDto {
    pub path: String,
    /// Daemon-validated target relative to the request's authorized workspace.
    /// Missing values from older daemons remain display-only.
    #[serde(default)]
    pub workspace_relative_path: Option<String>,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// A symbol entry in a document or workspace symbol read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspSymbolDto {
    pub name: String,
    /// LSP `SymbolKind` as an integer, preserved rather than reinterpreted so
    /// a client can style it without the protocol owning a symbol taxonomy.
    pub kind: u32,
    /// Enclosing symbol, when the server reports one.
    #[serde(default)]
    pub container: Option<String>,
    /// Selection range, falling back to the full range when absent.
    pub range: LspRangeDto,
}

/// One semantic token, with absolute line/character and the server's own
/// type and modifier names.
///
/// Names are carried verbatim rather than renumbered into a protocol-owned
/// taxonomy: egglsp reports the server's own strings, and inventing a numeric
/// mapping here would mean the protocol owned a symbol taxonomy it does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspSemanticTokenDto {
    pub line: u32,
    pub character: u32,
    pub length: u32,
    pub token_type: String,
    pub modifiers: Vec<String>,
}

/// The payload of a successful read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspReadPayloadDto {
    Hover {
        /// Truncated to [`MAX_LSP_HOVER_CHARS`]; `truncated` says so.
        text: String,
        truncated: bool,
    },
    Locations {
        locations: Vec<LspRangeDto>,
        truncated: bool,
    },
    Symbols {
        symbols: Vec<LspSymbolDto>,
        truncated: bool,
    },
    SemanticTokens {
        tokens: Vec<LspSemanticTokenDto>,
        truncated: bool,
    },
}

/// Result of one LSP read.
///
/// `status` is authoritative and comes first in meaning: a caller must check
/// it before reading `payload`, and a `NotReady` result never carries one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspReadResultDto {
    pub operation: LspReadOperation,
    pub status: LspReadStatus,
    #[serde(default)]
    pub payload: Option<LspReadPayloadDto>,
}

impl LspReadResultDto {
    /// A read that could not be answered because no server is warm.
    ///
    /// The payload is absent by construction, so a client cannot mistake a
    /// warming workspace for one with no findings.
    pub fn not_ready(operation: LspReadOperation) -> Self {
        Self {
            operation,
            status: LspReadStatus::NotReady,
            payload: None,
        }
    }

    /// A read that succeeded.
    pub fn ready(operation: LspReadOperation, payload: LspReadPayloadDto) -> Self {
        Self {
            operation,
            status: LspReadStatus::Ready,
            payload: Some(payload),
        }
    }
}

/// Authorized request for the authoritative current diagnostics of a project.
///
/// This is ADR-0012's correctness authority. The push stream is a latency
/// optimization; this is what a client uses to reconcile after a detected
/// gap, a reconnect, or a resync requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnosticsGetRequestDto {
    pub project_id: String,
    pub session_id: String,
}

/// The authoritative diagnostic set for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspFileDiagnosticsDto {
    /// Workspace-relative path.
    pub path: String,
    /// Monotonic per-file sequence, incremented on every published change.
    /// A client comparing this against the last streamed `sequence` detects a
    /// missed envelope and re-pulls.
    pub sequence: u64,
    /// Content digest of the diagnostic set. Identity of the set, independent
    /// of how old it is.
    pub digest: String,
    /// The **complete** current set for this file. Never a delta.
    pub diagnostics: Vec<LspDiagnosticDto>,
    /// `true` when the producing server has restarted since it last emitted.
    pub post_restart: bool,
    /// `true` when the set was capped at
    /// [`MAX_LSP_DIAGNOSTICS_PER_FILE`].
    pub truncated: bool,
}

/// One diagnostic entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnosticDto {
    pub range: LspRangeDto,
    /// LSP `DiagnosticSeverity` as an integer (1 = error).
    pub severity: u32,
    /// LSP `DiagnosticTag` as an integer, 0 when absent.
    #[serde(default)]
    pub tag: u32,
    #[serde(default)]
    pub code: Option<String>,
    pub message: String,
    pub source: Option<String>,
}

/// Whether a diagnostics response is a complete snapshot or not ready yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspDiagnosticsStatus {
    Ready,
    #[default]
    NotReady,
}

/// The authoritative diagnostics for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnosticsResultDto {
    pub project_id: String,
    /// Readiness is separate from the contents: an unavailable service must
    /// never be interpreted as an authoritative empty diagnostic set.
    #[serde(default)]
    pub status: LspDiagnosticsStatus,
    pub files: Vec<LspFileDiagnosticsDto>,
    /// `true` when the file list itself was capped, so a client knows the set
    /// it received is not the whole project.
    pub truncated: bool,
}

/// Authorized request to subscribe to a project's LSP diagnostics stream.
///
/// This exists as its own request rather than a flag on `ProjectionSubscribe`
/// because the generic subscribe is gated at `Opaque + project.observe`, which
/// cannot express the `file.read` that ADR-0012 §4 requires for
/// content-bearing reads. Diagnostics carry source messages, code, and
/// snippets, so reusing the generic gate would disclose them to a principal
/// that may only observe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnosticsSubscribeRequestDto {
    pub project_id: String,
    /// Replay cursor, when resuming a subscription that was resync-required.
    #[serde(default)]
    pub cursor: Option<crate::projection::replay::ProjectionCursor>,
    #[serde(default = "default_lsp_projection_version")]
    pub projection_version: u32,
}

fn default_lsp_projection_version() -> u32 {
    1
}

impl LspDiagnosticsSubscribeRequestDto {
    /// Validate before any subscription state is created.
    ///
    /// An unparseable project locator is rejected here rather than deeper in
    /// the projection service, so a malformed request cannot create a
    /// subscription that later resolves to an unexpected project.
    pub fn validate(&self) -> Result<(), LspReadInvalid> {
        if self.project_id.is_empty() {
            return Err(LspReadInvalid::InvalidPath);
        }
        if self.project_id.contains('/') || self.project_id.contains('\\') {
            return Err(LspReadInvalid::InvalidPath);
        }
        Ok(())
    }
}

/// The pushed form of a diagnostics change.
///
/// Byte-for-byte the same content as the authoritative
/// [`LspFileDiagnosticsDto`], because a stream that disagreed with the pull
/// would defeat the purpose of having a pull.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnosticsProjectionDto {
    pub project_id: String,
    pub file: LspFileDiagnosticsDto,
}

// ---------------------------------------------------------------------------
// M003 completion read surface (ADR-0021, Option B)
// ---------------------------------------------------------------------------

/// Maximum candidates returned for one completion read.
pub const MAX_LSP_COMPLETION_CANDIDATES: usize = 100;
/// Maximum total bytes across one completion result's labels, details and
/// replacement text. Past this the daemon stops adding candidates and sets
/// `truncated`.
pub const MAX_LSP_COMPLETION_RESULT_BYTES: usize = 256 * 1024;
/// Maximum characters for a candidate label or its filter/sort text.
/// Display strings are truncated; edits never are.
pub const MAX_LSP_COMPLETION_LABEL_CHARS: usize = 200;
/// Maximum characters for candidate detail text.
pub const MAX_LSP_COMPLETION_DETAIL_CHARS: usize = 200;
/// Maximum bytes for one candidate's replacement text. An oversized edit is
/// reported non-applicable, never silently truncated: truncating an edit
/// changes what would be inserted.
pub const MAX_LSP_COMPLETION_EDIT_BYTES: usize = 8192;
/// Maximum characters for an explicit trigger-character string.
pub const MAX_LSP_COMPLETION_TRIGGER_CHARS: usize = 8;

/// Authorized request for one bounded inert LSP completion read.
///
/// `session_id`, `path` and `document_id` are locators only, mirroring
/// [`LspReadRequestDto`]: the daemon resolves the session's project and
/// workspace, then checks the attachment against them. `revision` is a
/// precondition, not authority: the daemon answers `Ready` only when it
/// equals the daemon's current revision for the attachment, and `Stale`
/// otherwise. Positions are zero-based UTF-16 offsets, matching the language
/// server's view of the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCompletionRequestDto {
    pub session_id: String,
    /// Workspace-relative path. Must agree with the attachment's path; the
    /// two locators disagreeing is a client bug, rejected outright.
    pub path: String,
    /// Actual document attachment identity (`document.v1`).
    pub document_id: String,
    /// Caller's view of the daemon revision. Must equal the daemon's current
    /// revision for a `Ready` answer.
    pub revision: u64,
    /// Zero-based UTF-16 line of the cursor.
    pub line: u32,
    /// Zero-based UTF-16 character of the cursor.
    pub character: u32,
    /// Explicit trigger character, when the user typed one.
    #[serde(default)]
    pub trigger_character: Option<String>,
    /// Desired candidate cap. Must be within
    /// [`MAX_LSP_COMPLETION_CANDIDATES`]; the daemon additionally clamps, so
    /// a newer client can never coax an older bound out of a newer daemon.
    pub max_candidates: u32,
}

impl LspCompletionRequestDto {
    /// Validate before any service call so a malformed request can never
    /// reach a language server.
    pub fn validate(&self) -> Result<(), LspReadInvalid> {
        if self.path.is_empty() {
            return Err(LspReadInvalid::InvalidPath);
        }
        if self.document_id.is_empty() {
            return Err(LspReadInvalid::InvalidAttachment);
        }
        if self.max_candidates == 0 || self.max_candidates as usize > MAX_LSP_COMPLETION_CANDIDATES
        {
            return Err(LspReadInvalid::IncoherentFields);
        }
        if let Some(trigger) = &self.trigger_character {
            let chars = trigger.chars().count();
            if chars == 0 || chars > MAX_LSP_COMPLETION_TRIGGER_CHARS {
                return Err(LspReadInvalid::IncoherentFields);
            }
        }
        Ok(())
    }

    /// Effective item cap: the validated request clamped to the protocol
    /// bound, so daemon code never re-derives the minimum.
    pub fn capped_max_candidates(&self) -> usize {
        (self.max_candidates as usize).min(MAX_LSP_COMPLETION_CANDIDATES)
    }
}

/// Why a completion read did or did not produce candidates.
///
/// Every non-`Ready` state is explicit. `NotReady` (no warm server) is not an
/// empty success; `Stale` (caller behind the daemon) is not `Unsynced`
/// (daemon behind the language server); `Unsupported` (no completion
/// provider) is neither. Transport and authorization failures are not
/// statuses at all: they travel as `CoreResponse::Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspCompletionStatus {
    Ready,
    NotReady,
    Stale,
    Unsynced,
    Unsupported,
}

/// One supported plain-text insert/replace, with UTF-16 coordinates the
/// daemon validated against the exact snapshot the server saw.
///
/// The frontend must re-validate these coordinates against its own current
/// controller snapshot before applying: the daemon's snapshot may have moved
/// on by the time the user accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCompletionEditDto {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    /// Replacement text, bounded by [`MAX_LSP_COMPLETION_EDIT_BYTES`].
    pub new_text: String,
}

/// Why a visible candidate cannot be applied in v1.
///
/// Unsupported candidates stay visible with their reason rather than being
/// silently downgraded to label insertion: a downgrade would insert text the
/// server did not mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspCompletionUnsupportedReason {
    /// `insertTextFormat` is a snippet: placeholders cannot be applied.
    Snippet,
    /// Non-empty `additionalTextEdits`: more than one edit, possibly with
    /// imports the editor must not apply blindly.
    AdditionalEdits,
    /// A server `command` is attached: commands are never executed.
    Command,
    /// The edit range falls outside the snapshot the server saw.
    OutOfRange,
    /// The replacement text exceeds [`MAX_LSP_COMPLETION_EDIT_BYTES`].
    EditTooLarge,
    /// `insertAndReplace` ranges: not normalized in v1.
    InsertAndReplace,
}

/// Whether one candidate may be applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspCompletionApplicabilityDto {
    Applicable {
        edit: LspCompletionEditDto,
    },
    NonApplicable {
        reason: LspCompletionUnsupportedReason,
    },
}

/// One inert completion candidate.
///
/// Inert by construction: there is no command, no snippet body, and no
/// second edit. Display strings are truncated to their caps; only an
/// [`LspCompletionApplicabilityDto::Applicable`] edit may be applied, and
/// only after the frontend re-validates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCompletionCandidateDto {
    /// Display label, truncated to [`MAX_LSP_COMPLETION_LABEL_CHARS`].
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    /// LSP item kind as a stable lowercase name (`"function"`, …).
    #[serde(default)]
    pub kind: Option<String>,
    /// Server filter text for the M004 local filter, truncated like a label.
    #[serde(default)]
    pub filter_text: Option<String>,
    /// Server sort text, truncated like a label.
    #[serde(default)]
    pub sort_text: Option<String>,
    pub applicability: LspCompletionApplicabilityDto,
}

/// Result of one completion read.
///
/// `status` is authoritative and comes first in meaning: a caller must check
/// it before reading `candidates`, and every non-`Ready` result carries an
/// empty candidate list by construction. `document_id` and `revision` echo
/// the daemon's binding so a client discards responses with an unexpected
/// identity or revision instead of applying them to the wrong document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCompletionResultDto {
    pub document_id: String,
    /// Daemon-current revision at query time.
    pub revision: u64,
    /// Revision last delivered to the language server, when known.
    #[serde(default)]
    pub synced_revision: Option<u64>,
    pub status: LspCompletionStatus,
    #[serde(default)]
    pub candidates: Vec<LspCompletionCandidateDto>,
    /// `true` when the server truncated to the candidate or byte budget, so
    /// the client knows the list is not the server's whole answer.
    #[serde(default)]
    pub truncated: bool,
    /// The server's own `is_incomplete` flag: more may exist server-side.
    #[serde(default)]
    pub incomplete: bool,
}

impl LspCompletionResultDto {
    fn non_ready(
        document_id: String,
        revision: u64,
        synced_revision: Option<u64>,
        status: LspCompletionStatus,
    ) -> Self {
        Self {
            document_id,
            revision,
            synced_revision,
            status,
            candidates: Vec::new(),
            truncated: false,
            incomplete: false,
        }
    }

    /// No warm server (or a still-warming one). Starts nothing.
    pub fn not_ready(document_id: String) -> Self {
        Self::non_ready(document_id, 0, None, LspCompletionStatus::NotReady)
    }

    /// The caller's revision is not the daemon's current revision.
    pub fn stale(document_id: String, revision: u64, synced_revision: Option<u64>) -> Self {
        Self::non_ready(
            document_id,
            revision,
            synced_revision,
            LspCompletionStatus::Stale,
        )
    }

    /// The daemon's current revision was never delivered to the server.
    pub fn unsynced(document_id: String, revision: u64, synced_revision: Option<u64>) -> Self {
        Self::non_ready(
            document_id,
            revision,
            synced_revision,
            LspCompletionStatus::Unsynced,
        )
    }

    /// The warm server advertises no completion provider.
    pub fn unsupported(document_id: String, revision: u64, synced_revision: Option<u64>) -> Self {
        Self::non_ready(
            document_id,
            revision,
            synced_revision,
            LspCompletionStatus::Unsupported,
        )
    }

    /// A warm, synchronized answer. An empty candidate list is a real answer:
    /// the server had nothing to suggest.
    pub fn ready(
        document_id: String,
        revision: u64,
        synced_revision: Option<u64>,
        candidates: Vec<LspCompletionCandidateDto>,
        truncated: bool,
        incomplete: bool,
    ) -> Self {
        Self {
            document_id,
            revision,
            synced_revision,
            status: LspCompletionStatus::Ready,
            candidates,
            truncated,
            incomplete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(operation: LspReadOperation) -> LspReadRequestDto {
        LspReadRequestDto {
            operation,
            session_id: "session-1".to_string(),
            path: "src/lib.rs".to_string(),
            line: Some(3),
            column: Some(7),
            query: None,
        }
    }

    #[test]
    fn diagnostics_without_readiness_field_fail_closed_to_not_ready() {
        let result: LspDiagnosticsResultDto =
            serde_json::from_str(r#"{"project_id":"project-1","files":[],"truncated":false}"#)
                .expect("older daemon result remains deserializable");
        assert_eq!(result.status, LspDiagnosticsStatus::NotReady);
        assert!(result.files.is_empty());
    }

    #[test]
    fn older_location_responses_default_to_non_navigable() {
        let range: LspRangeDto = serde_json::from_str(
            r#"{"path":"lib.rs","start_line":1,"start_column":2,"end_line":1,"end_column":3}"#,
        )
        .expect("older response remains deserializable");
        assert_eq!(range.path, "lib.rs");
        assert_eq!(range.workspace_relative_path, None);
    }

    #[test]
    fn a_coherent_point_request_validates() {
        for operation in [
            LspReadOperation::Hover,
            LspReadOperation::Definition,
            LspReadOperation::References,
            LspReadOperation::DocumentSymbols,
            LspReadOperation::SemanticTokens,
        ] {
            assert_eq!(point(operation).validate(), Ok(()), "{operation:?}");
        }
    }

    #[test]
    fn document_symbols_do_not_require_a_position() {
        let mut request = point(LspReadOperation::DocumentSymbols);
        request.line = None;
        request.column = None;
        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn a_query_on_a_point_operation_is_rejected() {
        // A query is meaningful only for workspace symbols. Accepting it here
        // would let a caller believe it searched the workspace when it did not.
        let mut request = point(LspReadOperation::Hover);
        request.query = Some("Foo".to_string());
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
    }

    #[test]
    fn a_position_on_workspace_symbols_is_rejected() {
        let request = LspReadRequestDto {
            operation: LspReadOperation::WorkspaceSymbols,
            session_id: "session-1".to_string(),
            path: String::new(),
            line: Some(1),
            column: Some(1),
            query: Some("Foo".to_string()),
        };
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
    }

    #[test]
    fn workspace_symbols_require_a_non_empty_query() {
        let request = LspReadRequestDto {
            operation: LspReadOperation::WorkspaceSymbols,
            session_id: "session-1".to_string(),
            path: String::new(),
            line: None,
            column: None,
            query: Some(String::new()),
        };
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
    }

    #[test]
    fn an_oversized_symbol_query_is_rejected() {
        let request = LspReadRequestDto {
            operation: LspReadOperation::WorkspaceSymbols,
            session_id: "session-1".to_string(),
            path: String::new(),
            line: None,
            column: None,
            query: Some("x".repeat(MAX_LSP_SYMBOL_QUERY_BYTES + 1)),
        };
        assert_eq!(request.validate(), Err(LspReadInvalid::QueryTooLong));
    }

    #[test]
    fn a_point_request_without_a_path_is_rejected() {
        let mut request = point(LspReadOperation::Hover);
        request.path = String::new();
        assert_eq!(request.validate(), Err(LspReadInvalid::InvalidPath));
    }

    #[test]
    fn a_position_operation_without_a_position_is_rejected() {
        let mut request = point(LspReadOperation::Hover);
        request.line = None;
        request.column = None;
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
    }

    #[test]
    fn not_ready_carries_no_payload() {
        // The point of NotReady: a caller cannot render "clean" by accident.
        let result = LspReadResultDto::not_ready(LspReadOperation::Hover);
        assert_eq!(result.status, LspReadStatus::NotReady);
        assert!(result.payload.is_none());
    }

    // ---- M003 completion read surface ----

    fn completion_request() -> LspCompletionRequestDto {
        LspCompletionRequestDto {
            session_id: "session-1".to_string(),
            path: "src/lib.rs".to_string(),
            document_id: "doc-1".to_string(),
            revision: 4,
            line: 3,
            character: 7,
            trigger_character: None,
            max_candidates: 100,
        }
    }

    #[test]
    fn a_coherent_completion_request_validates() {
        assert_eq!(completion_request().validate(), Ok(()));
        assert_eq!(completion_request().capped_max_candidates(), 100);
    }

    #[test]
    fn a_completion_request_without_a_path_is_rejected() {
        let mut request = completion_request();
        request.path = String::new();
        assert_eq!(request.validate(), Err(LspReadInvalid::InvalidPath));
    }

    #[test]
    fn a_completion_request_without_an_attachment_is_rejected() {
        // The attachment is a locator, but an empty one is a client bug, not
        // a lookup miss: reject before any service call.
        let mut request = completion_request();
        request.document_id = String::new();
        assert_eq!(request.validate(), Err(LspReadInvalid::InvalidAttachment));
    }

    #[test]
    fn a_completion_request_with_zero_or_excessive_cap_is_rejected() {
        let mut request = completion_request();
        request.max_candidates = 0;
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
        request.max_candidates = MAX_LSP_COMPLETION_CANDIDATES as u32 + 1;
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
    }

    #[test]
    fn a_completion_request_with_a_bad_trigger_is_rejected() {
        let mut request = completion_request();
        request.trigger_character = Some(String::new());
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
        request.trigger_character = Some("x".repeat(MAX_LSP_COMPLETION_TRIGGER_CHARS + 1));
        assert_eq!(request.validate(), Err(LspReadInvalid::IncoherentFields));
        request.trigger_character = Some(".".to_string());
        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn non_ready_completion_results_carry_no_candidates() {
        for status in [
            LspCompletionStatus::NotReady,
            LspCompletionStatus::Stale,
            LspCompletionStatus::Unsynced,
            LspCompletionStatus::Unsupported,
        ] {
            let result = match status {
                LspCompletionStatus::NotReady => {
                    LspCompletionResultDto::not_ready("doc-1".to_string())
                }
                LspCompletionStatus::Stale => {
                    LspCompletionResultDto::stale("doc-1".to_string(), 9, Some(8))
                }
                LspCompletionStatus::Unsynced => {
                    LspCompletionResultDto::unsynced("doc-1".to_string(), 9, Some(8))
                }
                LspCompletionStatus::Unsupported => {
                    LspCompletionResultDto::unsupported("doc-1".to_string(), 9, Some(8))
                }
                LspCompletionStatus::Ready => unreachable!("not under test"),
            };
            assert_eq!(result.status, status);
            assert!(result.candidates.is_empty(), "{status:?} must not suggest");
            assert!(!result.truncated);
            assert!(!result.incomplete);
        }
    }

    #[test]
    fn stale_and_unsynced_echo_the_daemon_binding() {
        // The echo is what lets a client rebase (stale) or wait (unsynced)
        // instead of guessing.
        let stale = LspCompletionResultDto::stale("doc-1".to_string(), 9, Some(8));
        assert_eq!(stale.revision, 9);
        assert_eq!(stale.synced_revision, Some(8));
        let unsynced = LspCompletionResultDto::unsynced("doc-1".to_string(), 9, None);
        assert_eq!(unsynced.synced_revision, None);
    }

    #[test]
    fn a_ready_empty_completion_is_distinct_from_not_ready() {
        let ready = LspCompletionResultDto::ready(
            "doc-1".to_string(),
            9,
            Some(9),
            Vec::new(),
            false,
            false,
        );
        assert_eq!(ready.status, LspCompletionStatus::Ready);
        assert!(ready.candidates.is_empty());
        let cold = LspCompletionResultDto::not_ready("doc-1".to_string());
        assert_eq!(cold.status, LspCompletionStatus::NotReady);
        assert_ne!(ready.status, cold.status);
    }

    #[test]
    fn completion_status_names_are_stable_snake_case() {
        let json = serde_json::to_string(&LspCompletionStatus::NotReady).unwrap();
        assert_eq!(json, "\"not_ready\"");
        let back: LspCompletionStatus = serde_json::from_str("\"unsynced\"").unwrap();
        assert_eq!(back, LspCompletionStatus::Unsynced);
    }

    #[test]
    fn older_completion_wire_without_optional_fields_defaults_closed() {
        // Forward robustness: a result missing the additive fields must read
        // as "nothing suggested", never as applicable.
        let result: LspCompletionResultDto = serde_json::from_str(
            r#"{"document_id":"doc-1","revision":9,"status":"ready","candidates":[]}"#,
        )
        .expect("older result remains deserializable");
        assert_eq!(result.status, LspCompletionStatus::Ready);
        assert_eq!(result.synced_revision, None);
        assert!(!result.truncated);
        assert!(!result.incomplete);
    }

    #[test]
    fn completion_applicability_tags_are_stable() {
        let applicable = LspCompletionApplicabilityDto::Applicable {
            edit: LspCompletionEditDto {
                start_line: 1,
                start_character: 2,
                end_line: 1,
                end_character: 5,
                new_text: "foo".to_string(),
            },
        };
        let json = serde_json::to_string(&applicable).unwrap();
        assert!(json.contains("\"kind\":\"applicable\""), "{json}");
        let reason = LspCompletionApplicabilityDto::NonApplicable {
            reason: LspCompletionUnsupportedReason::AdditionalEdits,
        };
        let json = serde_json::to_string(&reason).unwrap();
        assert!(json.contains("\"kind\":\"non_applicable\""), "{json}");
        assert!(json.contains("\"additional_edits\""), "{json}");
    }

    #[test]
    fn the_pushed_file_is_the_same_type_as_the_authoritative_one() {
        // If these ever diverge, the stream and the pull could disagree and
        // the resync contract would be void. One type makes that impossible.
        let file = LspFileDiagnosticsDto {
            path: "src/lib.rs".to_string(),
            sequence: 7,
            digest: "abc".to_string(),
            diagnostics: Vec::new(),
            post_restart: false,
            truncated: false,
        };
        let projection = LspDiagnosticsProjectionDto {
            project_id: "project-1".to_string(),
            file: file.clone(),
        };
        assert_eq!(projection.file, file);
    }
}
