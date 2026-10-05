//! M006-B: client-side accessors for the native LSP read surface (ADR-0012).
//!
//! This crate is the frontend's only way to reach daemon-owned authority, so
//! the LSP reads live here rather than being issued ad hoc from the TUI. Two
//! contracts are encoded rather than left to each call site:
//!
//! - A read against a cold server is **`NotReady`, not an error and not an
//!   empty result.** [`LspReadClient::is_ready`] lets a caller branch on that
//!   without pattern-matching a status enum at every site.
//! - [`DiagnosticsReconciler`] is the resync contract. A client fed
//!   streamed diagnostics calls [`DiagnosticsReconciler::accept_streamed`]
//!   and [`DiagnosticsReconciler::reconcile`]; the reconciler decides whether
//!   a re-pull is required, so no caller has to remember the sequence rule.

use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::lsp::{
    LspDiagnosticsGetRequestDto, LspDiagnosticsResultDto, LspFileDiagnosticsDto, LspReadOperation,
    LspReadRequestDto, LspReadResultDto, LspReadStatus,
};

use crate::local::LocalSocketClient;

fn envelope(request: CoreRequest) -> RequestEnvelope<CoreRequest> {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        payload: request,
    }
}

/// Why an LSP read could not be completed by the client.
#[derive(Debug, thiserror::Error)]
pub enum LspReadClientError {
    #[error("lsp transport failed: {0}")]
    Transport(#[from] crate::ClientError),
    #[error("lsp service is unavailable")]
    Unavailable,
    #[error("the daemon rejected the read: {0}")]
    Rejected(String),
}

/// Whether a re-pull is required, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileDecision {
    /// The streamed sequence is current; nothing to do.
    Current,
    /// The authoritative sequence is ahead of what the client last saw, so at
    /// least one envelope was missed and the streamed view cannot be trusted.
    GapDetected { last_seen: u64, authoritative: u64 },
    /// The client has no sequence for this file at all, which is the state
    /// after a reconnect or a daemon restart.
    NoBaseline,
}

impl LocalSocketClient {
    /// One bounded LSP read.
    ///
    /// Returns the daemon's verdict as-is. In particular a `NotReady` result
    /// is returned, not turned into an error, so a caller can distinguish
    /// "no warm server" from "you may not ask".
    pub async fn lsp_read(
        &self,
        request: LspReadRequestDto,
    ) -> Result<LspReadResultDto, LspReadClientError> {
        let response = self
            .request(envelope(CoreRequest::LspReadGet { request }))
            .await?;
        match response {
            CoreResponse::LspReadResult { result } => Ok(result),
            CoreResponse::Error { message, .. } => Err(LspReadClientError::Rejected(message)),
            other => Err(LspReadClientError::Rejected(format!(
                "unexpected daemon response: {other:?}"
            ))),
        }
    }

    /// The authoritative current diagnostics for a project.
    ///
    /// This is the correctness authority. The project-scoped push stream is a
    /// latency optimization over it.
    pub async fn lsp_diagnostics(
        &self,
        project_id: String,
        session_id: String,
    ) -> Result<LspDiagnosticsResultDto, LspReadClientError> {
        let response = self
            .request(envelope(CoreRequest::LspDiagnosticsGet {
                request: LspDiagnosticsGetRequestDto {
                    project_id,
                    session_id,
                },
            }))
            .await?;
        match response {
            CoreResponse::LspDiagnosticsGetResult { result } => Ok(result),
            CoreResponse::Error { message, .. } => Err(LspReadClientError::Rejected(message)),
            other => Err(LspReadClientError::Rejected(format!(
                "unexpected daemon response: {other:?}"
            ))),
        }
    }
}

/// Convenience predicates so call sites read as intent rather than as enum
/// matching.
pub trait LspReadClient {
    /// `true` when the daemon had a warm server and answered.
    fn is_ready(result: &LspReadResultDto) -> bool;
    /// `true` when no server was warm and the read may be retried later.
    fn is_not_ready(result: &LspReadResultDto) -> bool;
}

impl LspReadClient for LspReadResultDto {
    fn is_ready(result: &LspReadResultDto) -> bool {
        result.status == LspReadStatus::Ready
    }

    fn is_not_ready(result: &LspReadResultDto) -> bool {
        result.status == LspReadStatus::NotReady
    }
}

/// Applies the ADR-0012 §2 resync rule.
///
/// The rule is small enough to fit in a few lines and important enough that it
/// should not be re-derived at each call site: the stream is only ever a
/// latency optimization, so a client that has missed an envelope must
/// re-pull rather than render a partial set.
#[derive(Debug, Default)]
pub struct DiagnosticsReconciler {
    /// Last sequence accepted from the stream, per file path.
    seen: std::collections::BTreeMap<String, u64>,
}

impl DiagnosticsReconciler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a streamed file update.
    ///
    /// Returns `false` when the update is older than what has already been
    /// accepted, so a reordered or replayed envelope cannot move the client
    /// backwards.
    pub fn accept_streamed(&mut self, file: &LspFileDiagnosticsDto) -> bool {
        match self.seen.get(&file.path) {
            Some(last) if *last >= file.sequence => false,
            _ => {
                self.seen.insert(file.path.clone(), file.sequence);
                true
            }
        }
    }

    /// Decide whether the streamed view can be trusted against the
    /// authoritative set.
    pub fn reconcile(&self, authoritative: &LspDiagnosticsResultDto) -> ReconcileDecision {
        // Track the widest gap rather than the first one found: a caller that
        // re-pulls once should learn how far behind it actually was, so a
        // single re-pull is visibly sufficient.
        let mut widest: Option<(u64, u64)> = None;
        let mut missing_baseline = false;
        for file in &authoritative.files {
            match self.seen.get(&file.path) {
                None => missing_baseline = true,
                Some(last) if *last < file.sequence => {
                    if widest
                        .is_none_or(|(seen, current)| (file.sequence - *last) > (current - seen))
                    {
                        widest = Some((*last, file.sequence));
                    }
                }
                Some(_) => {}
            }
        }
        if let Some((last_seen, authoritative_sequence)) = widest {
            return ReconcileDecision::GapDetected {
                last_seen,
                authoritative: authoritative_sequence,
            };
        }
        if missing_baseline {
            return ReconcileDecision::NoBaseline;
        }
        ReconcileDecision::Current
    }

    /// Adopt the authoritative set wholesale, which is what a re-pull does.
    pub fn adopt(&mut self, authoritative: &LspDiagnosticsResultDto) {
        self.seen.clear();
        for file in &authoritative.files {
            self.seen.insert(file.path.clone(), file.sequence);
        }
    }

    /// Forget one file, e.g. when its tab closes.
    pub fn forget(&mut self, path: &str) {
        self.seen.remove(path);
    }
}

/// The operation a caller asked for, for logging and for the TUI's status
/// line. Kept here so a caller does not have to thread the enum through.
pub fn operation_label(operation: LspReadOperation) -> &'static str {
    match operation {
        LspReadOperation::Hover => "hover",
        LspReadOperation::Definition => "definition",
        LspReadOperation::References => "references",
        LspReadOperation::DocumentSymbols => "document symbols",
        LspReadOperation::WorkspaceSymbols => "workspace symbols",
        LspReadOperation::SemanticTokens => "semantic tokens",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, sequence: u64) -> LspFileDiagnosticsDto {
        LspFileDiagnosticsDto {
            path: path.to_string(),
            sequence,
            digest: format!("digest-{sequence}"),
            diagnostics: Vec::new(),
            post_restart: false,
            truncated: false,
        }
    }

    fn authoritative(files: Vec<LspFileDiagnosticsDto>) -> LspDiagnosticsResultDto {
        LspDiagnosticsResultDto {
            project_id: "project-1".to_string(),
            files,
            truncated: false,
        }
    }

    #[test]
    fn a_fresh_stream_needs_no_resync() {
        let mut reconciler = DiagnosticsReconciler::new();
        reconciler.accept_streamed(&file("src/lib.rs", 3));
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 3)])),
            ReconcileDecision::Current
        );
    }

    #[test]
    fn a_missed_envelope_is_detected() {
        // The client saw sequence 1, the daemon moved to 3. Exactly the case
        // ADR-0012 §2 exists for: the stream dropped an envelope, so the
        // client must re-pull rather than render a stale set as current.
        let mut reconciler = DiagnosticsReconciler::new();
        reconciler.accept_streamed(&file("src/lib.rs", 1));
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 3)])),
            ReconcileDecision::GapDetected {
                last_seen: 1,
                authoritative: 3
            }
        );
    }

    #[test]
    fn a_client_with_no_baseline_must_pull() {
        // The post-restart state: no sequence history, so the streamed view
        // cannot be trusted at all.
        let reconciler = DiagnosticsReconciler::new();
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 1)])),
            ReconcileDecision::NoBaseline
        );
    }

    #[test]
    fn adopting_the_authoritative_set_clears_the_gap() {
        let mut reconciler = DiagnosticsReconciler::new();
        reconciler.accept_streamed(&file("src/lib.rs", 1));
        let current = authoritative(vec![file("src/lib.rs", 3)]);
        assert!(matches!(
            reconciler.reconcile(&current),
            ReconcileDecision::GapDetected { .. }
        ));
        reconciler.adopt(&current);
        assert_eq!(
            reconciler.reconcile(&current),
            ReconcileDecision::Current,
            "after a re-pull the client is in sync"
        );
    }

    #[test]
    fn a_replayed_or_reordered_envelope_cannot_move_the_client_backwards() {
        let mut reconciler = DiagnosticsReconciler::new();
        assert!(reconciler.accept_streamed(&file("src/lib.rs", 5)));
        assert!(
            !reconciler.accept_streamed(&file("src/lib.rs", 2)),
            "an older sequence must be refused"
        );
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 5)])),
            ReconcileDecision::Current
        );
    }

    #[test]
    fn a_forgotten_file_has_no_baseline() {
        let mut reconciler = DiagnosticsReconciler::new();
        reconciler.accept_streamed(&file("src/lib.rs", 4));
        reconciler.forget("src/lib.rs");
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 4)])),
            ReconcileDecision::NoBaseline
        );
    }

    #[test]
    fn readiness_is_distinguishable_from_an_empty_result() {
        let warming = LspReadResultDto::not_ready(LspReadOperation::Hover);
        assert!(LspReadResultDto::is_not_ready(&warming));
        assert!(!LspReadResultDto::is_ready(&warming));
        assert!(warming.payload.is_none());

        let ready = LspReadResultDto::ready(
            LspReadOperation::Hover,
            codegg_protocol::lsp::LspReadPayloadDto::Hover {
                text: String::new(),
                truncated: false,
            },
        );
        assert!(LspReadResultDto::is_ready(&ready));
        assert!(!LspReadResultDto::is_not_ready(&ready));
    }
}
