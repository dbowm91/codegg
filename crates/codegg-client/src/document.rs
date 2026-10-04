//! Frontend-neutral optimistic replica/controller for `document.v1`.
//!
//! The controller owns canonical text replication and a bounded, serial
//! transaction queue. Cursor, selection, viewport, and rendering stay in the UI.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use codegg_document::{DocumentBuffer, DocumentLimits, DocumentSnapshot, TextTransaction};
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::document::{
    DocumentSnapshotDto, DocumentTextEditDto, DocumentTextRangeDto, DocumentTransactionDto,
    MAX_DOCUMENT_EDITS, MAX_DOCUMENT_INSERT_BYTES, MAX_DOCUMENT_TEXT_BYTES,
};
use std::sync::Mutex;
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use uuid::Uuid;

const MAX_PENDING_TRANSACTIONS: usize = 128;
const MAX_PENDING_INSERT_BYTES: usize = 4 * 1024 * 1024;
static CONTROLLER_GENERATION: AtomicU64 = AtomicU64::new(1);

// `async_trait` marks the generated boxed future as must-use; the future type
// already carries that marker, so the generated attribute trips Clippy's
// double-must-use lint.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait DocumentTransport: Send + Sync {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String>;
}

#[async_trait]
impl DocumentTransport for crate::LocalSocketClient {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        self.request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: Uuid::new_v4().to_string(),
            payload: request,
        })
        .await
        .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DocumentState {
    #[default]
    Closed,
    Opening,
    Synced,
    DirtyLocal,
    Flushing,
    ResyncRequired,
    Conflict,
    ReadOnly,
    Disconnected,
    GoneWithLocalDraft,
}

#[derive(Debug, Error)]
pub enum DocumentControllerError {
    #[error("document transport failed: {0}")]
    Transport(String),
    #[error("unexpected document response: {0}")]
    Response(String),
    #[error("document transaction rejected: {0}")]
    Edit(#[from] codegg_document::DocumentError),
    #[error("document controller is not attached")]
    NotOpen,
    #[error("document controller already has an attachment")]
    AlreadyOpen,
    #[error("document lifecycle operation is in progress")]
    LifecycleBusy,
    #[error("document has no writer lease")]
    ReadOnly,
    #[error("pending document queue is full")]
    QueueFull,
    #[error("pending document bytes exceed the configured bound")]
    QueueBytes,
    #[error("document state requires an authoritative resync")]
    ResyncRequired,
    #[error("document daemon reports a disk conflict")]
    Conflict,
}

struct PendingChange {
    id: String,
    base_revision: u64,
    transaction: TextTransaction,
    dto: DocumentTransactionDto,
    base_snapshot: DocumentSnapshot,
    resulting_snapshot: DocumentSnapshot,
}

struct DynamicTransport(RwLock<Arc<dyn DocumentTransport>>);

impl DynamicTransport {
    fn new(transport: Arc<dyn DocumentTransport>) -> Self {
        Self(RwLock::new(transport))
    }
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        let transport = self.0.read().await.clone();
        transport.request(request).await
    }
    async fn replace(&self, transport: Arc<dyn DocumentTransport>) {
        *self.0.write().await = transport;
    }
}
struct Attachment {
    project_id: String,
    workspace_id: String,
    relative_path: String,
    document_id: String,
    writer_lease: Option<String>,
    daemon_revision: u64,
    buffer: DocumentBuffer,
    generation: u64,
    pending: VecDeque<PendingChange>,
    pending_bytes: usize,
    in_flight: Option<String>,
    dirty: bool,
    conflicted: bool,
}

/// One document replica. Mutating operations are serialized; `apply_local` is
/// synchronous and immediately updates the rope-backed optimistic snapshot.
pub struct DocumentController {
    transport: DynamicTransport,
    replica: Mutex<ReplicaState>,
    operation: AsyncMutex<()>,
    generation: u64,
    flush_worker_running: AtomicBool,
    network_enabled: AtomicBool,
}

struct LifecycleGuard<'a> {
    controller: &'a DocumentController,
    epoch: u64,
    restore: DocumentState,
    finished: bool,
}

impl LifecycleGuard<'_> {
    fn finish(mut self, state: DocumentState) {
        let mut replica = self
            .controller
            .replica
            .lock()
            .expect("replica mutex poisoned");
        if replica.lifecycle_epoch == self.epoch {
            replica.lifecycle = ControllerLifecycle::Idle;
            replica.state = state;
        }
        self.finished = true;
    }
}

impl Drop for LifecycleGuard<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut replica = self
            .controller
            .replica
            .lock()
            .expect("replica mutex poisoned");
        if replica.lifecycle_epoch == self.epoch {
            replica.lifecycle = ControllerLifecycle::Idle;
            replica.state = replica.attachment.as_ref().map_or_else(
                || self.restore.clone(),
                |attachment| {
                    if attachment.conflicted {
                        DocumentState::Conflict
                    } else if attachment.dirty || !attachment.pending.is_empty() {
                        DocumentState::DirtyLocal
                    } else {
                        self.restore.clone()
                    }
                },
            );
        }
    }
}

#[derive(Default)]
struct ReplicaState {
    attachment: Option<Attachment>,
    state: DocumentState,
    lifecycle: ControllerLifecycle,
    local_edit_seq: u64,
    lifecycle_epoch: u64,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum ControllerLifecycle {
    #[default]
    Idle,
    Saving,
    Reloading,
    Closing,
    Resyncing,
    Reconnecting,
    Opening,
}

impl DocumentController {
    pub fn new(transport: Arc<dyn DocumentTransport>) -> Self {
        Self {
            transport: DynamicTransport::new(transport),
            replica: Mutex::new(ReplicaState {
                state: DocumentState::Closed,
                ..ReplicaState::default()
            }),
            operation: AsyncMutex::new(()),
            generation: CONTROLLER_GENERATION.fetch_add(1, Ordering::Relaxed),
            flush_worker_running: AtomicBool::new(false),
            network_enabled: AtomicBool::new(true),
        }
    }
    pub async fn state(&self) -> DocumentState {
        self.replica
            .lock()
            .expect("replica mutex poisoned")
            .state
            .clone()
    }
    pub async fn attachment_scope(&self) -> Result<(String, String), DocumentControllerError> {
        let guard = self.replica.lock().expect("replica mutex poisoned");
        let attachment = guard
            .attachment
            .as_ref()
            .ok_or(DocumentControllerError::NotOpen)?;
        Ok((
            attachment.workspace_id.clone(),
            attachment.relative_path.clone(),
        ))
    }
    pub async fn snapshot(&self) -> Result<(DocumentSnapshot, u64), DocumentControllerError> {
        let a = self.replica.lock().expect("replica mutex poisoned");
        let a = a
            .attachment
            .as_ref()
            .ok_or(DocumentControllerError::NotOpen)?;
        Ok((a.buffer.snapshot(), a.daemon_revision))
    }
    pub async fn open(
        &self,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        writable: bool,
    ) -> Result<(), DocumentControllerError> {
        let _op = self.operation.lock().await;
        let open_epoch = {
            let mut replica = self.replica.lock().expect("replica mutex poisoned");
            if replica.attachment.is_some() {
                return Err(DocumentControllerError::AlreadyOpen);
            }
            replica.lifecycle = ControllerLifecycle::Opening;
            replica.lifecycle_epoch = replica.lifecycle_epoch.wrapping_add(1);
            replica.state = DocumentState::Opening;
            replica.lifecycle_epoch
        };
        let lifecycle = LifecycleGuard {
            controller: self,
            epoch: open_epoch,
            restore: DocumentState::Closed,
            finished: false,
        };
        let response = self
            .transport
            .request(CoreRequest::DocumentOpen {
                project_id: project_id.clone(),
                workspace_id: workspace_id.clone(),
                relative_path: relative_path.clone(),
            })
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.fail_open();
                return Err(DocumentControllerError::Transport(error));
            }
        };
        let (snapshot, lease) = match response {
            CoreResponse::DocumentSnapshot {
                snapshot,
                writer_lease,
                ..
            } => (snapshot, writer_lease),
            r => {
                self.fail_open();
                return self.response_error(r);
            }
        };
        let lease = if writable {
            match lease {
                Some(l) => Some(l),
                None => match self
                    .transport
                    .request(CoreRequest::DocumentWriterAcquire {
                        project_id: project_id.clone(),
                        document_id: snapshot.document_id.clone(),
                    })
                    .await
                    .map_err(|error| {
                        self.fail_open();
                        DocumentControllerError::Transport(error)
                    })? {
                    CoreResponse::DocumentWriterLease { writer_lease, .. } => Some(writer_lease),
                    r => {
                        self.fail_open();
                        return self.response_error(r);
                    }
                },
            }
        } else {
            None
        };
        let state = if snapshot.conflicted {
            DocumentState::Conflict
        } else if lease.is_none() {
            DocumentState::ReadOnly
        } else {
            DocumentState::Synced
        };
        if snapshot.text.len() > MAX_DOCUMENT_TEXT_BYTES {
            self.fail_open();
            return Err(DocumentControllerError::Response(
                "document snapshot exceeds the protocol size limit".into(),
            ));
        }
        let buffer = DocumentBuffer::new(&snapshot.text);
        {
            let mut replica = self.replica.lock().expect("replica mutex poisoned");
            replica.attachment = Some(Attachment {
                project_id,
                workspace_id,
                relative_path,
                document_id: snapshot.document_id,
                writer_lease: lease,
                daemon_revision: snapshot.revision,
                buffer,
                generation: self.generation,
                pending: VecDeque::new(),
                pending_bytes: 0,
                in_flight: None,
                dirty: snapshot.dirty,
                conflicted: snapshot.conflicted,
            });
            replica.state = state.clone();
            replica.lifecycle = ControllerLifecycle::Idle;
            replica.local_edit_seq = 0;
        }
        lifecycle.finish(state);
        Ok(())
    }
    fn fail_open(&self) {
        let mut replica = self.replica.lock().expect("replica mutex poisoned");
        if replica.attachment.is_none() {
            replica.state = DocumentState::Closed;
            replica.lifecycle = ControllerLifecycle::Idle;
        }
    }
    pub fn apply_local(
        self: &Arc<Self>,
        transaction: TextTransaction,
    ) -> Result<String, DocumentControllerError> {
        if transaction.edits.is_empty() {
            return Ok(Uuid::new_v4().to_string());
        }
        let mut slot = self.replica.lock().expect("replica mutex poisoned");
        if slot.lifecycle != ControllerLifecycle::Idle
            && slot.lifecycle != ControllerLifecycle::Saving
        {
            return Err(DocumentControllerError::LifecycleBusy);
        }
        let lifecycle = slot.lifecycle;
        let ReplicaState {
            attachment,
            state,
            local_edit_seq,
            ..
        } = &mut *slot;
        let a = attachment
            .as_mut()
            .ok_or(DocumentControllerError::NotOpen)?;
        if a.writer_lease.is_none() {
            return Err(DocumentControllerError::ReadOnly);
        }
        if a.pending.len() >= MAX_PENDING_TRANSACTIONS {
            return Err(DocumentControllerError::QueueFull);
        }
        if transaction.edits.len() > MAX_DOCUMENT_EDITS {
            return Err(DocumentControllerError::Edit(
                codegg_document::DocumentError::TooManyEdits,
            ));
        }
        let insert_bytes: usize = transaction.edits.iter().map(|e| e.insert.len()).sum();
        if insert_bytes > MAX_DOCUMENT_INSERT_BYTES {
            return Err(DocumentControllerError::Edit(
                codegg_document::DocumentError::TooMuchInsertedText,
            ));
        }
        if a.pending_bytes.saturating_add(insert_bytes) > MAX_PENDING_INSERT_BYTES {
            return Err(DocumentControllerError::QueueBytes);
        }
        let base_snapshot = a.buffer.snapshot();
        let applied = a.buffer.apply(
            &transaction,
            DocumentLimits {
                max_document_bytes: MAX_DOCUMENT_TEXT_BYTES,
                max_edits: MAX_DOCUMENT_EDITS,
                max_inserted_bytes: MAX_DOCUMENT_INSERT_BYTES,
            },
        )?;
        let resulting_snapshot = applied.snapshot;
        *local_edit_seq = local_edit_seq.wrapping_add(1);
        if lifecycle == ControllerLifecycle::Idle {
            *state = DocumentState::DirtyLocal;
        }
        let can_merge = a.pending.back().is_some_and(|last| {
            a.in_flight.as_deref() != Some(last.id.as_str())
                && last.transaction.edits.len() == 1
                && transaction.edits.len() == 1
                && last.transaction.edits[0].range.start == last.transaction.edits[0].range.end
                && transaction.edits[0].range.start == transaction.edits[0].range.end
                && transaction.edits[0].range.start
                    == last.transaction.edits[0].range.start
                        + last.transaction.edits[0].insert.len()
        });
        if can_merge {
            let last = a.pending.back_mut().expect("merge candidate exists");
            last.transaction.edits[0]
                .insert
                .push_str(&transaction.edits[0].insert);
            last.dto = transaction_to_dto(&last.transaction);
            last.resulting_snapshot = resulting_snapshot;
            a.pending_bytes += insert_bytes;
            a.dirty = true;
            let id = last.id.clone();
            drop(slot);
            self.schedule_flush();
            return Ok(id);
        }
        let base = a.daemon_revision + a.pending.len() as u64;
        let dto = transaction_to_dto(&transaction);
        let id = Uuid::new_v4().to_string();
        a.pending_bytes += insert_bytes;
        a.pending.push_back(PendingChange {
            id: id.clone(),
            base_revision: base,
            transaction,
            dto,
            base_snapshot,
            resulting_snapshot,
        });
        a.dirty = true;
        drop(slot);
        self.schedule_flush();
        Ok(id)
    }
    fn schedule_flush(self: &Arc<Self>) {
        if !self.network_enabled.load(Ordering::Acquire) {
            return;
        }
        if self.flush_worker_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.flush_worker_running.store(false, Ordering::Release);
            return;
        };
        let controller = Arc::clone(self);
        runtime.spawn(async move {
            controller.run_flush_worker().await;
        });
    }
    async fn run_flush_worker(self: Arc<Self>) {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = self.flush().await;
            let replica = self.replica.lock().expect("replica mutex poisoned");
            let should_continue = self.network_enabled.load(Ordering::Acquire)
                && replica.lifecycle == ControllerLifecycle::Idle
                && replica.attachment.as_ref().is_some_and(|a| {
                    !a.pending.is_empty() && a.writer_lease.is_some() && !a.conflicted
                });
            if should_continue {
                continue;
            }
            // Relinquish ownership while holding the same lock apply_local uses.
            self.flush_worker_running.store(false, Ordering::Release);
            let should_restart = self.network_enabled.load(Ordering::Acquire)
                && replica.lifecycle == ControllerLifecycle::Idle
                && replica.attachment.as_ref().is_some_and(|a| {
                    !a.pending.is_empty() && a.writer_lease.is_some() && !a.conflicted
                });
            drop(replica);
            if should_restart {
                self.schedule_flush();
            }
            return;
        }
    }
    pub async fn flush(&self) -> Result<(), DocumentControllerError> {
        let _op = self.operation.lock().await;
        loop {
            let req = {
                let mut guard = self.replica.lock().expect("replica mutex poisoned");
                let ReplicaState {
                    attachment, state, ..
                } = &mut *guard;
                let a = attachment
                    .as_mut()
                    .ok_or(DocumentControllerError::NotOpen)?;
                if a.conflicted {
                    *state = DocumentState::Conflict;
                    return Err(DocumentControllerError::Conflict);
                }
                let Some(p) = a.pending.front() else {
                    *state = if a.dirty {
                        DocumentState::DirtyLocal
                    } else {
                        DocumentState::Synced
                    };
                    return Ok(());
                };
                let lease = a
                    .writer_lease
                    .clone()
                    .ok_or(DocumentControllerError::ReadOnly)?;
                a.in_flight = Some(p.id.clone());
                *state = DocumentState::Flushing;
                (
                    a.project_id.clone(),
                    a.document_id.clone(),
                    lease,
                    p.base_revision,
                    p.id.clone(),
                    p.dto.clone(),
                    a.generation,
                )
            };
            let response = self
                .transport
                .request(CoreRequest::DocumentChange {
                    project_id: req.0.clone(),
                    document_id: req.1.clone(),
                    writer_lease: req.2,
                    base_revision: req.3,
                    change_id: req.4.clone(),
                    transaction: req.5,
                })
                .await;
            let mut guard = self.replica.lock().expect("replica mutex poisoned");
            let a = guard
                .attachment
                .as_mut()
                .ok_or(DocumentControllerError::NotOpen)?;
            if a.generation != req.6 || self.generation != req.6 || a.document_id != req.1 {
                return Ok(());
            }
            match response {
                Err(e) => {
                    a.in_flight = None;
                    self.network_enabled.store(false, Ordering::Release);
                    guard.state = DocumentState::Disconnected;
                    return Err(DocumentControllerError::Transport(e));
                }
                Ok(CoreResponse::DocumentChanged { revision, .. }) => {
                    a.in_flight = None;
                    if let Some(p) = a.pending.pop_front().filter(|p| p.id == req.4) {
                        a.pending_bytes = a.pending_bytes.saturating_sub(
                            p.transaction
                                .edits
                                .iter()
                                .map(|e| e.insert.len())
                                .sum::<usize>(),
                        );
                    }
                    a.daemon_revision = revision;
                }
                Ok(r) => {
                    a.in_flight = None;
                    self.network_enabled.store(false, Ordering::Release);
                    guard.state = DocumentState::ResyncRequired;
                    return self.response_error(r);
                }
            }
        }
    }
    pub async fn save(self: &Arc<Self>) -> Result<(), DocumentControllerError> {
        self.flush().await?;
        let _op = self.operation.lock().await;
        let (project, id, lease, revision, saved_local_seq, epoch, prior_state) = {
            let mut replica = self.replica.lock().expect("replica mutex poisoned");
            let (project, id, lease, revision) = {
                let a = replica
                    .attachment
                    .as_ref()
                    .ok_or(DocumentControllerError::NotOpen)?;
                if a.conflicted {
                    return Err(DocumentControllerError::Conflict);
                }
                (
                    a.project_id.clone(),
                    a.document_id.clone(),
                    a.writer_lease
                        .clone()
                        .ok_or(DocumentControllerError::ReadOnly)?,
                    a.daemon_revision,
                )
            };
            let prior_state = replica.state.clone();
            replica.lifecycle = ControllerLifecycle::Saving;
            replica.lifecycle_epoch = replica.lifecycle_epoch.wrapping_add(1);
            replica.state = DocumentState::Flushing;
            (
                project,
                id,
                lease,
                revision,
                replica.local_edit_seq,
                replica.lifecycle_epoch,
                prior_state,
            )
        };
        let lifecycle = LifecycleGuard {
            controller: self,
            epoch,
            restore: prior_state,
            finished: false,
        };
        let response = self
            .transport
            .request(CoreRequest::DocumentSave {
                project_id: project,
                document_id: id,
                writer_lease: lease,
                expected_revision: revision,
            })
            .await;
        match response {
            Err(error) => {
                let replica = self.replica.lock().expect("replica mutex poisoned");
                if replica.lifecycle_epoch == epoch {
                    self.network_enabled.store(false, Ordering::Release);
                }
                drop(replica);
                lifecycle.finish(DocumentState::Disconnected);
                Err(DocumentControllerError::Transport(error))
            }
            Ok(CoreResponse::DocumentSaved { revision, .. }) => {
                let mut replica = self.replica.lock().expect("replica mutex poisoned");
                if replica.lifecycle_epoch != epoch {
                    return Err(DocumentControllerError::LifecycleBusy);
                }
                if let Some(a) = replica.attachment.as_mut() {
                    a.daemon_revision = revision;
                }
                if replica.local_edit_seq == saved_local_seq
                    && replica
                        .attachment
                        .as_ref()
                        .is_some_and(|a| a.pending.is_empty())
                {
                    if let Some(a) = replica.attachment.as_mut() {
                        a.dirty = false;
                    }
                    replica.state = DocumentState::Synced;
                } else {
                    if let Some(a) = replica.attachment.as_mut() {
                        a.dirty = true;
                    }
                    replica.state = DocumentState::DirtyLocal;
                }
                let needs_flush = replica.state == DocumentState::DirtyLocal;
                let next_state = replica.state.clone();
                drop(replica);
                lifecycle.finish(next_state);
                if needs_flush {
                    self.schedule_flush();
                }
                Ok(())
            }
            Ok(r) => {
                let mut replica = self.replica.lock().expect("replica mutex poisoned");
                if matches!(&r, CoreResponse::Error { code, .. } if code == "document_disk_conflict")
                {
                    if let Some(a) = replica.attachment.as_mut() {
                        a.conflicted = true;
                    }
                    replica.state = DocumentState::Conflict;
                } else {
                    replica.state = DocumentState::ResyncRequired;
                    self.network_enabled.store(false, Ordering::Release);
                }
                let next_state = replica.state.clone();
                drop(replica);
                lifecycle.finish(next_state);
                self.response_error(r)
            }
        }
    }
    pub async fn reload_from_disk(&self) -> Result<(), DocumentControllerError> {
        let _op = self.operation.lock().await;
        let (project, id, lease, revision, epoch, prior_state) = {
            let mut replica = self.replica.lock().expect("replica mutex poisoned");
            let (project, id, lease, revision) = {
                let a = replica
                    .attachment
                    .as_ref()
                    .ok_or(DocumentControllerError::NotOpen)?;
                if !a.pending.is_empty() {
                    return Err(DocumentControllerError::ResyncRequired);
                }
                (
                    a.project_id.clone(),
                    a.document_id.clone(),
                    a.writer_lease
                        .clone()
                        .ok_or(DocumentControllerError::ReadOnly)?,
                    a.daemon_revision,
                )
            };
            let prior_state = replica.state.clone();
            replica.lifecycle = ControllerLifecycle::Reloading;
            replica.lifecycle_epoch = replica.lifecycle_epoch.wrapping_add(1);
            let epoch = replica.lifecycle_epoch;
            replica.state = DocumentState::Opening;
            (project, id, lease, revision, epoch, prior_state)
        };
        let lifecycle = LifecycleGuard {
            controller: self,
            epoch,
            restore: prior_state.clone(),
            finished: false,
        };
        let result = self
            .transport
            .request(CoreRequest::DocumentReload {
                project_id: project.clone(),
                document_id: id.clone(),
                writer_lease: lease,
                expected_revision: revision,
            })
            .await;
        let result = match result {
            Err(error) => Err(DocumentControllerError::Transport(error)),
            Ok(CoreResponse::DocumentReloaded { .. }) => {
                let r = self
                    .transport
                    .request(CoreRequest::DocumentSnapshotGet {
                        project_id: project.clone(),
                        document_id: id.clone(),
                    })
                    .await;
                match r {
                    Err(error) => Err(DocumentControllerError::Transport(error)),
                    Ok(CoreResponse::DocumentSnapshot { snapshot, .. }) => {
                        self.install_snapshot(snapshot)?;
                        Ok(())
                    }
                    Ok(response) => self.response_error(response),
                }
            }
            Ok(response) => self.response_error(response),
        };
        lifecycle.finish(if result.is_ok() {
            DocumentState::Synced
        } else {
            prior_state
        });
        result
    }
    pub async fn resync(self: &Arc<Self>) -> Result<(), DocumentControllerError> {
        let _op = self.operation.lock().await;
        let (project, id, epoch, prior_state) = {
            let mut replica = self.replica.lock().expect("replica mutex poisoned");
            let a = replica
                .attachment
                .as_ref()
                .ok_or(DocumentControllerError::NotOpen)?;
            let pair = (a.project_id.clone(), a.document_id.clone());
            let prior_state = replica.state.clone();
            replica.lifecycle = ControllerLifecycle::Resyncing;
            replica.lifecycle_epoch = replica.lifecycle_epoch.wrapping_add(1);
            replica.state = DocumentState::Opening;
            (pair.0, pair.1, replica.lifecycle_epoch, prior_state)
        };
        let lifecycle = LifecycleGuard {
            controller: self,
            epoch,
            restore: prior_state.clone(),
            finished: false,
        };
        let r = self
            .transport
            .request(CoreRequest::DocumentSnapshotGet {
                project_id: project,
                document_id: id,
            })
            .await;
        let result = match r {
            Err(error) => Err(DocumentControllerError::Transport(error)),
            Ok(CoreResponse::DocumentSnapshot { snapshot, .. }) => {
                self.install_snapshot(snapshot)?;
                let replica = self.replica.lock().expect("replica mutex poisoned");
                let next = replica
                    .attachment
                    .as_ref()
                    .map(|a| {
                        if a.conflicted {
                            DocumentState::Conflict
                        } else if a.writer_lease.is_none() {
                            DocumentState::ReadOnly
                        } else if a.dirty {
                            DocumentState::DirtyLocal
                        } else {
                            DocumentState::Synced
                        }
                    })
                    .unwrap_or(DocumentState::GoneWithLocalDraft);
                Ok(next)
            }
            Ok(CoreResponse::Error { code, message }) if code == "document_not_found" => {
                Err(DocumentControllerError::Response(message))
            }
            Ok(response) => self.response_error(response),
        };
        let next_state = match &result {
            Ok(next) => next.clone(),
            Err(DocumentControllerError::Response(_)) => DocumentState::GoneWithLocalDraft,
            Err(_) => prior_state,
        };
        lifecycle.finish(next_state);
        let recovered = result.is_ok();
        if recovered {
            self.network_enabled.store(true, Ordering::Release);
            self.schedule_flush();
        }
        result.map(|_| ())
    }
    /// Replace a closed transport and reattach the same scoped document. An
    /// uncertain change is resolved by snapshot comparison or retried with
    /// its original stable change ID; divergent drafts are retained.
    pub async fn reconnect(
        self: &Arc<Self>,
        transport: Arc<dyn DocumentTransport>,
    ) -> Result<(), DocumentControllerError> {
        let _op = self.operation.lock().await;
        self.transport.replace(transport).await;
        let (
            project_id,
            workspace_id,
            relative_path,
            document_id,
            wants_writer,
            epoch,
            prior_state,
        ) = {
            let mut guard = self.replica.lock().expect("replica mutex poisoned");
            let prior_state = guard.state.clone();
            let (project_id, workspace_id, relative_path, document_id, wants_writer) = {
                let a = guard
                    .attachment
                    .as_ref()
                    .ok_or(DocumentControllerError::NotOpen)?;
                (
                    a.project_id.clone(),
                    a.workspace_id.clone(),
                    a.relative_path.clone(),
                    a.document_id.clone(),
                    a.writer_lease.is_some(),
                )
            };
            guard.lifecycle = ControllerLifecycle::Reconnecting;
            guard.lifecycle_epoch = guard.lifecycle_epoch.wrapping_add(1);
            guard.state = DocumentState::Opening;
            (
                project_id,
                workspace_id,
                relative_path,
                document_id,
                wants_writer,
                guard.lifecycle_epoch,
                prior_state,
            )
        };
        let lifecycle = LifecycleGuard {
            controller: self,
            epoch,
            restore: prior_state,
            finished: false,
        };
        let response = self
            .transport
            .request(CoreRequest::DocumentOpen {
                project_id: project_id.clone(),
                workspace_id,
                relative_path,
            })
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.network_enabled.store(false, Ordering::Release);
                lifecycle.finish(DocumentState::Disconnected);
                return Err(DocumentControllerError::Transport(error));
            }
        };
        let (snapshot, mut lease) = match response {
            CoreResponse::DocumentSnapshot {
                snapshot,
                writer_lease,
                ..
            } => (snapshot, writer_lease),
            CoreResponse::Error { code, message } if code == "document_not_found" => {
                lifecycle.finish(DocumentState::GoneWithLocalDraft);
                return Err(DocumentControllerError::Response(message));
            }
            r => {
                lifecycle.finish(DocumentState::Disconnected);
                return self.response_error(r);
            }
        };
        if snapshot.document_id != document_id {
            lifecycle.finish(DocumentState::GoneWithLocalDraft);
            return Err(DocumentControllerError::ResyncRequired);
        }
        if wants_writer && lease.is_none() {
            let response = self
                .transport
                .request(CoreRequest::DocumentWriterAcquire {
                    project_id,
                    document_id,
                })
                .await;
            match response {
                Err(error) => {
                    lifecycle.finish(DocumentState::Disconnected);
                    return Err(DocumentControllerError::Transport(error));
                }
                Ok(CoreResponse::DocumentWriterLease { writer_lease, .. }) => {
                    lease = Some(writer_lease);
                }
                Ok(_) => lease = None,
            }
        }
        {
            let mut guard = self.replica.lock().expect("replica mutex poisoned");
            let a = guard
                .attachment
                .as_mut()
                .ok_or(DocumentControllerError::NotOpen)?;
            a.writer_lease = lease;
            a.generation = self.generation;
        }
        if let Err(error) = self.install_snapshot(snapshot) {
            let state = if matches!(error, DocumentControllerError::ResyncRequired) {
                DocumentState::ResyncRequired
            } else {
                DocumentState::Disconnected
            };
            lifecycle.finish(state);
            return Err(error);
        }
        let state = {
            let guard = self.replica.lock().expect("replica mutex poisoned");
            let a = guard
                .attachment
                .as_ref()
                .ok_or(DocumentControllerError::NotOpen)?;
            if a.conflicted {
                DocumentState::Conflict
            } else if a.writer_lease.is_none() {
                DocumentState::ReadOnly
            } else if a.pending.is_empty() && !a.dirty {
                DocumentState::Synced
            } else {
                DocumentState::DirtyLocal
            }
        };
        lifecycle.finish(state.clone());
        self.network_enabled.store(true, Ordering::Release);
        if state == DocumentState::DirtyLocal && wants_writer {
            drop(_op);
            self.schedule_flush();
            self.flush().await?;
        }
        Ok(())
    }
    /// Poll authorized metadata for an attached document. Observers receive
    /// revision/status only and must fetch a snapshot when the revision moves.
    pub async fn poll_status(&self) -> Result<(u64, bool, bool, bool), DocumentControllerError> {
        let (project_id, document_id) = {
            let guard = self.replica.lock().expect("replica mutex poisoned");
            let a = guard
                .attachment
                .as_ref()
                .ok_or(DocumentControllerError::NotOpen)?;
            (a.project_id.clone(), a.document_id.clone())
        };
        match self
            .transport
            .request(CoreRequest::DocumentStatusGet {
                project_id,
                document_id,
            })
            .await
            .map_err(DocumentControllerError::Transport)?
        {
            CoreResponse::DocumentStatus {
                revision,
                dirty,
                conflicted,
                writer,
                ..
            } => Ok((revision, dirty, conflicted, writer)),
            r => self.response_error(r),
        }
    }
    fn install_snapshot(
        &self,
        snapshot: DocumentSnapshotDto,
    ) -> Result<(), DocumentControllerError> {
        let mut g = self.replica.lock().expect("replica mutex poisoned");
        let a = g
            .attachment
            .as_mut()
            .ok_or(DocumentControllerError::NotOpen)?;
        if snapshot.document_id != a.document_id || snapshot.project_id != a.project_id {
            g.state = DocumentState::GoneWithLocalDraft;
            return Err(DocumentControllerError::ResyncRequired);
        }
        if !a.pending.is_empty() {
            if let Some(accepted) = a.pending.iter().rposition(|pending| {
                pending.base_revision.saturating_add(1) == snapshot.revision
                    && snapshot.text == pending.resulting_snapshot.to_string()
            }) {
                for _ in 0..=accepted {
                    if let Some(pending) = a.pending.pop_front() {
                        a.pending_bytes = a.pending_bytes.saturating_sub(
                            pending
                                .transaction
                                .edits
                                .iter()
                                .map(|e| e.insert.len())
                                .sum(),
                        );
                    }
                }
            } else if a.pending.front().is_some_and(|pending| {
                pending.base_revision == snapshot.revision
                    && snapshot.text == pending.base_snapshot.to_string()
            }) {
                // The server still has the exact base snapshot. Retry the
                // unchanged queue and its stable change IDs after reattach.
            } else {
                g.state = DocumentState::ResyncRequired;
                return Err(DocumentControllerError::ResyncRequired);
            }
        }
        if snapshot.text.len() > MAX_DOCUMENT_TEXT_BYTES {
            return Err(DocumentControllerError::Response(
                "document snapshot exceeds the protocol size limit".into(),
            ));
        }
        if a.pending.is_empty() {
            a.buffer = DocumentBuffer::new(&snapshot.text);
        }
        a.daemon_revision = snapshot.revision;
        a.dirty = snapshot.dirty || !a.pending.is_empty();
        a.conflicted = snapshot.conflicted;
        Ok(())
    }
    pub async fn close(&self) -> Result<(), DocumentControllerError> {
        let _op = self.operation.lock().await;
        let (project, id, epoch, prior_state) = {
            let mut a = self.replica.lock().expect("replica mutex poisoned");
            let (project, id) = {
                let attachment = a
                    .attachment
                    .as_ref()
                    .ok_or(DocumentControllerError::NotOpen)?;
                if !attachment.pending.is_empty() {
                    return Err(DocumentControllerError::ResyncRequired);
                }
                (
                    attachment.project_id.clone(),
                    attachment.document_id.clone(),
                )
            };
            let prior_state = a.state.clone();
            a.lifecycle = ControllerLifecycle::Closing;
            a.lifecycle_epoch = a.lifecycle_epoch.wrapping_add(1);
            a.state = DocumentState::Opening;
            (project, id, a.lifecycle_epoch, prior_state)
        };
        let lifecycle = LifecycleGuard {
            controller: self,
            epoch,
            restore: prior_state.clone(),
            finished: false,
        };
        let r = self
            .transport
            .request(CoreRequest::DocumentClose {
                project_id: project,
                document_id: id,
            })
            .await;
        let r = match r {
            Ok(response) => response,
            Err(error) => {
                lifecycle.finish(prior_state);
                return Err(DocumentControllerError::Transport(error));
            }
        };
        if matches!(r, CoreResponse::Ack) {
            let mut replica = self.replica.lock().expect("replica mutex poisoned");
            if replica.lifecycle_epoch != epoch {
                return Err(DocumentControllerError::LifecycleBusy);
            }
            replica.attachment = None;
            drop(replica);
            lifecycle.finish(DocumentState::Closed);
            Ok(())
        } else {
            lifecycle.finish(prior_state);
            self.response_error(r)
        }
    }
    fn response_error<T>(&self, response: CoreResponse) -> Result<T, DocumentControllerError> {
        Err(DocumentControllerError::Response(format!("{response:?}")))
    }
}
fn transaction_to_dto(tx: &TextTransaction) -> DocumentTransactionDto {
    DocumentTransactionDto {
        edits: tx
            .edits
            .iter()
            .map(|e| DocumentTextEditDto {
                range: DocumentTextRangeDto {
                    start: e.range.start,
                    end: e.range.end,
                },
                insert: e.insert.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use tokio::sync::Mutex as AsyncMutex;

    struct FakeTransport {
        responses: AsyncMutex<VecDeque<Result<CoreResponse, String>>>,
        requests: AsyncMutex<Vec<CoreRequest>>,
    }

    struct SaveGateTransport {
        save_entered: tokio::sync::Notify,
        release_save: AsyncMutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        close_entered: tokio::sync::Notify,
        release_close: AsyncMutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        close_fails: bool,
        requests: AsyncMutex<Vec<CoreRequest>>,
        revision: AtomicU64,
    }

    struct FlushGateTransport {
        entered: tokio::sync::Notify,
        release: AsyncMutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        completed: AtomicU64,
        in_flight: AtomicU64,
        max_in_flight: AtomicU64,
        completion_notify: tokio::sync::Notify,
    }

    #[async_trait]
    impl DocumentTransport for FlushGateTransport {
        async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
            match request {
                CoreRequest::DocumentOpen { .. } => Ok(CoreResponse::DocumentSnapshot {
                    snapshot: DocumentSnapshotDto {
                        document_id: "doc-1".into(),
                        project_id: "project-1".into(),
                        workspace_id: "workspace-1".into(),
                        relative_path: "src/lib.rs".into(),
                        revision: 0,
                        text: "".into(),
                        dirty: false,
                        conflicted: false,
                        writer: true,
                        disk_base_digest: "digest".into(),
                    },
                    writer_lease: Some("lease-1".into()),
                    lsp_degraded: false,
                }),
                CoreRequest::DocumentChange { .. } => {
                    let current = self.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
                    self.max_in_flight.fetch_max(current, Ordering::AcqRel);
                    if self.completed.load(Ordering::Acquire) == 0 {
                        self.entered.notify_one();
                        if let Some(release) = self.release.lock().await.take() {
                            release.await.map_err(|e| e.to_string())?;
                        }
                    }
                    let revision = self.completed.fetch_add(1, Ordering::AcqRel) + 1;
                    self.in_flight.fetch_sub(1, Ordering::AcqRel);
                    self.completion_notify.notify_one();
                    Ok(CoreResponse::DocumentChanged {
                        revision,
                        lsp_degraded: false,
                    })
                }
                _ => Ok(CoreResponse::Ack),
            }
        }
    }

    #[async_trait]
    impl DocumentTransport for SaveGateTransport {
        async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
            self.requests.lock().await.push(request.clone());
            match request {
                CoreRequest::DocumentOpen { .. } => Ok(CoreResponse::DocumentSnapshot {
                    snapshot: DocumentSnapshotDto {
                        document_id: "doc-1".into(),
                        project_id: "project-1".into(),
                        workspace_id: "workspace-1".into(),
                        relative_path: "src/lib.rs".into(),
                        revision: 0,
                        text: "a".into(),
                        dirty: false,
                        conflicted: false,
                        writer: true,
                        disk_base_digest: "digest".into(),
                    },
                    writer_lease: Some("lease-1".into()),
                    lsp_degraded: false,
                }),
                CoreRequest::DocumentSave { .. } => {
                    self.save_entered.notify_one();
                    if let Some(release) = self.release_save.lock().await.take() {
                        release.await.map_err(|e| e.to_string())?;
                    }
                    Ok(CoreResponse::DocumentSaved {
                        revision: self.revision.load(Ordering::Acquire),
                        disk_base_digest: "saved".into(),
                        lsp_degraded: false,
                    })
                }
                CoreRequest::DocumentChange { .. } => {
                    let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
                    Ok(CoreResponse::DocumentChanged {
                        revision,
                        lsp_degraded: false,
                    })
                }
                CoreRequest::DocumentClose { .. } => {
                    self.close_entered.notify_one();
                    if let Some(release) = self.release_close.lock().await.take() {
                        release.await.map_err(|e| e.to_string())?;
                    }
                    if self.close_fails {
                        Ok(CoreResponse::Error {
                            code: "close_failed".into(),
                            message: "close failed".into(),
                        })
                    } else {
                        Ok(CoreResponse::Ack)
                    }
                }
                _ => Ok(CoreResponse::Ack),
            }
        }
    }

    #[async_trait]
    impl DocumentTransport for FakeTransport {
        async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
            self.requests.lock().await.push(request);
            self.responses.lock().await.pop_front().unwrap()
        }
    }

    #[test]
    fn transaction_dto_preserves_order_and_ranges() {
        let tx = TextTransaction::new(vec![codegg_document::TextEdit::new(1..1, "X")]);
        let dto = transaction_to_dto(&tx);
        assert_eq!(dto.edits[0].range.start, 1);
        assert_eq!(dto.edits[0].range.end, 1);
        assert_eq!(dto.edits[0].insert, "X");
    }

    #[tokio::test]
    async fn uncertain_change_retries_with_same_id_and_optimistic_text() {
        let transport = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([
                Ok(CoreResponse::DocumentSnapshot {
                    snapshot: DocumentSnapshotDto {
                        document_id: "doc-1".into(),
                        project_id: "project-1".into(),
                        workspace_id: "workspace-1".into(),
                        relative_path: "src/lib.rs".into(),
                        revision: 4,
                        text: "ab".into(),
                        dirty: false,
                        conflicted: false,
                        writer: true,
                        disk_base_digest: "digest".into(),
                    },
                    writer_lease: Some("lease-1".into()),
                    lsp_degraded: false,
                }),
                Err("response lost after send".into()),
                Ok(CoreResponse::DocumentChanged {
                    revision: 5,
                    lsp_degraded: false,
                }),
            ])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let id = controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                1..1,
                "X",
            )]))
            .unwrap();
        let (snapshot, revision) = controller.snapshot().await.unwrap();
        assert_eq!(snapshot.to_string(), "aXb");
        assert_eq!(revision, 4);
        assert!(controller.flush().await.is_err());
        assert_eq!(controller.state().await, DocumentState::Disconnected);
        controller.flush().await.unwrap();
        let (snapshot, revision) = controller.snapshot().await.unwrap();
        assert_eq!(snapshot.to_string(), "aXb");
        assert_eq!(revision, 5);
        let requests = transport.requests.lock().await;
        let ids: Vec<_> = requests
            .iter()
            .filter_map(|request| match request {
                CoreRequest::DocumentChange { change_id, .. } => Some(change_id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![&id, &id]);
    }

    #[tokio::test]
    async fn adjacent_unsent_insertions_coalesce_before_flush() {
        let transport = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([
                Ok(CoreResponse::DocumentSnapshot {
                    snapshot: DocumentSnapshotDto {
                        document_id: "doc-1".into(),
                        project_id: "project-1".into(),
                        workspace_id: "workspace-1".into(),
                        relative_path: "src/lib.rs".into(),
                        revision: 0,
                        text: "".into(),
                        dirty: false,
                        conflicted: false,
                        writer: true,
                        disk_base_digest: "digest".into(),
                    },
                    writer_lease: Some("lease-1".into()),
                    lsp_degraded: false,
                }),
                Ok(CoreResponse::DocumentChanged {
                    revision: 1,
                    lsp_degraded: false,
                }),
            ])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let first = controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                0..0,
                "a",
            )]))
            .unwrap();
        let second = controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                1..1,
                "b",
            )]))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(controller.snapshot().await.unwrap().0.to_string(), "ab");
        controller.flush().await.unwrap();
        let requests = transport.requests.lock().await;
        let changes: Vec<_> = requests
            .iter()
            .filter_map(|request| match request {
                CoreRequest::DocumentChange { transaction, .. } => Some(transaction),
                _ => None,
            })
            .collect();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].edits[0].insert, "ab");
    }

    #[tokio::test]
    async fn reconnect_reattaches_and_retries_uncertain_change_with_same_id() {
        let disconnected = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([
                Ok(CoreResponse::DocumentSnapshot {
                    snapshot: DocumentSnapshotDto {
                        document_id: "doc-1".into(),
                        project_id: "project-1".into(),
                        workspace_id: "workspace-1".into(),
                        relative_path: "src/lib.rs".into(),
                        revision: 0,
                        text: "a".into(),
                        dirty: false,
                        conflicted: false,
                        writer: true,
                        disk_base_digest: "digest".into(),
                    },
                    writer_lease: Some("old-lease".into()),
                    lsp_degraded: false,
                }),
                Err("connection lost after write".into()),
            ])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let reconnected = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([
                Ok(CoreResponse::DocumentSnapshot {
                    snapshot: DocumentSnapshotDto {
                        document_id: "doc-1".into(),
                        project_id: "project-1".into(),
                        workspace_id: "workspace-1".into(),
                        relative_path: "src/lib.rs".into(),
                        revision: 0,
                        text: "a".into(),
                        dirty: false,
                        conflicted: false,
                        writer: false,
                        disk_base_digest: "digest".into(),
                    },
                    writer_lease: None,
                    lsp_degraded: false,
                }),
                Ok(CoreResponse::DocumentWriterLease {
                    document_id: "doc-1".into(),
                    writer_lease: "new-lease".into(),
                }),
                Ok(CoreResponse::DocumentChanged {
                    revision: 1,
                    lsp_degraded: false,
                }),
            ])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(disconnected.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let change_id = controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                1..1,
                "X",
            )]))
            .unwrap();
        assert!(controller.flush().await.is_err());
        controller.reconnect(reconnected.clone()).await.unwrap();
        assert_eq!(controller.state().await, DocumentState::DirtyLocal);
        let old_id = disconnected
            .requests
            .lock()
            .await
            .iter()
            .find_map(|request| match request {
                CoreRequest::DocumentChange { change_id, .. } => Some(change_id.clone()),
                _ => None,
            })
            .unwrap();
        let new_id = reconnected
            .requests
            .lock()
            .await
            .iter()
            .find_map(|request| match request {
                CoreRequest::DocumentChange { change_id, .. } => Some(change_id.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(old_id, change_id);
        assert_eq!(new_id, change_id);
        let (snapshot, revision) = controller.snapshot().await.unwrap();
        assert_eq!(snapshot.to_string(), "aX");
        assert_eq!(revision, 1);
    }

    #[tokio::test]
    async fn mib_snapshot_clones_remain_rope_backed_during_local_edit() {
        let text = "x".repeat(1024 * 1024);
        let transport = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([Ok(CoreResponse::DocumentSnapshot {
                snapshot: DocumentSnapshotDto {
                    document_id: "doc-1".into(),
                    project_id: "project-1".into(),
                    workspace_id: "workspace-1".into(),
                    relative_path: "src/lib.rs".into(),
                    revision: 0,
                    text,
                    dirty: false,
                    conflicted: false,
                    writer: true,
                    disk_base_digest: "digest".into(),
                },
                writer_lease: Some("lease-1".into()),
                lsp_degraded: false,
            })])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(transport));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let _change = controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                1024..1024,
                "y",
            )]))
            .unwrap();
        let (snapshot, _) = controller.snapshot().await.unwrap();
        let shared_clone = snapshot.clone();
        assert_eq!(snapshot.len_bytes(), 1024 * 1024 + 1);
        assert_eq!(shared_clone.len_bytes(), snapshot.len_bytes());
    }

    #[tokio::test]
    async fn daemon_restart_keeps_gone_document_draft_without_replacing_it() {
        let old = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([Ok(CoreResponse::DocumentSnapshot {
                snapshot: DocumentSnapshotDto {
                    document_id: "old-doc".into(),
                    project_id: "project-1".into(),
                    workspace_id: "workspace-1".into(),
                    relative_path: "src/lib.rs".into(),
                    revision: 0,
                    text: "disk".into(),
                    dirty: false,
                    conflicted: false,
                    writer: true,
                    disk_base_digest: "digest".into(),
                },
                writer_lease: Some("lease".into()),
                lsp_degraded: false,
            })])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(old));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                4..4,
                " draft",
            )]))
            .unwrap();
        let restarted = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([Ok(CoreResponse::Error {
                code: "document_not_found".into(),
                message: "daemon document identity is gone".into(),
            })])),
            requests: AsyncMutex::new(Vec::new()),
        });
        assert!(controller.reconnect(restarted).await.is_err());
        assert_eq!(controller.state().await, DocumentState::GoneWithLocalDraft);
        assert_eq!(
            controller.snapshot().await.unwrap().0.to_string(),
            "disk draft"
        );
    }

    #[tokio::test]
    async fn save_fence_keeps_edits_accepted_while_save_is_in_flight() {
        let (release, gate) = tokio::sync::oneshot::channel();
        let transport = Arc::new(SaveGateTransport {
            save_entered: tokio::sync::Notify::new(),
            release_save: AsyncMutex::new(Some(gate)),
            close_entered: tokio::sync::Notify::new(),
            release_close: AsyncMutex::new(None),
            close_fails: false,
            requests: AsyncMutex::new(Vec::new()),
            revision: AtomicU64::new(0),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let save_controller = controller.clone();
        let save = tokio::spawn(async move { save_controller.save().await });
        transport.save_entered.notified().await;
        controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                1..1,
                "new",
            )]))
            .unwrap();
        release.send(()).unwrap();
        save.await.unwrap().unwrap();
        assert_eq!(controller.snapshot().await.unwrap().0.to_string(), "anew");
        assert!(matches!(
            controller.state().await,
            DocumentState::DirtyLocal | DocumentState::Flushing
        ));
        controller.flush().await.unwrap();
        assert_eq!(controller.snapshot().await.unwrap().0.to_string(), "anew");
    }

    #[tokio::test]
    async fn repeated_open_is_rejected_without_a_second_remote_open() {
        let transport = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([Ok(CoreResponse::DocumentSnapshot {
                snapshot: DocumentSnapshotDto {
                    document_id: "doc-1".into(),
                    project_id: "project-1".into(),
                    workspace_id: "workspace-1".into(),
                    relative_path: "src/lib.rs".into(),
                    revision: 0,
                    text: "".into(),
                    dirty: false,
                    conflicted: false,
                    writer: true,
                    disk_base_digest: "d".into(),
                },
                writer_lease: Some("lease".into()),
                lsp_degraded: false,
            })])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        assert!(matches!(
            controller
                .open(
                    "project-1".into(),
                    "workspace-1".into(),
                    "other.rs".into(),
                    true
                )
                .await,
            Err(DocumentControllerError::AlreadyOpen)
        ));
        assert_eq!(
            transport
                .requests
                .lock()
                .await
                .iter()
                .filter(|r| matches!(r, CoreRequest::DocumentOpen { .. }))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn failed_open_returns_to_closed_state() {
        let transport = Arc::new(FakeTransport {
            responses: AsyncMutex::new(VecDeque::from([Err("offline".into())])),
            requests: AsyncMutex::new(Vec::new()),
        });
        let controller = Arc::new(DocumentController::new(transport));
        assert!(controller
            .open("p".into(), "w".into(), "f".into(), false)
            .await
            .is_err());
        assert_eq!(controller.state().await, DocumentState::Closed);
    }

    #[tokio::test]
    async fn close_fence_rejects_typing_and_failed_close_restores_attachment() {
        let (release, gate) = tokio::sync::oneshot::channel();
        let transport = Arc::new(SaveGateTransport {
            save_entered: tokio::sync::Notify::new(),
            release_save: AsyncMutex::new(None),
            close_entered: tokio::sync::Notify::new(),
            release_close: AsyncMutex::new(Some(gate)),
            close_fails: true,
            requests: AsyncMutex::new(Vec::new()),
            revision: AtomicU64::new(0),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let close_controller = controller.clone();
        let close = tokio::spawn(async move { close_controller.close().await });
        transport.close_entered.notified().await;
        let edit =
            controller.apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                0..0,
                "x",
            )]));
        assert!(matches!(edit, Err(DocumentControllerError::LifecycleBusy)));
        release.send(()).unwrap();
        assert!(close.await.unwrap().is_err());
        assert_eq!(controller.state().await, DocumentState::Synced);
        assert_eq!(controller.snapshot().await.unwrap().0.to_string(), "a");
    }

    #[tokio::test]
    async fn cancelled_close_releases_the_lifecycle_fence() {
        let (_release, gate) = tokio::sync::oneshot::channel();
        let transport = Arc::new(SaveGateTransport {
            save_entered: tokio::sync::Notify::new(),
            release_save: AsyncMutex::new(None),
            close_entered: tokio::sync::Notify::new(),
            release_close: AsyncMutex::new(Some(gate)),
            close_fails: false,
            requests: AsyncMutex::new(Vec::new()),
            revision: AtomicU64::new(0),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        let close_controller = controller.clone();
        let close = tokio::spawn(async move { close_controller.close().await });
        transport.close_entered.notified().await;
        close.abort();
        let _ = close.await;
        assert!(controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                0..0,
                "x"
            )]))
            .is_ok());
    }

    #[tokio::test]
    async fn blocked_flush_owns_one_worker_through_a_local_edit_burst() {
        let (release, gate) = tokio::sync::oneshot::channel();
        let transport = Arc::new(FlushGateTransport {
            entered: tokio::sync::Notify::new(),
            release: AsyncMutex::new(Some(gate)),
            completed: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            max_in_flight: AtomicU64::new(0),
            completion_notify: tokio::sync::Notify::new(),
        });
        let controller = Arc::new(DocumentController::new(transport.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
            )
            .await
            .unwrap();
        controller
            .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                0..0,
                "0",
            )]))
            .unwrap();
        transport.entered.notified().await;
        for i in 1..=25 {
            controller
                .apply_local(TextTransaction::new(vec![codegg_document::TextEdit::new(
                    0..0,
                    i.to_string(),
                )]))
                .unwrap();
        }
        assert!(controller.flush_worker_running.load(Ordering::Acquire));
        release.send(()).unwrap();
        while transport.completed.load(Ordering::Acquire) < 26 {
            transport.completion_notify.notified().await;
        }
        assert_eq!(transport.max_in_flight.load(Ordering::Acquire), 1);
        assert_eq!(controller.snapshot().await.unwrap().0.len_bytes(), 42);
    }
}
