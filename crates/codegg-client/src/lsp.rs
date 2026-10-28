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

use codegg_protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION,
};
use codegg_protocol::lsp::{
    LspCompletionRequestDto, LspCompletionResultDto, LspDiagnosticsGetRequestDto,
    LspDiagnosticsResultDto, LspFileDiagnosticsDto, LspReadOperation, LspReadRequestDto,
    LspReadResultDto, LspReadStatus,
};
use codegg_protocol::projection::event::{
    ProjectionEnvelope, ProjectionEvent, ProjectionStreamScope,
};
use codegg_protocol::projection::replay::{
    ProjectionSnapshotBundle, ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
    ProjectionSubscriptionId,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::local::ClientEvent;
use crate::local::LocalSocketClient;

/// Trailing debounce for high-frequency completion and hover gestures. It is
/// paid only by the async request task; key handling and rendering remain
/// local and never wait for it.
pub const LSP_INTERACTIVE_DEBOUNCE: Duration = Duration::from_millis(40);

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
    /// The daemon has no authoritative diagnostics snapshot yet.
    NotReady,
    /// The streamed sequence is current; nothing to do.
    Current,
    /// The authoritative sequence is ahead of what the client last saw, so at
    /// least one envelope was missed and the streamed view cannot be trusted.
    GapDetected { last_seen: u64, authoritative: u64 },
    /// The client has no sequence for this file at all, which is the state
    /// after a reconnect or a daemon restart.
    NoBaseline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamedDiagnosticDecision {
    Accepted,
    Duplicate,
    Gap {
        last_seen: Option<u64>,
        received: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsStreamUpdate {
    Ignored,
    Updated(LspDiagnosticsResultDto),
    ReconcileRequired,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiagnosticsSubscriptionError {
    #[error("subscription does not belong to the requested project")]
    WrongProject,
    #[error("subscription is not for the project diagnostics stream")]
    WrongStream,
    #[error("subscription snapshot is not a diagnostics snapshot")]
    WrongSnapshot,
    #[error("subscription cursor does not match its descriptor")]
    InvalidCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LspRequestScope {
    pub frontend_id: String,
    pub attachment_id: String,
    pub route_generation: u64,
    pub connection_generation: u64,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub document_id: Option<String>,
    pub path: Option<String>,
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LspRequestOperation {
    Diagnostics,
    Read(LspReadOperation),
    Completion,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LspRequestKey {
    pub scope: LspRequestScope,
    pub operation: LspRequestOperation,
    pub document_generation: u64,
    pub position: Option<(u32, u32)>,
    pub argument: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LspRequestTicket {
    key: LspRequestKey,
    sequence: u64,
    cancellation: CancellationToken,
}

impl PartialEq for LspRequestTicket {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.sequence == other.sequence
    }
}

impl Eq for LspRequestTicket {}

impl LspRequestTicket {
    pub fn key(&self) -> &LspRequestKey {
        &self.key
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspRequestStart {
    Started(Box<LspRequestTicket>),
    AlreadyInFlight,
    AtCapacity,
}

#[derive(Debug, Default)]
struct LspRequestCoordinatorState {
    next_sequence: u64,
    outstanding: HashMap<u64, (LspRequestKey, CancellationToken)>,
    latest: HashMap<LspRequestKey, u64>,
}

#[derive(Debug, Clone)]
pub struct LspRequestCoordinator {
    state: Arc<Mutex<LspRequestCoordinatorState>>,
    max_outstanding: usize,
}

impl Default for LspRequestCoordinator {
    fn default() -> Self {
        Self::new(32)
    }
}

impl LspRequestCoordinator {
    pub fn new(max_outstanding: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(LspRequestCoordinatorState::default())),
            max_outstanding: max_outstanding.max(1),
        }
    }

    pub fn begin(&self, key: LspRequestKey) -> LspRequestStart {
        let Ok(mut state) = self.state.lock() else {
            return LspRequestStart::AtCapacity;
        };
        if state.latest.contains_key(&key) {
            return LspRequestStart::AlreadyInFlight;
        }
        let superseded: Vec<_> = state
            .latest
            .iter()
            .filter(|(pending, _)| {
                pending.scope.frontend_id == key.scope.frontend_id
                    && pending.scope.attachment_id == key.scope.attachment_id
                    && pending.scope.project_id == key.scope.project_id
                    && pending.scope.workspace_id == key.scope.workspace_id
                    && pending.scope.session_id == key.scope.session_id
                    && pending.operation == key.operation
            })
            .map(|(pending, sequence)| (pending.clone(), *sequence))
            .collect();
        for (pending, sequence) in superseded {
            state.latest.remove(&pending);
            if let Some((_, cancellation)) = state.outstanding.remove(&sequence) {
                cancellation.cancel();
            }
        }
        if state.outstanding.len() >= self.max_outstanding {
            return LspRequestStart::AtCapacity;
        }
        state.next_sequence = state.next_sequence.wrapping_add(1);
        let sequence = state.next_sequence;
        let cancellation = CancellationToken::new();
        state
            .outstanding
            .insert(sequence, (key.clone(), cancellation.clone()));
        state.latest.insert(key.clone(), sequence);
        LspRequestStart::Started(Box::new(LspRequestTicket {
            key,
            sequence,
            cancellation,
        }))
    }

    pub fn finish(
        &self,
        ticket: &LspRequestTicket,
        active_scope: &LspRequestScope,
        active_document_generation: u64,
    ) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some((key, _)) = state.outstanding.remove(&ticket.sequence) else {
            return false;
        };
        let is_current =
            key == ticket.key && state.latest.get(&ticket.key).copied() == Some(ticket.sequence);
        if is_current {
            state.latest.remove(&ticket.key);
        }
        is_current
            && &ticket.key.scope == active_scope
            && ticket.key.document_generation == active_document_generation
    }

    pub fn invalidate_scope(&self, scope: &LspRequestScope) {
        if let Ok(mut state) = self.state.lock() {
            let stale: Vec<_> = state
                .latest
                .iter()
                .filter(|(key, _)| &key.scope == scope)
                .map(|(key, sequence)| (key.clone(), *sequence))
                .collect();
            for (key, sequence) in stale {
                state.latest.remove(&key);
                if let Some((_, cancellation)) = state.outstanding.remove(&sequence) {
                    cancellation.cancel();
                }
            }
        }
    }

    /// Cancel one operation in a frontend scope without disturbing other work.
    pub fn invalidate_operation(&self, scope: &LspRequestScope, operation: LspRequestOperation) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let stale: Vec<_> = state
            .latest
            .iter()
            .filter(|(key, _)| key.scope == *scope && key.operation == operation)
            .map(|(key, sequence)| (key.clone(), *sequence))
            .collect();
        for (key, sequence) in stale {
            state.latest.remove(&key);
            if let Some((_, cancellation)) = state.outstanding.remove(&sequence) {
                cancellation.cancel();
            }
        }
    }

    pub fn invalidate_frontend(&self, frontend_id: &str) {
        if let Ok(mut state) = self.state.lock() {
            let stale: Vec<_> = state
                .latest
                .iter()
                .filter(|(key, _)| key.scope.frontend_id == frontend_id)
                .map(|(key, sequence)| (key.clone(), *sequence))
                .collect();
            for (key, sequence) in stale {
                state.latest.remove(&key);
                if let Some((_, cancellation)) = state.outstanding.remove(&sequence) {
                    cancellation.cancel();
                }
            }
        }
    }

    pub fn outstanding(&self) -> usize {
        self.state.lock().map_or(0, |state| state.outstanding.len())
    }
}

/// Project-scoped streamed diagnostics state. `LspDiagnosticsGet` remains the
/// authority; a stream gap marks this state for a pull before it can be shown
/// as current.
#[derive(Debug, Clone)]
pub struct LspDiagnosticsStream {
    project_id: String,
    subscription_id: ProjectionSubscriptionId,
    stream_id: ProjectionStreamId,
    projection_version: u32,
    cursor: u64,
    files: std::collections::BTreeMap<String, LspFileDiagnosticsDto>,
    reconciler: DiagnosticsReconciler,
    authoritative: Option<LspDiagnosticsResultDto>,
    needs_reconcile: bool,
}

impl LspDiagnosticsStream {
    pub fn from_subscription(
        project_id: &str,
        subscription_id: ProjectionSubscriptionId,
        descriptor: &ProjectionStreamDescriptor,
        snapshot: &ProjectionSnapshotBundle,
        cursor: &codegg_protocol::projection::replay::ProjectionCursor,
    ) -> Result<Self, DiagnosticsSubscriptionError> {
        if descriptor.project_id != project_id {
            return Err(DiagnosticsSubscriptionError::WrongProject);
        }
        if descriptor.kind != ProjectionStreamKind::Project {
            return Err(DiagnosticsSubscriptionError::WrongStream);
        }
        if cursor.stream_id != descriptor.stream_id
            || cursor.projection_version != descriptor.projection_version
            || cursor.event_seq != descriptor.high_water_seq
        {
            return Err(DiagnosticsSubscriptionError::InvalidCursor);
        }
        let ProjectionSnapshotBundle::LspDiagnostics {
            project_id: snapshot_project,
            files,
            ..
        } = snapshot
        else {
            return Err(DiagnosticsSubscriptionError::WrongSnapshot);
        };
        if snapshot_project != project_id {
            return Err(DiagnosticsSubscriptionError::WrongProject);
        }
        let mut reconciler = DiagnosticsReconciler::new();
        let mut indexed = std::collections::BTreeMap::new();
        for file in files {
            reconciler.adopt_file(file);
            indexed.insert(file.path.clone(), file.clone());
        }
        Ok(Self {
            project_id: project_id.to_owned(),
            subscription_id,
            stream_id: descriptor.stream_id.clone(),
            projection_version: descriptor.projection_version,
            cursor: cursor.event_seq,
            files: indexed,
            reconciler,
            authoritative: None,
            needs_reconcile: true,
        })
    }

    pub fn cursor(&self) -> codegg_protocol::projection::replay::ProjectionCursor {
        codegg_protocol::projection::replay::ProjectionCursor {
            stream_id: self.stream_id.clone(),
            event_seq: self.cursor,
            projection_version: self.projection_version,
        }
    }

    pub fn subscription_id(&self) -> &ProjectionSubscriptionId {
        &self.subscription_id
    }

    /// Apply a loss-aware socket event to this subscription. Any transport
    /// loss makes the displayed snapshot stale until an authorized pull is
    /// applied; unrelated events are ignored.
    pub fn accept_client_event(&mut self, event: &ClientEvent) -> DiagnosticsStreamUpdate {
        match event {
            ClientEvent::Lagged { .. } | ClientEvent::Gap { .. } | ClientEvent::Closed => {
                self.mark_transport_gap();
                DiagnosticsStreamUpdate::ReconcileRequired
            }
            ClientEvent::Event(event) => match &event.payload {
                CoreEvent::ProjectionStreamEvent {
                    subscription_id,
                    stream_id,
                    envelope,
                } => self.accept_projection_event(subscription_id, stream_id, envelope),
                _ => DiagnosticsStreamUpdate::Ignored,
            },
        }
    }

    pub fn needs_reconcile(&self) -> bool {
        self.needs_reconcile
    }

    pub fn current(&self) -> Option<&LspDiagnosticsResultDto> {
        self.authoritative.as_ref()
    }

    pub fn mark_transport_gap(&mut self) {
        self.needs_reconcile = true;
    }

    pub fn accept_projection_event(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
        stream_id: &ProjectionStreamId,
        envelope: &ProjectionEnvelope,
    ) -> DiagnosticsStreamUpdate {
        if subscription_id != &self.subscription_id
            || stream_id != &self.stream_id
            || envelope.scope != ProjectionStreamScope::Project
        {
            return DiagnosticsStreamUpdate::Ignored;
        }
        if envelope.event_seq <= self.cursor {
            return DiagnosticsStreamUpdate::Ignored;
        }
        let sequence_gap = envelope.event_seq != self.cursor.saturating_add(1);
        self.cursor = envelope.event_seq;
        if sequence_gap {
            self.needs_reconcile = true;
            return DiagnosticsStreamUpdate::ReconcileRequired;
        }
        let ProjectionEvent::LspDiagnosticsUpdated { file } = &envelope.payload else {
            return DiagnosticsStreamUpdate::Ignored;
        };
        if file.project_id != self.project_id {
            self.needs_reconcile = true;
            return DiagnosticsStreamUpdate::ReconcileRequired;
        }
        match self.reconciler.accept_streamed_checked(&file.file) {
            StreamedDiagnosticDecision::Duplicate => DiagnosticsStreamUpdate::Ignored,
            StreamedDiagnosticDecision::Gap { .. } => {
                self.needs_reconcile = true;
                DiagnosticsStreamUpdate::ReconcileRequired
            }
            StreamedDiagnosticDecision::Accepted => {
                if self.needs_reconcile || self.authoritative.is_none() {
                    self.needs_reconcile = true;
                    return DiagnosticsStreamUpdate::ReconcileRequired;
                }
                self.files.insert(file.file.path.clone(), file.file.clone());
                let mut current = self.authoritative.clone().expect("checked above");
                current.files = self.files.values().cloned().collect();
                self.authoritative = Some(current.clone());
                DiagnosticsStreamUpdate::Updated(current)
            }
        }
    }

    pub fn apply_authoritative(
        &mut self,
        authoritative: LspDiagnosticsResultDto,
    ) -> DiagnosticsStreamUpdate {
        if authoritative.project_id != self.project_id {
            self.needs_reconcile = true;
            return DiagnosticsStreamUpdate::ReconcileRequired;
        }
        if authoritative.status != codegg_protocol::lsp::LspDiagnosticsStatus::Ready {
            self.needs_reconcile = true;
            return DiagnosticsStreamUpdate::Updated(
                self.authoritative.clone().unwrap_or(authoritative),
            );
        }
        self.files = authoritative
            .files
            .iter()
            .cloned()
            .map(|file| (file.path.clone(), file))
            .collect();
        self.reconciler.adopt(&authoritative);
        self.needs_reconcile = authoritative.truncated;
        self.authoritative = Some(authoritative.clone());
        DiagnosticsStreamUpdate::Updated(authoritative)
    }
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

    /// One bounded inert LSP completion read (ADR-0021, Option B).
    ///
    /// Returns the daemon's verdict as-is: `Ready` (possibly with zero
    /// candidates), `NotReady`, `Stale`, `Unsynced` or `Unsupported`. Three
    /// things this accessor deliberately never does:
    ///
    /// - synthesize an edit from a candidate (candidates are inert data; only
    ///   an explicit user acceptance through the controller may apply one);
    /// - replay a request after a reconnect (a retried request gets a new
    ///   envelope and a fresh revision check daemon-side);
    /// - infer success from an old server's absence: an old daemon answers a
    ///   new variant with `Error`, which maps to `Rejected` below, never to
    ///   an empty `Ready`.
    pub async fn lsp_completion(
        &self,
        request: LspCompletionRequestDto,
    ) -> Result<LspCompletionResultDto, LspReadClientError> {
        let response = self
            .request(envelope(CoreRequest::LspCompletionGet { request }))
            .await?;
        map_completion_response(response)
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

/// Map one daemon envelope to a completion verdict.
///
/// Pure so the mapping is unit-testable without a socket: every shape is
/// covered, and an unknown response is a loud `Rejected`, never an inferred
/// success.
pub fn map_completion_response(
    response: CoreResponse,
) -> Result<LspCompletionResultDto, LspReadClientError> {
    match response {
        CoreResponse::LspCompletionResult { result } => Ok(result),
        CoreResponse::Error { message, .. } => Err(LspReadClientError::Rejected(message)),
        other => Err(LspReadClientError::Rejected(format!(
            "unexpected daemon response: {other:?}"
        ))),
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
#[derive(Debug, Default, Clone)]
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

    pub fn accept_streamed_checked(
        &mut self,
        file: &LspFileDiagnosticsDto,
    ) -> StreamedDiagnosticDecision {
        match self.seen.get(&file.path).copied() {
            Some(last) if file.sequence <= last => StreamedDiagnosticDecision::Duplicate,
            Some(last) if file.sequence != last.saturating_add(1) => {
                StreamedDiagnosticDecision::Gap {
                    last_seen: Some(last),
                    received: file.sequence,
                }
            }
            None if file.sequence != 1 => StreamedDiagnosticDecision::Gap {
                last_seen: None,
                received: file.sequence,
            },
            _ => {
                self.seen.insert(file.path.clone(), file.sequence);
                StreamedDiagnosticDecision::Accepted
            }
        }
    }

    fn adopt_file(&mut self, file: &LspFileDiagnosticsDto) {
        self.seen.insert(file.path.clone(), file.sequence);
    }

    /// Decide whether the streamed view can be trusted against the
    /// authoritative set.
    pub fn reconcile(&self, authoritative: &LspDiagnosticsResultDto) -> ReconcileDecision {
        if authoritative.status != codegg_protocol::lsp::LspDiagnosticsStatus::Ready {
            return ReconcileDecision::NotReady;
        }
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
        if authoritative.status != codegg_protocol::lsp::LspDiagnosticsStatus::Ready {
            return;
        }
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
        LspReadOperation::Declaration => "declaration",
        LspReadOperation::Implementation => "implementation",
        LspReadOperation::References => "references",
        LspReadOperation::DocumentHighlight => "document highlights",
        LspReadOperation::SelectionRange => "selection range",
        LspReadOperation::DocumentSymbols => "document symbols",
        LspReadOperation::WorkspaceSymbols => "workspace symbols",
        LspReadOperation::SemanticTokens => "semantic tokens",
        LspReadOperation::SignatureHelp => "signature help",
        LspReadOperation::FormatPreview => "format preview",
        LspReadOperation::RenamePreview => "rename preview",
        LspReadOperation::CodeActionPreview => "code action preview",
    }
}

#[cfg(test)]
mod coordinator_tests {
    use super::*;

    fn key(scope: LspRequestScope, generation: u64) -> LspRequestKey {
        LspRequestKey {
            scope,
            operation: LspRequestOperation::Read(LspReadOperation::Hover),
            document_generation: generation,
            position: Some((2, 4)),
            argument: None,
        }
    }

    fn scope() -> LspRequestScope {
        LspRequestScope {
            frontend_id: "tui-1".into(),
            attachment_id: "attachment-1".into(),
            route_generation: 3,
            connection_generation: 2,
            project_id: "project-1".into(),
            workspace_id: "workspace-1".into(),
            session_id: "session-1".into(),
            document_id: Some("document-1".into()),
            path: Some("src/lib.rs".into()),
            server_id: Some("rust-analyzer".into()),
        }
    }

    #[test]
    fn coordinator_collapses_equivalent_work_and_fences_scope_and_generation() {
        let coordinator = LspRequestCoordinator::new(2);
        let scope = scope();
        let ticket = match coordinator.begin(key(scope.clone(), 8)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        assert_eq!(
            coordinator.begin(key(scope.clone(), 8)),
            LspRequestStart::AlreadyInFlight
        );
        assert!(!coordinator.finish(&ticket, &scope, 9));
        assert_eq!(coordinator.outstanding(), 0);

        let ticket = match coordinator.begin(key(scope.clone(), 8)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        let mut other_attachment = scope.clone();
        other_attachment.attachment_id = "attachment-2".into();
        assert!(!coordinator.finish(&ticket, &other_attachment, 8));
        assert_eq!(coordinator.outstanding(), 0);
    }

    #[test]
    fn coordinator_bounds_outstanding_work_and_old_finish_cannot_retire_new_ticket() {
        let coordinator = LspRequestCoordinator::new(2);
        let scope = scope();
        let old = match coordinator.begin(key(scope.clone(), 8)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        let mut new_scope = scope.clone();
        new_scope.route_generation += 1;
        let mut new_key = key(new_scope.clone(), 9);
        new_key.operation = LspRequestOperation::Completion;
        let new = match coordinator.begin(new_key.clone()) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        let mut third_key = key(new_scope.clone(), 9);
        third_key.scope.frontend_id = "desktop".into();
        assert_eq!(coordinator.begin(third_key), LspRequestStart::AtCapacity);
        coordinator.invalidate_scope(&scope);
        assert!(!coordinator.finish(&old, &scope, 8));
        assert_eq!(coordinator.outstanding(), 1);
        assert!(coordinator.finish(&new, &new_scope, 9));
        assert_eq!(coordinator.outstanding(), 0);
    }

    #[test]
    fn coordinator_keeps_same_operation_independent_across_attachments() {
        let coordinator = LspRequestCoordinator::new(4);
        let first_scope = scope();
        let mut second_scope = first_scope.clone();
        second_scope.attachment_id = "attachment-2".into();
        second_scope.document_id = Some("document-2".into());
        let first = match coordinator.begin(key(first_scope.clone(), 8)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        let second = match coordinator.begin(key(second_scope.clone(), 8)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        assert_eq!(coordinator.outstanding(), 2);
        assert!(coordinator.finish(&first, &first_scope, 8));
        assert!(coordinator.finish(&second, &second_scope, 8));
        assert_eq!(coordinator.outstanding(), 0);
    }

    #[test]
    fn diagnostics_invalidation_cancels_delayed_response_without_touching_reads() {
        let coordinator = LspRequestCoordinator::new(4);
        let scope = scope();
        let mut diagnostics_key = key(scope.clone(), 8);
        diagnostics_key.operation = LspRequestOperation::Diagnostics;
        let mut read_key = key(scope.clone(), 8);
        read_key.operation = LspRequestOperation::Read(LspReadOperation::Hover);
        let diagnostics = match coordinator.begin(diagnostics_key) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected diagnostics admission: {other:?}"),
        };
        let read = match coordinator.begin(read_key) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected read admission: {other:?}"),
        };

        coordinator.invalidate_operation(&scope, LspRequestOperation::Diagnostics);

        assert!(diagnostics.cancellation_token().is_cancelled());
        assert!(!coordinator.finish(&diagnostics, &scope, 8));
        assert!(coordinator.finish(&read, &scope, 8));
        assert_eq!(coordinator.outstanding(), 0);
    }

    #[test]
    fn coordinator_cancels_superseded_and_invalidated_requests() {
        let coordinator = LspRequestCoordinator::new(4);
        let scope = scope();
        let old = match coordinator.begin(key(scope.clone(), 8)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        let old_cancel = old.cancellation_token();
        let current = match coordinator.begin(key(scope.clone(), 9)) {
            LspRequestStart::Started(ticket) => ticket,
            other => panic!("unexpected admission: {other:?}"),
        };
        assert!(old_cancel.is_cancelled());
        assert!(!coordinator.finish(&old, &scope, 8));
        let current_cancel = current.cancellation_token();
        coordinator.invalidate_frontend("tui-1");
        assert!(current_cancel.is_cancelled());
        assert!(!coordinator.finish(&current, &scope, 9));
    }

    #[test]
    fn coordinator_keeps_only_the_latest_of_one_hundred_rapid_editor_requests() {
        let coordinator = LspRequestCoordinator::new(2);
        let scope = scope();
        let mut previous: Option<Box<LspRequestTicket>> = None;
        for generation in 1..=100 {
            let mut request = key(scope.clone(), generation);
            request.position = Some((generation as u32, 4));
            let ticket = match coordinator.begin(request) {
                LspRequestStart::Started(ticket) => ticket,
                other => panic!("request {generation} was not admitted: {other:?}"),
            };
            if let Some(old) = previous.take() {
                assert!(old.cancellation_token().is_cancelled());
                assert!(!coordinator.finish(&old, &scope, old.key().document_generation));
            }
            assert_eq!(coordinator.outstanding(), 1);
            previous = Some(ticket);
        }
        let latest = previous.expect("one latest request");
        assert!(coordinator.finish(&latest, &scope, 100));
        assert_eq!(coordinator.outstanding(), 0);
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
            status: codegg_protocol::lsp::LspDiagnosticsStatus::Ready,
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
    fn not_ready_pull_preserves_the_last_complete_diagnostics_baseline() {
        let mut reconciler = DiagnosticsReconciler::new();
        reconciler.accept_streamed(&file("src/lib.rs", 4));
        let not_ready = LspDiagnosticsResultDto {
            project_id: "project-1".into(),
            status: codegg_protocol::lsp::LspDiagnosticsStatus::NotReady,
            files: Vec::new(),
            truncated: false,
        };
        assert_eq!(
            reconciler.reconcile(&not_ready),
            ReconcileDecision::NotReady
        );
        reconciler.adopt(&not_ready);
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 4)])),
            ReconcileDecision::Current,
            "NotReady must not erase the prior accepted sequence"
        );
    }

    #[test]
    fn ready_empty_pull_is_authoritative_and_clears_the_baseline() {
        let mut reconciler = DiagnosticsReconciler::new();
        reconciler.accept_streamed(&file("src/lib.rs", 4));
        let empty = authoritative(Vec::new());
        reconciler.adopt(&empty);
        assert_eq!(reconciler.reconcile(&empty), ReconcileDecision::Current);
        assert_eq!(
            reconciler.reconcile(&authoritative(vec![file("src/lib.rs", 5)])),
            ReconcileDecision::NoBaseline
        );
    }

    fn diagnostics_stream() -> LspDiagnosticsStream {
        diagnostics_stream_for("subscription-1")
    }

    fn diagnostics_stream_for(subscription_id: &str) -> LspDiagnosticsStream {
        use codegg_protocol::projection::replay::{
            ProjectionCursor, ProjectionSnapshotBundle, ProjectionStreamDescriptor,
            ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
        };
        let descriptor = ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new("project-stream").expect("stream id"),
            kind: ProjectionStreamKind::Project,
            project_id: "project-1".into(),
            workspace_id: None,
            session_id: None,
            projection_version: codegg_protocol::projection::caps::PROJECTION_PROTOCOL_VERSION,
            retention_floor_seq: 0,
            high_water_seq: 1,
            latest_checkpoint_seq: None,
        };
        let cursor = ProjectionCursor {
            stream_id: descriptor.stream_id.clone(),
            event_seq: 1,
            projection_version: descriptor.projection_version,
        };
        let snapshot = ProjectionSnapshotBundle::LspDiagnostics {
            project_id: "project-1".into(),
            files: vec![file("src/lib.rs", 1)],
            truncated: false,
        };
        LspDiagnosticsStream::from_subscription(
            "project-1",
            ProjectionSubscriptionId::new(subscription_id),
            &descriptor,
            &snapshot,
            &cursor,
        )
        .expect("valid diagnostics subscription")
    }

    fn projection_event(
        event_seq: u64,
        diagnostic_file: LspFileDiagnosticsDto,
    ) -> ProjectionEnvelope {
        ProjectionEnvelope {
            protocol_version: codegg_protocol::projection::caps::PROJECTION_PROTOCOL_VERSION,
            event_seq,
            timestamp_ms: 1,
            session_id: None,
            turn_id: None,
            scope: ProjectionStreamScope::Project,
            payload: ProjectionEvent::LspDiagnosticsUpdated {
                file: codegg_protocol::lsp::LspDiagnosticsProjectionDto {
                    project_id: "project-1".into(),
                    file: diagnostic_file,
                },
            },
        }
    }

    #[test]
    fn diagnostics_stream_requires_authoritative_pull_after_snapshot() {
        let mut stream = diagnostics_stream();
        assert!(stream.needs_reconcile());
        assert!(
            stream.current().is_none(),
            "the status-free subscription snapshot is not an authoritative Ready result"
        );
        let ready = authoritative(vec![file("src/lib.rs", 1)]);
        assert_eq!(
            stream.apply_authoritative(ready.clone()),
            DiagnosticsStreamUpdate::Updated(ready)
        );
        assert!(!stream.needs_reconcile());
    }

    #[test]
    fn diagnostics_stream_applies_contiguous_update_and_requests_pull_on_gap() {
        use codegg_protocol::projection::replay::{ProjectionStreamId, ProjectionSubscriptionId};
        let mut stream = diagnostics_stream();
        stream.apply_authoritative(authoritative(vec![file("src/lib.rs", 1)]));
        let subscription = ProjectionSubscriptionId::new("subscription-1");
        let stream_id = ProjectionStreamId::new("project-stream").expect("stream id");
        assert!(matches!(
            stream.accept_projection_event(
                &subscription,
                &stream_id,
                &projection_event(2, file("src/lib.rs", 2)),
            ),
            DiagnosticsStreamUpdate::Updated(_)
        ));
        assert_eq!(
            stream.accept_projection_event(
                &subscription,
                &stream_id,
                &projection_event(4, file("src/lib.rs", 3)),
            ),
            DiagnosticsStreamUpdate::ReconcileRequired
        );
        assert!(stream.needs_reconcile());
    }

    #[test]
    fn loss_aware_client_event_marks_the_snapshot_stale_and_accepts_project_updates() {
        use codegg_protocol::core::{CoreEvent, EventEnvelope};
        use codegg_protocol::projection::replay::{ProjectionStreamId, ProjectionSubscriptionId};

        let mut stream = diagnostics_stream();
        stream.apply_authoritative(authoritative(vec![file("src/lib.rs", 1)]));
        let envelope = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 2,
            timestamp_ms: 1,
            session_id: None,
            turn_id: None,
            payload: CoreEvent::ProjectionStreamEvent {
                subscription_id: ProjectionSubscriptionId::new("subscription-1"),
                stream_id: ProjectionStreamId::new("project-stream").expect("stream id"),
                envelope: projection_event(2, file("src/lib.rs", 2)),
            },
        };
        assert!(matches!(
            stream.accept_client_event(&ClientEvent::Event(Box::new(envelope))),
            DiagnosticsStreamUpdate::Updated(_)
        ));
        assert_eq!(
            stream.accept_client_event(&ClientEvent::Lagged { dropped: 1 }),
            DiagnosticsStreamUpdate::ReconcileRequired
        );
        assert!(stream.needs_reconcile());
    }

    #[test]
    fn two_frontend_streams_receive_the_same_project_update_independently() {
        use codegg_protocol::projection::replay::{ProjectionStreamId, ProjectionSubscriptionId};

        let mut tui = diagnostics_stream_for("subscription-tui");
        let mut desktop = diagnostics_stream_for("subscription-desktop");
        let baseline = authoritative(vec![file("src/lib.rs", 1)]);
        assert_eq!(
            tui.apply_authoritative(baseline.clone()),
            DiagnosticsStreamUpdate::Updated(baseline.clone())
        );
        assert_eq!(
            desktop.apply_authoritative(baseline.clone()),
            DiagnosticsStreamUpdate::Updated(baseline)
        );
        let stream_id = ProjectionStreamId::new("project-stream").expect("stream id");
        let update = projection_event(2, file("src/lib.rs", 2));
        let tui_update = tui.accept_projection_event(
            &ProjectionSubscriptionId::new("subscription-tui"),
            &stream_id,
            &update,
        );
        let desktop_update = desktop.accept_projection_event(
            &ProjectionSubscriptionId::new("subscription-desktop"),
            &stream_id,
            &update,
        );
        assert_eq!(tui_update, desktop_update);
        assert_eq!(tui.current(), desktop.current());
        assert_eq!(
            tui.current().map(|result| result.files[0].sequence),
            Some(2)
        );
    }

    #[test]
    fn diagnostics_stream_gap_recovers_from_authoritative_pull() {
        use codegg_protocol::projection::replay::{ProjectionStreamId, ProjectionSubscriptionId};

        let mut stream = diagnostics_stream();
        let first = authoritative(vec![file("src/lib.rs", 1)]);
        stream.apply_authoritative(first);
        let subscription = ProjectionSubscriptionId::new("subscription-1");
        let stream_id = ProjectionStreamId::new("project-stream").expect("stream id");
        assert_eq!(
            stream.accept_projection_event(
                &subscription,
                &stream_id,
                &projection_event(4, file("src/lib.rs", 4)),
            ),
            DiagnosticsStreamUpdate::ReconcileRequired
        );
        assert!(stream.needs_reconcile());
        assert_eq!(
            stream.current().map(|result| result.files[0].sequence),
            Some(1),
            "the last complete view remains visible while stale"
        );

        let recovered = authoritative(vec![file("src/lib.rs", 4)]);
        assert_eq!(
            stream.apply_authoritative(recovered.clone()),
            DiagnosticsStreamUpdate::Updated(recovered)
        );
        assert!(!stream.needs_reconcile());
        assert_eq!(
            stream.accept_projection_event(
                &subscription,
                &stream_id,
                &projection_event(5, file("src/lib.rs", 5)),
            ),
            DiagnosticsStreamUpdate::Updated(authoritative(vec![file("src/lib.rs", 5)]))
        );
    }

    #[test]
    fn diagnostics_not_ready_keeps_last_ready_set_and_marks_it_stale() {
        let mut stream = diagnostics_stream();
        let ready = authoritative(vec![file("src/lib.rs", 1)]);
        stream.apply_authoritative(ready.clone());
        let not_ready = LspDiagnosticsResultDto {
            project_id: "project-1".into(),
            status: codegg_protocol::lsp::LspDiagnosticsStatus::NotReady,
            files: Vec::new(),
            truncated: false,
        };
        assert_eq!(
            stream.apply_authoritative(not_ready),
            DiagnosticsStreamUpdate::Updated(ready)
        );
        assert!(stream.needs_reconcile());
    }

    #[test]
    fn completion_result_passes_through_verbatim() {
        use codegg_protocol::lsp::{LspCompletionResultDto, LspCompletionStatus};
        let result = LspCompletionResultDto::ready(
            "doc-1".to_string(),
            9,
            Some(9),
            Vec::new(),
            false,
            false,
        );
        let back = map_completion_response(CoreResponse::LspCompletionResult {
            result: result.clone(),
        })
        .expect("a completion result is a success");
        assert_eq!(back, result);
        assert_eq!(back.status, LspCompletionStatus::Ready);
    }

    #[test]
    fn completion_rejection_is_never_an_inferred_success() {
        // An old daemon answers the unknown variant with `Error`; a confused
        // daemon answers something else. Both must be loud rejections.
        let denied = map_completion_response(CoreResponse::Error {
            code: "unsupported".to_string(),
            message: "unknown".to_string(),
        });
        assert!(matches!(denied, Err(LspReadClientError::Rejected(_))));
        let confused = map_completion_response(CoreResponse::Ack);
        assert!(matches!(confused, Err(LspReadClientError::Rejected(_))));
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
