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

/// The authoritative diagnostics for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnosticsResultDto {
    pub project_id: String,
    pub files: Vec<LspFileDiagnosticsDto>,
    /// `true` when the file list itself was capped, so a client knows the set
    /// it received is not the whole project.
    pub truncated: bool,
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
