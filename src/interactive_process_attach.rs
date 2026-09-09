//! Bounded attach/resume protocol and attachment authority (M002).
//!
//! This module exposes the M001 scheduler-owned PTY engine
//! ([`InteractiveProcessService`]) through the versioned daemon operation
//! family `create/list/attach/detach/input/resize/terminate/remove/resume`.
//! [`InteractiveProcessProtocol`] is the daemon-owned handler family: every
//! method takes the trusted transport `client_id` alongside the request so
//! attachment ownership is transport-derived, never caller-supplied.
//!
//! ```text
//! create(client, ctx, dto)        -> handle (no attachment yet)
//! attach(client, handle, cursor)  -> attachment_id + bounded chunk
//! input/resize/terminate/remove(client, attachment_id, ...)
//! resume(client, attachment_id, cursor) -> chunk | typed resync
//! detach(client, attachment_id) / handle_disconnect(client)
//! ```
//!
//! # Authority model
//!
//! - Request DTOs carry no identity: [`InteractiveAuthority`] is built from
//!   transport evidence (`ClientRegistry::principal_for`) by the daemon.
//!   Unregistered (in-process/stdio/local) connections resolve to
//!   [`InteractiveTransport::LocalOwner`]. A future identity plug can grant
//!   the semantic terminate capability through
//!   [`InteractiveAuthority::with_terminate_capability`] without a wire change.
//! - Mutating operations name an `attachment_id`. The daemon resolves the
//!   process handle server-side from the caller-owned attachment, so a
//!   client cannot drive another process by copying IDs. Unknown and
//!   foreign attachment IDs answer with the same `interactive_attachment_gone`
//!   code so callers cannot probe for other clients' attachments.
//! - Terminate/remove additionally require [`InteractiveAuthority::can_terminate`]
//!   (local-owner transport or the semantic capability).
//! - Detach and disconnect only drop attachments. The process follows the
//!   M001 owner/idle policy: it keeps running until explicit terminate,
//!   natural exit, remove, or daemon shutdown.
//!
//! # Bounded resources
//!
//! Output integration reuses the M001 shared ring: reads are on-demand from a
//! sequence cursor, so there is no per-attachment queue and no background
//! task per attachment (M001 owns exactly two bounded tasks per session).
//! Attachment counts are capped per client, per process, and daemon-wide.
//! Lag is typed ([`InteractiveResync`]) rather than silently shifted.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use codegg_core::workspace::ExecutionContext;
use dashmap::DashMap;
use thiserror::Error;

use crate::interactive_process::{
    InteractiveError, InteractiveHandle, InteractiveProcessService, PtySize, SessionSnapshot,
    SpawnSpec,
};
use crate::protocol::core::CoreResponse;
use crate::protocol::interactive_process::{
    validate_attachment_id, validate_interactive_id, InteractiveOutputChunk,
    InteractiveProcessCapabilities, InteractiveProcessCreateRequest, InteractiveProcessMetadata,
    InteractiveResync, InteractiveResyncReason, DEFAULT_INTERACTIVE_CHUNK_BYTES,
    INTERACTIVE_PROCESS_PROTOCOL_VERSION, MAX_ATTACHMENTS_PER_CLIENT, MAX_ATTACHMENTS_PER_DAEMON,
    MAX_ATTACHMENTS_PER_PROCESS, MAX_INTERACTIVE_CHUNK_BYTES, MAX_INTERACTIVE_ID_LENGTH,
    MAX_INTERACTIVE_INPUT_BYTES, MAX_INTERACTIVE_LIST_ITEMS,
};

/// Opaque caller-owned attachment to one interactive process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttachmentId(String);

impl AttachmentId {
    fn fresh() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Rebuild from a wire string. Shape-checked only; ownership is
    /// checked against the registry by the caller.
    pub fn parse(value: &str) -> Result<Self, InteractiveAttachError> {
        validate_attachment_id(value)
            .map_err(|_| InteractiveAttachError::InvalidRequest("invalid attachment id"))?;
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One caller-owned attachment record.
#[derive(Debug)]
pub struct AttachmentRecord {
    /// Opaque attachment identifier issued at attach time.
    pub id: AttachmentId,
    /// Process handle this attachment observes (resolved server-side).
    pub handle: String,
    /// Transport `client_id` that owns this attachment.
    pub client_id: String,
    /// Last cursor the owner resumed from (observability only; the
    /// authoritative cursor travels in each resume request).
    pub last_from_seq: AtomicU64,
    /// Wall-clock attach time (millis since epoch, diagnostics only).
    pub attached_at_ms: i64,
}

/// Transport-derived attachment ownership registry.
///
/// Keyed by attachment id; every lookup additionally checks the caller's
/// transport `client_id`, so attachment IDs are useless across owners.
pub struct InteractiveAttachmentRegistry {
    attachments: DashMap<String, AttachmentRecord>,
}

impl Default for InteractiveAttachmentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveAttachmentRegistry {
    /// Build an empty registry. Registries are ephemeral: a daemon restart
    /// drops every attachment alongside the M001 handles they observe.
    pub fn new() -> Self {
        Self {
            attachments: DashMap::new(),
        }
    }

    /// Number of live attachments daemon-wide.
    pub fn len(&self) -> usize {
        self.attachments.len()
    }

    /// `true` when no attachment is tracked.
    pub fn is_empty(&self) -> bool {
        self.attachments.is_empty()
    }

    /// Attachments owned by one transport client.
    pub fn client_count(&self, client_id: &str) -> usize {
        self.attachments
            .iter()
            .filter(|entry| entry.client_id == client_id)
            .count()
    }

    /// Attachments observing one process handle.
    pub fn process_count(&self, handle: &str) -> usize {
        self.attachments
            .iter()
            .filter(|entry| entry.handle == handle)
            .count()
    }

    /// Existing attachment for one (client, handle) pair, if any.
    /// Re-attach is idempotent: the same client re-attaching to the same
    /// handle keeps its cursor model instead of forking attachments.
    pub fn existing(&self, client_id: &str, handle: &str) -> Option<AttachmentId> {
        self.attachments
            .iter()
            .find(|entry| entry.client_id == client_id && entry.handle == handle)
            .map(|entry| entry.id.clone())
    }

    /// Record one attachment, enforcing per-client, per-process, and
    /// daemon-wide bounds. Callers must have verified the handle exists.
    pub fn attach(
        &self,
        client_id: &str,
        handle: &str,
    ) -> Result<AttachmentId, InteractiveAttachError> {
        validate_client_id(client_id)?;
        if let Some(id) = self.existing(client_id, handle) {
            return Ok(id);
        }
        if self.client_count(client_id) >= MAX_ATTACHMENTS_PER_CLIENT {
            return Err(InteractiveAttachError::AttachmentLimitExceeded { scope: "client" });
        }
        if self.process_count(handle) >= MAX_ATTACHMENTS_PER_PROCESS {
            return Err(InteractiveAttachError::AttachmentLimitExceeded { scope: "process" });
        }
        if self.len() >= MAX_ATTACHMENTS_PER_DAEMON {
            return Err(InteractiveAttachError::AttachmentLimitExceeded { scope: "daemon" });
        }
        let id = AttachmentId::fresh();
        self.attachments.insert(
            id.as_str().to_string(),
            AttachmentRecord {
                id: id.clone(),
                handle: handle.to_string(),
                client_id: client_id.to_string(),
                last_from_seq: AtomicU64::new(0),
                attached_at_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        Ok(id)
    }

    /// Resolve one caller-owned attachment. Unknown and foreign IDs fail
    /// with errors that share one wire code (`interactive_attachment_gone`)
    /// so callers cannot probe for other clients' attachments.
    pub fn resolve(
        &self,
        client_id: &str,
        attachment_id: &AttachmentId,
    ) -> Result<dashmap::mapref::one::Ref<'_, String, AttachmentRecord>, InteractiveAttachError>
    {
        let entry = self
            .attachments
            .get(attachment_id.as_str())
            .ok_or(InteractiveAttachError::UnknownAttachment)?;
        if entry.client_id != client_id {
            return Err(InteractiveAttachError::NotOwner);
        }
        Ok(entry)
    }

    /// Release one caller-owned attachment. The process is unaffected.
    pub fn detach(
        &self,
        client_id: &str,
        attachment_id: &AttachmentId,
    ) -> Result<(), InteractiveAttachError> {
        let record = self.resolve(client_id, attachment_id)?;
        let owned_client = record.client_id.clone();
        let key = attachment_id.as_str().to_string();
        drop(record);
        debug_assert_eq!(owned_client, client_id);
        self.attachments.remove(&key);
        Ok(())
    }

    /// Drop every attachment owned by one transport connection.
    ///
    /// Connection-close/EOF cleanup path: attachments are transient
    /// subscriptions, so disconnect releases them without touching the
    /// underlying processes (which follow the M001 owner/idle policy).
    /// Returns the number of attachments released.
    pub fn handle_disconnect(&self, client_id: &str) -> usize {
        let keys: Vec<String> = self
            .attachments
            .iter()
            .filter(|entry| entry.client_id == client_id)
            .map(|entry| entry.key().clone())
            .collect();
        let released = keys.len();
        for key in keys {
            self.attachments.remove(&key);
        }
        released
    }
}

/// Which transport class bound the connection. Derived from the daemon's
/// `ClientRegistry` principal (handshake evidence), never from payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveTransport {
    /// Trusted local transport (Unix socket, stdio, in-process) with no
    /// login ceremony. Full interactive authority.
    LocalOwner,
    /// Authenticated remote principal. Observe/attach/input/resize follow
    /// attachment ownership; terminate/remove need the semantic capability
    /// until a later milestone binds project-scoped interactive rights.
    AuthenticatedRemote,
}

/// Transport-derived authority for one connection.
///
/// This is the explicit authorization seam: later identity work plugs
/// semantic capabilities through [`Self::with_terminate_capability`]
/// without changing the wire contract.
#[derive(Debug, Clone)]
pub struct InteractiveAuthority {
    client_id: String,
    transport: InteractiveTransport,
    terminate_capability: bool,
}

impl InteractiveAuthority {
    /// Authority for a trusted local connection.
    pub fn local(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            transport: InteractiveTransport::LocalOwner,
            terminate_capability: false,
        }
    }

    /// Authority for an authenticated remote principal.
    pub fn remote(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            transport: InteractiveTransport::AuthenticatedRemote,
            terminate_capability: false,
        }
    }

    /// Grant the semantic terminate capability (future identity plug;
    /// no wire change required).
    pub fn with_terminate_capability(mut self) -> Self {
        self.terminate_capability = true;
        self
    }

    /// Transport `client_id` this authority speaks for.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Bound transport class (diagnostics only; decisions use the methods).
    pub fn transport(&self) -> InteractiveTransport {
        self.transport
    }

    /// Whether terminate/remove are permitted. Local owners always pass;
    /// remote principals need the plugged semantic capability.
    pub fn can_terminate(&self) -> bool {
        match self.transport {
            InteractiveTransport::LocalOwner => true,
            InteractiveTransport::AuthenticatedRemote => self.terminate_capability,
        }
    }
}

/// Failure modes for the attach/resume family. Unknown and foreign
/// attachments share one wire code; everything maps to a typed
/// `CoreResponse::Error` without leaking handles, owners, or secrets.
#[derive(Debug, Error)]
pub enum InteractiveAttachError {
    #[error("invalid interactive request: {0}")]
    InvalidRequest(&'static str),
    #[error("unknown interactive attachment")]
    UnknownAttachment,
    #[error("interactive attachment is not owned by this connection")]
    NotOwner,
    #[error("interactive attachment limit exceeded ({scope})")]
    AttachmentLimitExceeded { scope: &'static str },
    #[error("unknown interactive handle")]
    UnknownHandle,
    #[error("interactive handle is gone")]
    HandleGone { handle: String },
    #[error("interactive process is no longer running")]
    NotRunning,
    #[error("interactive input exceeds the per-write bound")]
    InputTooLarge,
    #[error("interactive output read exceeds the per-read bound")]
    ReadTooLarge,
    #[error("interactive {operation} is not authorized for this connection")]
    NotAuthorized { operation: &'static str },
    #[error("interactive service is shutting down")]
    ShuttingDown,
    #[error("interactive operation failed: {0}")]
    Upstream(String),
}

impl InteractiveAttachError {
    /// Stable wire code for `CoreResponse::Error`. Unknown and foreign
    /// attachments deliberately share `interactive_attachment_gone`.
    pub fn code(&self) -> &'static str {
        match self {
            InteractiveAttachError::InvalidRequest(_) => "interactive_invalid_request",
            InteractiveAttachError::UnknownAttachment | InteractiveAttachError::NotOwner => {
                "interactive_attachment_gone"
            }
            InteractiveAttachError::AttachmentLimitExceeded { .. } => {
                "interactive_attachment_limit"
            }
            InteractiveAttachError::UnknownHandle | InteractiveAttachError::HandleGone { .. } => {
                "interactive_handle_gone"
            }
            InteractiveAttachError::NotRunning => "interactive_not_running",
            InteractiveAttachError::InputTooLarge => "interactive_input_too_large",
            InteractiveAttachError::ReadTooLarge => "interactive_read_too_large",
            InteractiveAttachError::NotAuthorized { .. } => "interactive_not_authorized",
            InteractiveAttachError::ShuttingDown => "interactive_shutting_down",
            InteractiveAttachError::Upstream(_) => "interactive_upstream",
        }
    }

    fn response(&self) -> CoreResponse {
        CoreResponse::Error {
            code: self.code().to_string(),
            message: self.to_string(),
        }
    }
}

impl From<InteractiveError> for InteractiveAttachError {
    fn from(error: InteractiveError) -> Self {
        match error {
            InteractiveError::UnknownHandle => InteractiveAttachError::UnknownHandle,
            InteractiveError::NotRunning => InteractiveAttachError::NotRunning,
            InteractiveError::InputTooLarge { .. } => InteractiveAttachError::InputTooLarge,
            InteractiveError::ReadTooLarge => InteractiveAttachError::ReadTooLarge,
            InteractiveError::ShuttingDown => InteractiveAttachError::ShuttingDown,
            InteractiveError::EmptyArgv
            | InteractiveError::InvalidArgument(_)
            | InteractiveError::ArgumentTooLarge(_)
            | InteractiveError::InvalidSize { .. }
            | InteractiveError::InvalidScrollback { .. }
            | InteractiveError::TooManyEnvOverrides
            | InteractiveError::EnvEntryTooLarge
            | InteractiveError::AbsoluteCwdRejected
            | InteractiveError::WorkspacePolicy(_) => {
                InteractiveAttachError::InvalidRequest("invalid create parameters")
            }
            InteractiveError::AdmissionBlocked(_)
            | InteractiveError::AdmissionImpossible(_)
            | InteractiveError::PlatformUnsupported { .. }
            | InteractiveError::SpawnFailed(_)
            | InteractiveError::WriteFailed(_)
            | InteractiveError::ResizeFailed(_)
            | InteractiveError::TerminateTimeout
            | InteractiveError::Io(_) => {
                InteractiveAttachError::Upstream(strip_interactive_details(&error))
            }
        }
    }
}

/// Map engine errors to secret-free summaries. Argument values, paths, and
/// environment content never cross into responses or logs.
fn strip_interactive_details(error: &InteractiveError) -> String {
    match error {
        InteractiveError::AdmissionBlocked(_) => {
            "scheduler admission refused the spawn".to_string()
        }
        InteractiveError::AdmissionImpossible(_) => {
            "scheduler admission cannot satisfy the spawn".to_string()
        }
        InteractiveError::PlatformUnsupported { platform } => {
            format!("interactive PTY is not supported on {platform}")
        }
        InteractiveError::SpawnFailed(_) => "interactive process spawn failed".to_string(),
        InteractiveError::WriteFailed(_) => "interactive input write failed".to_string(),
        InteractiveError::ResizeFailed(_) => "interactive resize failed".to_string(),
        InteractiveError::TerminateTimeout => "interactive termination timed out".to_string(),
        InteractiveError::Io(_) => "interactive PTY I/O error".to_string(),
        other => format!("interactive engine error: {other}"),
    }
}

fn validate_client_id(client_id: &str) -> Result<(), InteractiveAttachError> {
    if client_id.is_empty() || client_id.len() > MAX_INTERACTIVE_ID_LENGTH {
        return Err(InteractiveAttachError::InvalidRequest("invalid client id"));
    }
    Ok(())
}

fn snapshot_to_metadata(snapshot: &SessionSnapshot) -> InteractiveProcessMetadata {
    InteractiveProcessMetadata {
        handle: snapshot.handle.as_str().to_string(),
        workspace_id: snapshot.workspace_id.as_str().to_string(),
        command: snapshot.command.clone(),
        state: snapshot.state.as_str().to_string(),
        cols: snapshot.size.cols,
        rows: snapshot.size.rows,
        child_pid: snapshot.child_pid,
        next_seq: snapshot.next_seq,
        retained_bytes: snapshot.retained_bytes,
        total_bytes: snapshot.total_bytes,
        truncated: snapshot.truncated,
        exit_code: snapshot.exit.and_then(|exit| exit.code),
        exit_signal: snapshot.exit.and_then(|exit| exit.signal),
    }
}

/// Daemon-owned attach/resume handler family over the M001 engine.
///
/// Constructed once per execution node with the daemon's scheduler
/// admission controller (shared process-slot accounting). All methods take
/// the trusted transport `client_id` so ownership never comes from the
/// payload. No method spawns background tasks: output reads are on-demand
/// from the shared M001 ring, and lifecycle tasks stay inside M001.
pub struct InteractiveProcessProtocol {
    service: Arc<InteractiveProcessService>,
    attachments: Arc<InteractiveAttachmentRegistry>,
}

impl InteractiveProcessProtocol {
    /// Build the handler family over one scheduler admission controller.
    pub fn new(admission: Arc<crate::scheduler::admission::AdmissionController>) -> Self {
        Self {
            service: Arc::new(InteractiveProcessService::new(admission)),
            attachments: Arc::new(InteractiveAttachmentRegistry::new()),
        }
    }

    /// Underlying M001 engine (shutdown coordination, diagnostics).
    pub fn service(&self) -> &Arc<InteractiveProcessService> {
        &self.service
    }

    /// Attachment registry (disconnect cleanup, diagnostics).
    pub fn attachments(&self) -> &Arc<InteractiveAttachmentRegistry> {
        &self.attachments
    }

    /// Capability negotiation answer. Unknown/disjoint client ranges are
    /// answered with `supported: false`, never with an error.
    pub fn capabilities(&self, client: &InteractiveProcessCapabilities) -> CoreResponse {
        let negotiated = InteractiveProcessCapabilities::negotiate(
            client,
            &InteractiveProcessCapabilities::current(),
        );
        match negotiated {
            Some(version) => CoreResponse::InteractiveProcessCapabilitiesResponse {
                supported: true,
                protocol_version: version,
                max_chunk_bytes: MAX_INTERACTIVE_CHUNK_BYTES,
                max_input_bytes: MAX_INTERACTIVE_INPUT_BYTES,
                max_attachments_per_client: MAX_ATTACHMENTS_PER_CLIENT,
                max_attachments_per_process: MAX_ATTACHMENTS_PER_PROCESS,
                max_list_items: MAX_INTERACTIVE_LIST_ITEMS,
            },
            None => CoreResponse::InteractiveProcessCapabilitiesResponse {
                supported: false,
                protocol_version: INTERACTIVE_PROCESS_PROTOCOL_VERSION,
                max_chunk_bytes: MAX_INTERACTIVE_CHUNK_BYTES,
                max_input_bytes: MAX_INTERACTIVE_INPUT_BYTES,
                max_attachments_per_client: MAX_ATTACHMENTS_PER_CLIENT,
                max_attachments_per_process: MAX_ATTACHMENTS_PER_PROCESS,
                max_list_items: MAX_INTERACTIVE_LIST_ITEMS,
            },
        }
    }

    /// Create one scheduler-admitted process. Returns the handle only; the
    /// caller holds no attachment until [`Self::attach`].
    pub async fn create(
        &self,
        client_id: &str,
        ctx: &Arc<ExecutionContext>,
        dto: &crate::protocol::interactive_process::InteractiveProcessCreateRequest,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        if let Err(error) = dto.validate() {
            return map_protocol_error(&error);
        }
        let spec = match create_to_spec(dto) {
            Ok(spec) => spec,
            Err(error) => return error.response(),
        };
        match self.service.spawn(ctx, spec).await {
            Ok(handle) => match self.service.snapshot(&handle).await {
                Ok(snapshot) => CoreResponse::InteractiveProcessCreated {
                    handle: handle.as_str().to_string(),
                    metadata: snapshot_to_metadata(&snapshot),
                },
                Err(error) => InteractiveAttachError::from(error).response(),
            },
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Bounded metadata list, optionally filtered by workspace.
    pub async fn list(
        &self,
        client_id: &str,
        workspace_id: Option<&str>,
        limit: Option<usize>,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        let limit = limit.unwrap_or(MAX_INTERACTIVE_LIST_ITEMS);
        if limit == 0 || limit > MAX_INTERACTIVE_LIST_ITEMS {
            return InteractiveAttachError::InvalidRequest("invalid list limit").response();
        }
        let mut processes: Vec<InteractiveProcessMetadata> = self
            .service
            .snapshot_all()
            .await
            .iter()
            .filter(|snapshot| {
                workspace_id.map_or(true, |filter| snapshot.workspace_id.as_str() == filter)
            })
            .map(snapshot_to_metadata)
            .collect();
        processes.sort_by(|left, right| left.handle.cmp(&right.handle));
        let truncated = processes.len() > limit;
        processes.truncate(limit);
        CoreResponse::InteractiveProcessList {
            processes,
            truncated,
        }
    }

    /// Attach the calling connection to a handle and read bounded output.
    /// Re-attach is idempotent per (client, handle).
    pub async fn attach(
        &self,
        client_id: &str,
        handle_raw: &str,
        from_seq: Option<u64>,
        max_bytes: Option<usize>,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        if validate_interactive_id(handle_raw).is_err() {
            return InteractiveAttachError::UnknownHandle.response();
        }
        let handle = match InteractiveHandle::parse(handle_raw) {
            Ok(handle) => handle,
            Err(_) => return InteractiveAttachError::UnknownHandle.response(),
        };
        // Existence is checked before the attachment is recorded so failed
        // attaches never mint orphan attachments.
        let snapshot = match self.service.snapshot(&handle).await {
            Ok(snapshot) => snapshot,
            Err(InteractiveError::UnknownHandle) => {
                return InteractiveAttachError::UnknownHandle.response();
            }
            Err(error) => return InteractiveAttachError::from(error).response(),
        };
        let attachment_id = match self.attachments.attach(client_id, handle.as_str()) {
            Ok(id) => id,
            Err(error) => return error.response(),
        };
        let from_seq = from_seq.unwrap_or(0);
        let max_bytes = max_bytes.unwrap_or(DEFAULT_INTERACTIVE_CHUNK_BYTES);
        if max_bytes == 0 || max_bytes > MAX_INTERACTIVE_CHUNK_BYTES {
            return InteractiveAttachError::ReadTooLarge.response();
        }
        match self.service.read_output(&handle, from_seq, max_bytes).await {
            Ok(read) => {
                set_last_cursor(&self.attachments, &attachment_id, read.next_seq);
                let chunk = InteractiveOutputChunk {
                    handle: handle.as_str().to_string(),
                    from_seq,
                    next_seq: read.next_seq,
                    gap: read.gap,
                    data_b64: B64.encode(&read.bytes),
                };
                let resync = if read.gap {
                    Some(InteractiveResync {
                        reason: InteractiveResyncReason::HistoryExpired,
                        handle: handle.as_str().to_string(),
                        base_seq: snapshot
                            .total_bytes
                            .saturating_sub(snapshot.retained_bytes as u64),
                        next_seq: read.next_seq,
                        snapshot: Some(snapshot_to_metadata(&snapshot)),
                    })
                } else if from_seq > read.next_seq {
                    Some(InteractiveResync {
                        reason: InteractiveResyncReason::CursorAhead,
                        handle: handle.as_str().to_string(),
                        base_seq: snapshot
                            .total_bytes
                            .saturating_sub(snapshot.retained_bytes as u64),
                        next_seq: read.next_seq,
                        snapshot: Some(snapshot_to_metadata(&snapshot)),
                    })
                } else {
                    None
                };
                CoreResponse::InteractiveProcessAttached {
                    attachment_id: attachment_id.as_str().to_string(),
                    handle: handle.as_str().to_string(),
                    chunk,
                    resync,
                }
            }
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Release one caller-owned attachment. The process is unaffected.
    pub async fn detach(&self, client_id: &str, attachment_id_raw: &str) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        let attachment_id = match AttachmentId::parse(attachment_id_raw) {
            Ok(id) => id,
            Err(_) => return InteractiveAttachError::UnknownAttachment.response(),
        };
        match self.attachments.detach(client_id, &attachment_id) {
            Ok(()) => CoreResponse::InteractiveProcessDetached {
                attachment_id: attachment_id.as_str().to_string(),
            },
            Err(error) => error.response(),
        }
    }

    /// Write bounded input through a caller-owned attachment. The handle
    /// is resolved server-side; the payload cannot steer another process.
    pub async fn input(
        &self,
        client_id: &str,
        attachment_id_raw: &str,
        data_b64: &str,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        let attachment_id = match AttachmentId::parse(attachment_id_raw) {
            Ok(id) => id,
            Err(_) => return InteractiveAttachError::UnknownAttachment.response(),
        };
        let bytes = match B64.decode(data_b64) {
            Ok(bytes) => bytes,
            Err(_) => {
                return InteractiveAttachError::InvalidRequest("input is not valid base64")
                    .response();
            }
        };
        if bytes.len() > MAX_INTERACTIVE_INPUT_BYTES {
            return InteractiveAttachError::InputTooLarge.response();
        }
        let record = match self.attachments.resolve(client_id, &attachment_id) {
            Ok(record) => record,
            Err(error) => return error.response(),
        };
        let handle_raw = record.handle.clone();
        drop(record);
        let Ok(handle) = InteractiveHandle::parse(&handle_raw) else {
            return InteractiveAttachError::HandleGone { handle: handle_raw }.response();
        };
        match self.service.write_input(&handle, &bytes).await {
            Ok(()) => CoreResponse::InteractiveProcessInputAccepted {
                attachment_id: attachment_id.as_str().to_string(),
                bytes_accepted: bytes.len(),
            },
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Resize through a caller-owned attachment.
    pub async fn resize(
        &self,
        client_id: &str,
        attachment_id_raw: &str,
        cols: u16,
        rows: u16,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        let attachment_id = match AttachmentId::parse(attachment_id_raw) {
            Ok(id) => id,
            Err(_) => return InteractiveAttachError::UnknownAttachment.response(),
        };
        let size = match PtySize::new(cols, rows) {
            Ok(size) => size,
            Err(_) => {
                return InteractiveAttachError::InvalidRequest("invalid terminal size").response();
            }
        };
        let record = match self.attachments.resolve(client_id, &attachment_id) {
            Ok(record) => record,
            Err(error) => return error.response(),
        };
        let handle_raw = record.handle.clone();
        drop(record);
        let Ok(handle) = InteractiveHandle::parse(&handle_raw) else {
            return InteractiveAttachError::HandleGone { handle: handle_raw }.response();
        };
        match self.service.resize(&handle, size).await {
            Ok(()) => CoreResponse::InteractiveProcessResized {
                attachment_id: attachment_id.as_str().to_string(),
                cols,
                rows,
            },
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Resume bounded output through a caller-owned attachment.
    ///
    /// Incremental reads answer `Resumed`; cursors that fell out of the
    /// bounded ring, ran ahead, or name a gone handle answer typed
    /// `ResyncRequired` instead of silently shifting history.
    pub async fn resume(
        &self,
        client_id: &str,
        attachment_id_raw: &str,
        from_seq: u64,
        max_bytes: Option<usize>,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        let attachment_id = match AttachmentId::parse(attachment_id_raw) {
            Ok(id) => id,
            Err(_) => return InteractiveAttachError::UnknownAttachment.response(),
        };
        let max_bytes = max_bytes.unwrap_or(DEFAULT_INTERACTIVE_CHUNK_BYTES);
        if max_bytes == 0 || max_bytes > MAX_INTERACTIVE_CHUNK_BYTES {
            return InteractiveAttachError::ReadTooLarge.response();
        }
        let record = match self.attachments.resolve(client_id, &attachment_id) {
            Ok(record) => record,
            Err(error) => return error.response(),
        };
        let handle_raw = record.handle.clone();
        drop(record);
        let Ok(handle) = InteractiveHandle::parse(&handle_raw) else {
            return CoreResponse::InteractiveProcessResyncRequired {
                attachment_id: attachment_id.as_str().to_string(),
                resync: InteractiveResync {
                    reason: InteractiveResyncReason::HandleGone,
                    handle: handle_raw,
                    base_seq: 0,
                    next_seq: 0,
                    snapshot: None,
                },
            };
        };
        let snapshot = match self.service.snapshot(&handle).await {
            Ok(snapshot) => snapshot,
            Err(InteractiveError::UnknownHandle) => {
                return CoreResponse::InteractiveProcessResyncRequired {
                    attachment_id: attachment_id.as_str().to_string(),
                    resync: InteractiveResync {
                        reason: InteractiveResyncReason::HandleGone,
                        handle: handle.as_str().to_string(),
                        base_seq: 0,
                        next_seq: 0,
                        snapshot: None,
                    },
                };
            }
            Err(error) => return InteractiveAttachError::from(error).response(),
        };
        match self.service.read_output(&handle, from_seq, max_bytes).await {
            Ok(read) => {
                set_last_cursor(&self.attachments, &attachment_id, read.next_seq);
                if read.gap {
                    CoreResponse::InteractiveProcessResyncRequired {
                        attachment_id: attachment_id.as_str().to_string(),
                        resync: InteractiveResync {
                            reason: InteractiveResyncReason::HistoryExpired,
                            handle: handle.as_str().to_string(),
                            base_seq: snapshot
                                .total_bytes
                                .saturating_sub(snapshot.retained_bytes as u64),
                            next_seq: read.next_seq,
                            snapshot: Some(snapshot_to_metadata(&snapshot)),
                        },
                    }
                } else if from_seq > read.next_seq {
                    CoreResponse::InteractiveProcessResyncRequired {
                        attachment_id: attachment_id.as_str().to_string(),
                        resync: InteractiveResync {
                            reason: InteractiveResyncReason::CursorAhead,
                            handle: handle.as_str().to_string(),
                            base_seq: snapshot
                                .total_bytes
                                .saturating_sub(snapshot.retained_bytes as u64),
                            next_seq: read.next_seq,
                            snapshot: Some(snapshot_to_metadata(&snapshot)),
                        },
                    }
                } else {
                    CoreResponse::InteractiveProcessResumed {
                        attachment_id: attachment_id.as_str().to_string(),
                        chunk: InteractiveOutputChunk {
                            handle: handle.as_str().to_string(),
                            from_seq,
                            next_seq: read.next_seq,
                            gap: false,
                            data_b64: B64.encode(&read.bytes),
                        },
                    }
                }
            }
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Terminate the attachment's process (bounded SIGTERM-then-SIGKILL
    /// escalation). Requires attachment ownership plus terminate authority.
    /// The attachment survives so remaining scrollback stays resumable
    /// until [`Self::remove`].
    pub async fn terminate(
        &self,
        client_id: &str,
        authority: &InteractiveAuthority,
        attachment_id_raw: &str,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        if !authority.can_terminate() {
            return InteractiveAttachError::NotAuthorized {
                operation: "terminate",
            }
            .response();
        }
        let attachment_id = match AttachmentId::parse(attachment_id_raw) {
            Ok(id) => id,
            Err(_) => return InteractiveAttachError::UnknownAttachment.response(),
        };
        let record = match self.attachments.resolve(client_id, &attachment_id) {
            Ok(record) => record,
            Err(error) => return error.response(),
        };
        let handle_raw = record.handle.clone();
        drop(record);
        let Ok(handle) = InteractiveHandle::parse(&handle_raw) else {
            return InteractiveAttachError::HandleGone { handle: handle_raw }.response();
        };
        match self.service.terminate(&handle).await {
            Ok(exit) => CoreResponse::InteractiveProcessTerminated {
                attachment_id: attachment_id.as_str().to_string(),
                handle: handle.as_str().to_string(),
                exit_code: exit.and_then(|info| info.code),
                exit_signal: exit.and_then(|info| info.signal),
            },
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Drop the attachment's handle, freeing scrollback. Live processes
    /// are terminated first (bounded escalation inside M001). Requires
    /// attachment ownership plus terminate authority.
    pub async fn remove(
        &self,
        client_id: &str,
        authority: &InteractiveAuthority,
        attachment_id_raw: &str,
    ) -> CoreResponse {
        if let Err(error) = validate_client_id(client_id) {
            return error.response();
        }
        if !authority.can_terminate() {
            return InteractiveAttachError::NotAuthorized {
                operation: "remove",
            }
            .response();
        }
        let attachment_id = match AttachmentId::parse(attachment_id_raw) {
            Ok(id) => id,
            Err(_) => return InteractiveAttachError::UnknownAttachment.response(),
        };
        let record = match self.attachments.resolve(client_id, &attachment_id) {
            Ok(record) => record,
            Err(error) => return error.response(),
        };
        let handle_raw = record.handle.clone();
        drop(record);
        let Ok(handle) = InteractiveHandle::parse(&handle_raw) else {
            self.attachments.attachments.remove(attachment_id.as_str());
            return CoreResponse::InteractiveProcessRemoved {
                attachment_id: attachment_id.as_str().to_string(),
                handle: handle_raw,
            };
        };
        match self.service.remove(&handle).await {
            Ok(()) => {
                self.attachments.attachments.remove(attachment_id.as_str());
                CoreResponse::InteractiveProcessRemoved {
                    attachment_id: attachment_id.as_str().to_string(),
                    handle: handle.as_str().to_string(),
                }
            }
            Err(InteractiveError::UnknownHandle) => {
                self.attachments.attachments.remove(attachment_id.as_str());
                CoreResponse::InteractiveProcessRemoved {
                    attachment_id: attachment_id.as_str().to_string(),
                    handle: handle.as_str().to_string(),
                }
            }
            Err(error) => InteractiveAttachError::from(error).response(),
        }
    }

    /// Connection-close/EOF cleanup: drop every attachment owned by the
    /// connection. Processes are never touched here; an explicit terminate
    /// (or shutdown) is the only path that kills them.
    pub fn handle_disconnect(&self, client_id: &str) -> usize {
        self.attachments.handle_disconnect(client_id)
    }
}

fn set_last_cursor(
    registry: &InteractiveAttachmentRegistry,
    attachment_id: &AttachmentId,
    next_seq: u64,
) {
    if let Some(record) = registry.attachments.get(attachment_id.as_str()) {
        record.last_from_seq.store(next_seq, Ordering::Relaxed);
    }
}

fn map_protocol_error(
    error: &crate::protocol::interactive_process::InteractiveProtocolError,
) -> CoreResponse {
    use crate::protocol::interactive_process::InteractiveProtocolError as E;
    let code = match error {
        E::InvalidWorkspaceId | E::InvalidArgv | E::InvalidCwd | E::InvalidEnvEntry => {
            "interactive_invalid_request"
        }
        E::TooManyEnvOverrides => "interactive_invalid_request",
        E::InvalidHandle => "interactive_handle_gone",
        E::InvalidAttachmentId => "interactive_attachment_gone",
        E::InvalidCursor => "interactive_invalid_request",
        E::ReadTooLarge => "interactive_read_too_large",
        E::InputTooLarge => "interactive_input_too_large",
        E::InvalidSize => "interactive_invalid_request",
    };
    CoreResponse::Error {
        code: code.to_string(),
        message: error.to_string(),
    }
}

fn create_to_spec(
    dto: &InteractiveProcessCreateRequest,
) -> Result<SpawnSpec, InteractiveAttachError> {
    use std::ffi::OsString;
    use std::path::PathBuf;

    let argv: Vec<OsString> = dto.argv.iter().map(OsString::from).collect();
    let mut spec = SpawnSpec::new(argv);
    if let Some(cwd) = dto.cwd.as_ref() {
        spec.cwd = Some(PathBuf::from(cwd));
    }
    for entry in &dto.env_overrides {
        spec.env_overrides
            .push((OsString::from(&entry.name), OsString::from(&entry.value)));
    }
    let cols = dto.cols.unwrap_or(80);
    let rows = dto.rows.unwrap_or(24);
    spec.size = PtySize::new(cols, rows)
        .map_err(|_| InteractiveAttachError::InvalidRequest("invalid terminal size"))?;
    spec.scrollback_bytes = dto.scrollback_bytes;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority(client: &str) -> InteractiveAuthority {
        InteractiveAuthority::local(client)
    }

    #[test]
    fn registry_attach_is_idempotent_per_client_and_handle() {
        let registry = InteractiveAttachmentRegistry::new();
        let first = registry.attach("client-a", "handle-1").expect("attach");
        let second = registry.attach("client-a", "handle-1").expect("reattach");
        assert_eq!(first, second);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registry_isolates_clients_and_handles() {
        let registry = InteractiveAttachmentRegistry::new();
        let a = registry.attach("client-a", "handle-1").expect("attach a");
        let b = registry.attach("client-b", "handle-1").expect("attach b");
        assert_ne!(a, b);
        assert_eq!(registry.process_count("handle-1"), 2);
        assert_eq!(registry.client_count("client-a"), 1);
    }

    #[test]
    fn resolve_rejects_foreign_attachments() {
        let registry = InteractiveAttachmentRegistry::new();
        let id = registry.attach("client-a", "handle-1").expect("attach");
        assert!(registry.resolve("client-a", &id).is_ok());
        assert!(matches!(
            registry.resolve("client-b", &id),
            Err(InteractiveAttachError::NotOwner)
        ));
        // Unknown and foreign IDs share one wire code.
        assert_eq!(
            InteractiveAttachError::NotOwner.code(),
            InteractiveAttachError::UnknownAttachment.code()
        );
    }

    #[test]
    fn detach_releases_only_the_caller_attachment() {
        let registry = InteractiveAttachmentRegistry::new();
        let a = registry.attach("client-a", "handle-1").expect("attach a");
        let b = registry.attach("client-b", "handle-1").expect("attach b");
        registry.detach("client-a", &a).expect("detach");
        assert!(registry.resolve("client-a", &a).is_err());
        assert!(registry.resolve("client-b", &b).is_ok());
    }

    #[test]
    fn disconnect_releases_attachments_without_touching_others() {
        let registry = InteractiveAttachmentRegistry::new();
        registry.attach("client-a", "handle-1").expect("attach");
        registry.attach("client-a", "handle-2").expect("attach");
        registry.attach("client-b", "handle-1").expect("attach");
        assert_eq!(registry.handle_disconnect("client-a"), 2);
        assert_eq!(registry.client_count("client-a"), 0);
        assert_eq!(registry.client_count("client-b"), 1);
    }

    #[test]
    fn attachment_bounds_are_enforced() {
        let registry = InteractiveAttachmentRegistry::new();
        for index in 0..MAX_ATTACHMENTS_PER_CLIENT {
            registry
                .attach("client-a", &format!("handle-{index}"))
                .expect("attach");
        }
        assert!(matches!(
            registry.attach("client-a", "handle-overflow"),
            Err(InteractiveAttachError::AttachmentLimitExceeded { scope: "client" })
        ));
    }

    #[test]
    fn authority_grants_terminate_to_local_owner_only() {
        let local = InteractiveAuthority::local("client-a");
        assert!(local.can_terminate());
        let remote = InteractiveAuthority::remote("client-b");
        assert!(!remote.can_terminate());
        let elevated = InteractiveAuthority::remote("client-b").with_terminate_capability();
        assert!(elevated.can_terminate());
        let _ = test_authority("client-a");
    }

    #[test]
    fn error_codes_do_not_leak_owners_or_handles() {
        let gone = InteractiveAttachError::HandleGone {
            handle: "handle-1".to_string(),
        };
        assert_eq!(gone.code(), "interactive_handle_gone");
        assert!(!gone.to_string().contains("handle-1"));
        let foreign = InteractiveAttachError::NotOwner;
        assert!(!foreign.to_string().contains("client"));
    }

    #[test]
    fn cli_wire_ids_reject_path_traversal() {
        assert!(AttachmentId::parse("../escape").is_err());
        assert!(AttachmentId::parse("").is_err());
    }
}
