//! Additive document.v1 protocol handler.
//!
//! # Two authorities, deliberately not one
//!
//! Every handler below dispatches on [`DocumentScope`]. The
//! `Workspace` arms are unchanged and remain workspace-contained: the path
//! resolves against the bound workspace root under the usual relative-path
//! rules, and every write is gated by that workspace's repository lock. The
//! `GlobalAgent` arms resolve a *different* root through
//! [`global_agent_authority`], take a different lock, accept only a bare
//! `<name>.toml`, and skip every workspace-coupled behaviour — LSP managed
//! documents, external-disk refresh, workspace service acquisition.
//!
//! Keeping them as separate match arms rather than one arm with a
//! conditional root is the point: the two authorities have genuinely
//! different containment, different locking, and different side effects,
//! and folding them together is how a "just this once" LSP sync or lock
//! ends up applied to a path outside the workspace it was written for.

use super::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::document::{
    DocumentScope, WorkspaceDirectoryListRequestDto, WorkspaceDirectoryListResultDto,
    WorkspaceDirectoryListStatusDto, MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};

use crate::core::global_agent_authority as global_agent;

/// C006: deterministic blocking-worker admission probe.
///
/// Counts **actual** `spawn_blocking` worker entry/exit, not Tokio
/// task/handler counts or `Semaphore::available_permits()`. `active`
/// is the current live-worker count, `peak` the maximum observed since
/// construction or the last [`Self::reset_peak`]. Both are updated on
/// the worker thread itself under a worker-owned RAII guard, so an
/// aborted async waiter cannot distort them.
///
/// The optional test gate blocks (or fails/panics) the worker *after*
/// it has entered and acquired admission, letting tests prove a second
/// request answers `Busy` while the first worker is still alive.
/// Production never installs a gate.
#[derive(Debug)]
pub struct DirectoryListingProbe {
    active: AtomicUsize,
    peak: AtomicUsize,
    changed: tokio::sync::Notify,
    gate: Mutex<Option<Arc<DirectoryListingGate>>>,
}

impl DirectoryListingProbe {
    /// Fresh probe with no gate and zeroed counters.
    pub fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            changed: tokio::sync::Notify::new(),
            gate: Mutex::new(None),
        }
    }

    /// Current live blocking workers.
    pub fn active_workers(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Maximum live blocking workers observed since construction/reset.
    pub fn peak_workers(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    /// Reset the peak to the current active count.
    pub fn reset_peak(&self) {
        self.peak
            .store(self.active.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// Install a gate that blocks the next workers until released.
    /// Returns the gate so tests can release it.
    pub fn install_blocking_gate(&self) -> Arc<DirectoryListingGate> {
        let gate = Arc::new(DirectoryListingGate::blocking());
        *self.gate.lock().expect("probe gate") = Some(gate.clone());
        gate
    }

    /// Install a gate that panics the worker after entry.
    pub fn install_panicking_gate(&self) -> Arc<DirectoryListingGate> {
        let gate = Arc::new(DirectoryListingGate::panicking());
        *self.gate.lock().expect("probe gate") = Some(gate.clone());
        gate
    }

    /// Install a gate that fails the worker after entry.
    pub fn install_failing_gate(&self) -> Arc<DirectoryListingGate> {
        let gate = Arc::new(DirectoryListingGate::failing());
        *self.gate.lock().expect("probe gate") = Some(gate.clone());
        gate
    }

    /// Remove any installed gate.
    pub fn clear_gate(&self) {
        *self.gate.lock().expect("probe gate") = None;
    }

    fn take_gate(&self) -> Option<Arc<DirectoryListingGate>> {
        self.gate.lock().expect("probe gate").clone()
    }

    fn enter(&self) -> WorkerActiveGuard<'_> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        self.changed.notify_one();
        WorkerActiveGuard { probe: self }
    }

    /// Wait until `active == expected` or `timeout` elapses.
    /// Barrier/notifier based; no fixed sleeps.
    pub async fn wait_for_active(&self, expected: usize, timeout: std::time::Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.active_workers() == expected {
                return true;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return self.active_workers() == expected;
            }
            let notified = self.changed.notified();
            if tokio::time::timeout(remaining, notified).await.is_err() {
                return self.active_workers() == expected;
            }
        }
    }
}

impl Default for DirectoryListingProbe {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for one live blocking worker. Decrements `active` and
/// notifies waiters on drop, including panic unwind.
struct WorkerActiveGuard<'a> {
    probe: &'a DirectoryListingProbe,
}

impl Drop for WorkerActiveGuard<'_> {
    fn drop(&mut self) {
        self.probe.active.fetch_sub(1, Ordering::SeqCst);
        self.probe.changed.notify_one();
    }
}

/// Test-only worker behaviour after entry. Production never installs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryListingGateBehavior {
    /// Block on a condvar until [`DirectoryListingGate::release`].
    BlockUntilReleased,
    /// Panic after entry (tests permit release on unwind).
    PanicAfterEnter,
    /// Return an injected I/O error after entry.
    FailAfterEnter,
}

/// Deterministic barrier for the blocking worker. The worker signals
/// entry via the probe's `active` counter (async waiters use
/// [`DirectoryListingProbe::wait_for_active`]); release is a blocking
/// condvar wait so it works on the `spawn_blocking` thread.
#[derive(Debug)]
pub struct DirectoryListingGate {
    behavior: DirectoryListingGateBehavior,
    released: Mutex<bool>,
    released_cvar: Condvar,
}

impl DirectoryListingGate {
    fn blocking() -> Self {
        Self {
            behavior: DirectoryListingGateBehavior::BlockUntilReleased,
            released: Mutex::new(false),
            released_cvar: Condvar::new(),
        }
    }

    fn panicking() -> Self {
        Self {
            behavior: DirectoryListingGateBehavior::PanicAfterEnter,
            released: Mutex::new(false),
            released_cvar: Condvar::new(),
        }
    }

    fn failing() -> Self {
        Self {
            behavior: DirectoryListingGateBehavior::FailAfterEnter,
            released: Mutex::new(false),
            released_cvar: Condvar::new(),
        }
    }

    /// Release a blocking gate. Idempotent.
    pub fn release(&self) {
        let mut released = self.released.lock().expect("gate release");
        *released = true;
        self.released_cvar.notify_all();
    }

    fn behavior(&self) -> DirectoryListingGateBehavior {
        self.behavior
    }

    fn wait_for_release(&self) {
        let mut released = self.released.lock().expect("gate wait");
        while !*released {
            released = self.released_cvar.wait(released).expect("gate wait");
        }
    }
}

impl CoreDaemon {
    /// Confirm `workspace_id` is one of `project_id`'s workspaces.
    ///
    /// Returns a ready-to-use error code so each call site keeps its existing
    /// response shape. Pool-less daemons skip the check, as they always have:
    /// they hold no catalog to check against.
    async fn verify_project_workspace(
        &self,
        project_id: &str,
        workspace_id: &codegg_core::workspace::WorkspaceId,
    ) -> Result<(), &'static str> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let Ok(project_id_parsed) = codegg_core::identity::ProjectId::parse(project_id) else {
            return Err("document_not_found");
        };
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        match catalog
            .list_workspaces_for_project(&project_id_parsed)
            .await
        {
            Ok(workspaces) if workspaces.iter().any(|ws| &ws.workspace_id == workspace_id) => {
                Ok(())
            }
            _ => Err("document_not_found"),
        }
    }

    async fn sync_managed_document(
        &self,
        snapshot: &codegg_protocol::document::DocumentSnapshotDto,
    ) -> bool {
        // M003: the sync witness. A revision is recorded only when the push
        // below succeeds; every other exit clears the witness, so a revision
        // that never reached a server can never later read as synchronized.
        // Eviction clears it too (see `DocumentService`), matching the
        // `didClose` the close path sends exactly on eviction.
        let Some(service) = &self.deps.lsp_service else {
            return false;
        };
        let workspace_id = match codegg_core::workspace::WorkspaceId::parse(&snapshot.workspace_id)
        {
            Ok(workspace_id) => workspace_id,
            Err(_) => {
                self.documents.clear_lsp_synced(&snapshot.document_id);
                return true;
            }
        };
        let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
            self.documents.clear_lsp_synced(&snapshot.document_id);
            return true;
        };
        let path = workspace.canonical_root.join(&snapshot.relative_path);
        let degraded = service
            .set_managed_document(&path, &snapshot.text, snapshot.dirty)
            .await
            .is_err();
        if degraded {
            self.documents.clear_lsp_synced(&snapshot.document_id);
        } else {
            self.documents
                .record_lsp_synced(&snapshot.document_id, snapshot.revision);
        }
        degraded
    }

    /// Re-read the document's bytes from disk and reconcile the snapshot.
    ///
    /// `repository_lock_held` says the caller already holds the
    /// workspace's repository lock. It must: the create path holds that
    /// lock across create-then-open so a concurrent write to the same new
    /// path cannot land between the two steps and be silently adopted as
    /// our snapshot, and re-acquiring the same lock here would wait for
    /// the guard the caller is still holding — a self-deadlock.
    async fn refresh_external_disk(
        &self,
        mut snapshot: codegg_protocol::document::DocumentSnapshotDto,
        client_id: &str,
        repository_lock_held: bool,
    ) -> codegg_protocol::document::DocumentSnapshotDto {
        let Ok(workspace_id) = codegg_core::workspace::WorkspaceId::parse(&snapshot.workspace_id)
        else {
            return snapshot;
        };
        let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
            return snapshot;
        };
        let Ok(services) = self.workspace_services.acquire(&workspace_id).await else {
            return snapshot;
        };
        let _guard = if repository_lock_held {
            None
        } else {
            Some(
                services
                    .locks()
                    .acquire_repository(&workspace.canonical_root)
                    .await,
            )
        };
        let Ok(path) = crate::tool::util::validate_path(
            std::path::Path::new(&snapshot.relative_path),
            &workspace.canonical_root,
        ) else {
            return snapshot;
        };
        let bounded = tokio::fs::metadata(&path).await.is_ok_and(|meta| {
            meta.len() <= codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES as u64
        });
        if !bounded {
            return snapshot;
        }
        let Ok(bytes) = tokio::fs::read(&path).await else {
            return snapshot;
        };
        let Ok(text) = String::from_utf8(bytes.clone()) else {
            return snapshot;
        };
        let _ = self
            .documents
            .refresh_clean_from_disk(
                &snapshot.document_id,
                &snapshot.project_id,
                client_id,
                snapshot.revision,
                text,
                sha256(&bytes),
            )
            .await;
        if let Ok(refreshed) = self
            .documents
            .snapshot(&snapshot.document_id, &snapshot.project_id, client_id)
            .await
        {
            snapshot = refreshed;
        }
        snapshot
    }

    pub(crate) async fn handle_document_request(
        &self,
        request: CoreRequest,
        client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::DocumentCapabilities => Ok(CoreResponse::DocumentCapabilities {
                capability: codegg_protocol::document::DOCUMENT_PROTOCOL_CAPABILITY.to_owned(),
                supported: true,
                max_document_bytes: codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES,
                max_edits: codegg_protocol::document::MAX_DOCUMENT_EDITS,
                max_insert_bytes: codegg_protocol::document::MAX_DOCUMENT_INSERT_BYTES,
            }),
            CoreRequest::DocumentOpen {
                project_id,
                workspace_id,
                relative_path,
                scope,
            } => {
                if scope.is_global_agent() {
                    return Ok(self
                        .open_global_agent(&project_id, &workspace_id, &relative_path, client_id)
                        .await);
                }
                let workspace_id = match codegg_core::workspace::WorkspaceId::parse(&workspace_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(doc_error("document_invalid_workspace", error.to_string()))
                    }
                };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(doc_error("document_not_found", "workspace not found"));
                };
                if let Err(code) = self
                    .verify_project_workspace(&project_id, &workspace_id)
                    .await
                {
                    return Ok(doc_error(code, "project or workspace not found"));
                }
                match self
                    .documents
                    .open(
                        project_id,
                        workspace_id.as_str().to_owned(),
                        &workspace.canonical_root,
                        &relative_path,
                        client_id,
                        false,
                        DocumentScope::Workspace,
                    )
                    .await
                {
                    Ok((snapshot, writer_lease)) => {
                        let snapshot = self.refresh_external_disk(snapshot, client_id, false).await;
                        let lsp_degraded = self.sync_managed_document(&snapshot).await;
                        Ok(CoreResponse::DocumentSnapshot {
                            snapshot,
                            writer_lease,
                            lsp_degraded,
                        })
                    }
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentCreate {
                project_id,
                workspace_id,
                relative_path,
                initial_text,
                scope,
            } => {
                if scope.is_global_agent() {
                    return Ok(self
                        .create_global_agent(
                            &project_id,
                            &workspace_id,
                            &relative_path,
                            &initial_text,
                            client_id,
                        )
                        .await);
                }
                let workspace_id = match codegg_core::workspace::WorkspaceId::parse(&workspace_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(doc_error("document_invalid_workspace", error.to_string()))
                    }
                };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(doc_error("document_not_found", "workspace not found"));
                };
                if let Err(code) = self
                    .verify_project_workspace(&project_id, &workspace_id)
                    .await
                {
                    return Ok(doc_error(code, "project or workspace not found"));
                }
                // Hold the repository lock across create-then-open so a
                // concurrent write to the same new path cannot land between
                // the two steps and be silently adopted as our snapshot.
                let services = match self.workspace_services.acquire(&workspace_id).await {
                    Ok(services) => services,
                    Err(_) => {
                        return Ok(doc_error(
                            "document_workspace_unavailable",
                            "workspace services unavailable",
                        ))
                    }
                };
                let _workspace = services
                    .locks()
                    .acquire_repository(&workspace.canonical_root)
                    .await;
                match crate::lsp::mutation::checked_workspace_file_create(
                    &workspace.canonical_root,
                    &relative_path,
                    &initial_text,
                )
                .await
                {
                    crate::lsp::mutation::CheckedFileCreate::AlreadyExists => {
                        return Ok(doc_error(
                            "document_already_exists",
                            "a file already exists at that path — open it to edit",
                        ))
                    }
                    crate::lsp::mutation::CheckedFileCreate::Rejected(message) => {
                        return Ok(doc_error("document_invalid_path", message))
                    }
                    crate::lsp::mutation::CheckedFileCreate::Created { .. } => {}
                }
                // The file now exists, so the ordinary open path attaches it
                // with every containment and authorization check it always
                // had. Creating the bytes and attaching the replica stay two
                // auditable steps rather than one fused operation.
                match self
                    .documents
                    .open(
                        project_id,
                        workspace_id.as_str().to_owned(),
                        &workspace.canonical_root,
                        &relative_path,
                        client_id,
                        true,
                        DocumentScope::Workspace,
                    )
                    .await
                {
                    Ok((snapshot, writer_lease)) => {
                        let snapshot = self.refresh_external_disk(snapshot, client_id, true).await;
                        let lsp_degraded = self.sync_managed_document(&snapshot).await;
                        Ok(CoreResponse::DocumentSnapshot {
                            snapshot,
                            writer_lease,
                            lsp_degraded,
                        })
                    }
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentDelete {
                project_id,
                workspace_id,
                relative_path,
                scope,
            } => {
                if scope.is_global_agent() {
                    return Ok(self.delete_global_agent(&project_id, &relative_path).await);
                }
                let workspace_id = match codegg_core::workspace::WorkspaceId::parse(&workspace_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(doc_error("document_invalid_workspace", error.to_string()))
                    }
                };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(doc_error("document_not_found", "workspace not found"));
                };
                if let Err(code) = self
                    .verify_project_workspace(&project_id, &workspace_id)
                    .await
                {
                    return Ok(doc_error(code, "project or workspace not found"));
                }
                // Deleting a file that is currently attached would leave the
                // replica writing to a path that no longer exists, so a live
                // attachment is refused rather than silently broken.
                if self
                    .documents
                    .is_path_attached(&project_id, &relative_path, DocumentScope::Workspace)
                    .await
                {
                    return Ok(doc_error(
                        "document_in_use",
                        "this file is open in the editor — close it before deleting",
                    ));
                }
                let services = match self.workspace_services.acquire(&workspace_id).await {
                    Ok(services) => services,
                    Err(_) => {
                        return Ok(doc_error(
                            "document_workspace_unavailable",
                            "workspace services unavailable",
                        ))
                    }
                };
                let _workspace = services
                    .locks()
                    .acquire_repository(&workspace.canonical_root)
                    .await;
                match crate::lsp::mutation::checked_workspace_file_delete(
                    &workspace.canonical_root,
                    &relative_path,
                )
                .await
                {
                    crate::lsp::mutation::CheckedFileDelete::Deleted => {
                        Ok(CoreResponse::DocumentDeleted {
                            relative_path,
                            existed: true,
                            scope: DocumentScope::Workspace,
                        })
                    }
                    crate::lsp::mutation::CheckedFileDelete::Missing => {
                        Ok(CoreResponse::DocumentDeleted {
                            relative_path,
                            existed: false,
                            scope: DocumentScope::Workspace,
                        })
                    }
                    crate::lsp::mutation::CheckedFileDelete::Rejected(message) => {
                        Ok(doc_error("document_invalid_path", message))
                    }
                }
            }
            CoreRequest::DocumentSnapshotGet {
                project_id,
                document_id,
            } => {
                match self
                    .documents
                    .snapshot(&document_id, &project_id, client_id)
                    .await
                {
                    Ok(snapshot) => {
                        let snapshot = self.refresh_external_disk(snapshot, client_id, false).await;
                        let lsp_degraded = self.sync_managed_document(&snapshot).await;
                        Ok(CoreResponse::DocumentSnapshot {
                            snapshot,
                            writer_lease: None,
                            lsp_degraded,
                        })
                    }
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentWriterAcquire {
                project_id,
                document_id,
            } => {
                match self
                    .documents
                    .acquire_writer(&document_id, &project_id, client_id)
                    .await
                {
                    Ok(writer_lease) => Ok(CoreResponse::DocumentWriterLease {
                        document_id,
                        writer_lease,
                    }),
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentStatusGet {
                project_id,
                document_id,
            } => {
                match self
                    .documents
                    .status(&document_id, &project_id, client_id)
                    .await
                {
                    Ok((revision, dirty, conflicted, writer)) => Ok(CoreResponse::DocumentStatus {
                        document_id,
                        revision,
                        dirty,
                        conflicted,
                        writer,
                    }),
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentChange {
                project_id,
                document_id,
                writer_lease,
                base_revision,
                change_id,
                transaction,
                ..
            } => {
                match self
                    .documents
                    .change(
                        &document_id,
                        &project_id,
                        client_id,
                        &writer_lease,
                        base_revision,
                        &change_id,
                        transaction,
                    )
                    .await
                {
                    Ok(revision) => {
                        let snapshot = self
                            .documents
                            .snapshot(&document_id, &project_id, client_id)
                            .await;
                        let lsp_degraded = match snapshot {
                            Ok(snapshot) => self.sync_managed_document(&snapshot).await,
                            Err(_) => true,
                        };
                        Ok(CoreResponse::DocumentChanged {
                            revision,
                            lsp_degraded,
                        })
                    }
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentSave {
                project_id,
                document_id,
                writer_lease,
                expected_revision,
            } => {
                let operation = match self
                    .documents
                    .operation_lock(&document_id, &project_id, client_id, &writer_lease)
                    .await
                {
                    Ok(lock) => lock,
                    Err(error) => return Ok(document_error(error)),
                };
                let _operation = operation.lock_owned().await;
                let save = match self
                    .documents
                    .capture_save(
                        &document_id,
                        &project_id,
                        client_id,
                        &writer_lease,
                        expected_revision,
                    )
                    .await
                {
                    Ok(save) => save,
                    Err(error) => return Ok(document_error(error)),
                };
                // The scope comes from the attachment, never from the
                // request: a save must land under the same authority root the
                // document was opened under, or a document opened in a
                // workspace could be written into the global agent root.
                if save.scope.is_global_agent() {
                    return Ok(self.save_global_agent(&save).await);
                }
                let workspace_id =
                    match codegg_core::workspace::WorkspaceId::parse(&save.workspace_id) {
                        Ok(id) => id,
                        Err(_) => {
                            return Ok(doc_error("document_not_found", "workspace not found"))
                        }
                    };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(doc_error("document_not_found", "workspace not found"));
                };
                let services = match self.workspace_services.acquire(&workspace_id).await {
                    Ok(services) => services,
                    Err(_) => {
                        return Ok(doc_error(
                            "document_workspace_unavailable",
                            "workspace services unavailable",
                        ))
                    }
                };
                let _workspace = services
                    .locks()
                    .acquire_repository(&workspace.canonical_root)
                    .await;
                let result = crate::lsp::mutation::checked_workspace_text_write(
                    &workspace.canonical_root,
                    &save.relative_path,
                    &save.expected_disk_digest,
                    &save.text,
                )
                .await;
                match result {
                    Ok(crate::lsp::mutation::CheckedTextWrite::Conflict { actual_hash }) => {
                        let _ = self
                            .documents
                            .mark_disk_conflict(&document_id, &project_id, &actual_hash)
                            .await;
                        Ok(doc_error(
                            "document_disk_conflict",
                            format!("disk content changed (sha256 {actual_hash})"),
                        ))
                    }
                    Err(error) => Ok(doc_error("document_save_failed", error)),
                    Ok(crate::lsp::mutation::CheckedTextWrite::Written { new_hash }) => {
                        let _ = self
                            .documents
                            .commit_save(
                                &document_id,
                                &project_id,
                                expected_revision,
                                new_hash.clone(),
                            )
                            .await;
                        crate::bus::global::GlobalEventBus::publish(
                            crate::bus::events::AppEvent::FileChanged {
                                path: save.path.display().to_string(),
                                action: "Modified".to_string(),
                                old_content: None,
                            },
                        );
                        let lsp_degraded = if let Some(service) = &self.deps.lsp_service {
                            if service
                                .set_managed_document(&save.path, &save.text, false)
                                .await
                                .is_err()
                            {
                                true
                            } else {
                                service.save_file(&save.path, None).await.is_err()
                            }
                        } else {
                            false
                        };
                        Ok(CoreResponse::DocumentSaved {
                            revision: expected_revision,
                            disk_base_digest: new_hash,
                            lsp_degraded,
                        })
                    }
                }
            }
            CoreRequest::DocumentReload {
                project_id,
                document_id,
                writer_lease,
                expected_revision,
            } => {
                let operation = match self
                    .documents
                    .operation_lock(&document_id, &project_id, client_id, &writer_lease)
                    .await
                {
                    Ok(lock) => lock,
                    Err(error) => return Ok(document_error(error)),
                };
                let _operation = operation.lock_owned().await;
                let current = match self
                    .documents
                    .snapshot(&document_id, &project_id, client_id)
                    .await
                {
                    Ok(snapshot) if snapshot.writer => snapshot,
                    _ => return Ok(doc_error("document_not_found", "document not found")),
                };
                if current.revision != expected_revision {
                    return Ok(doc_error(
                        "document_stale_revision",
                        "document revision is stale",
                    ));
                }
                let workspace_id =
                    match codegg_core::workspace::WorkspaceId::parse(&current.workspace_id) {
                        Ok(id) => id,
                        Err(_) => {
                            return Ok(doc_error("document_not_found", "workspace not found"))
                        }
                    };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(doc_error("document_not_found", "workspace not found"));
                };
                let services = match self.workspace_services.acquire(&workspace_id).await {
                    Ok(services) => services,
                    Err(_) => {
                        return Ok(doc_error(
                            "document_workspace_unavailable",
                            "workspace services unavailable",
                        ))
                    }
                };
                let _workspace = services
                    .locks()
                    .acquire_repository(&workspace.canonical_root)
                    .await;
                let path = match crate::tool::util::validate_path(
                    std::path::Path::new(&current.relative_path),
                    &workspace.canonical_root,
                ) {
                    Ok(path) => path,
                    Err(_) => {
                        return Ok(doc_error(
                            "document_invalid_path",
                            "document path is no longer safe",
                        ))
                    }
                };
                let size = match tokio::fs::metadata(&path).await {
                    Ok(meta)
                        if meta.len()
                            <= codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES as u64 =>
                    {
                        meta.len()
                    }
                    Ok(_) => {
                        return Ok(doc_error(
                            "document_resource_limit",
                            "document exceeds the text size limit",
                        ))
                    }
                    Err(error) => {
                        return Ok(doc_error("document_reload_failed", error.to_string()))
                    }
                };
                let bytes = match tokio::fs::read(&path).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return Ok(doc_error("document_reload_failed", error.to_string()))
                    }
                };
                if bytes.len() as u64 > size
                    || bytes.len() > codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES
                {
                    return Ok(doc_error(
                        "document_resource_limit",
                        "document exceeds the text size limit",
                    ));
                }
                let text = match String::from_utf8(bytes.clone()) {
                    Ok(text) => text,
                    Err(_) => {
                        return Ok(doc_error("document_invalid_text", "document is not UTF-8"))
                    }
                };
                let digest = sha256(&bytes);
                let revision = match self
                    .documents
                    .reload_verified(
                        &document_id,
                        &project_id,
                        client_id,
                        &writer_lease,
                        expected_revision,
                        text,
                        digest,
                    )
                    .await
                {
                    Ok(revision) => revision,
                    Err(error) => return Ok(document_error(error)),
                };
                let updated = self
                    .documents
                    .snapshot(&document_id, &project_id, client_id)
                    .await
                    .ok();
                let lsp_degraded = if let Some(snapshot) = updated {
                    self.sync_managed_document(&snapshot).await
                } else {
                    true
                };
                Ok(CoreResponse::DocumentReloaded {
                    revision,
                    lsp_degraded,
                })
            }
            CoreRequest::DocumentClose {
                project_id,
                document_id,
            } => {
                let snapshot = self
                    .documents
                    .snapshot(&document_id, &project_id, client_id)
                    .await
                    .ok();
                let evicted = self
                    .documents
                    .detach_document(&document_id, &project_id, client_id)
                    .await;
                if evicted {
                    if let Some(snapshot) = snapshot {
                        if let Ok(workspace_id) =
                            codegg_core::workspace::WorkspaceId::parse(&snapshot.workspace_id)
                        {
                            if let Some(workspace) = self.workspaces.resolve(&workspace_id).await {
                                if let Some(service) = &self.deps.lsp_service {
                                    let _ = service
                                        .close_file(
                                            &workspace.canonical_root.join(snapshot.relative_path),
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
                Ok(CoreResponse::Ack)
            }
            CoreRequest::WorkspaceDirectoryList { request } => {
                Ok(self.directory_listing(request).await)
            }
            _ => Ok(doc_error(
                "document_unimplemented",
                "unsupported document request",
            )),
        }
    }

    /// C005 (ADR-0022, accepted): daemon-owned workspace directory listing.
    /// C006: worker-owned single-flight admission + full-response budget.
    ///
    /// Server-side authorization already passed in
    /// `handle_request_with_client` before dispatch (`DirectProject` +
    /// `FileRead`); denials for team principals are masked as not-found by
    /// the `observe_private` rule, so this method never needs its own
    /// role logic. It resolves the daemon-owned canonical root, enforces
    /// prefix containment with the same sequence `DocumentOpen` uses,
    /// then runs the single shared walk on a blocking worker under the
    /// daemon's single-flight admission. It never reads file bytes.
    ///
    /// Admission: the `OwnedSemaphorePermit` is moved into the
    /// `spawn_blocking` closure, so dropping/aborting the async waiter
    /// cannot free a worker that is still scanning. The permit releases
    /// only when the blocking closure exits, including error and panic
    /// unwind. Contention answers `busy`, never queues.
    async fn directory_listing(&self, request: WorkspaceDirectoryListRequestDto) -> CoreResponse {
        let workspace_id = match codegg_core::workspace::WorkspaceId::parse(&request.workspace_id) {
            Ok(id) => id,
            Err(error) => {
                return doc_error("document_invalid_workspace", error.to_string());
            }
        };
        let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
            return doc_error("document_not_found", "workspace not found");
        };
        if let Err(code) = self
            .verify_project_workspace(&request.project_id, &workspace_id)
            .await
        {
            return doc_error(code, "project or workspace not found");
        }
        let prefix = match request.validate_prefix() {
            Ok(prefix) => prefix,
            Err(reason) => return doc_error("document_invalid_path", reason),
        };
        // Single-flight admission, worker-owned (C006 F1): acquire here,
        // move into the blocking closure below. An aborted async waiter
        // cannot free the worker; a second request while the worker lives
        // answers `busy`. No await sits between acquire and spawn, so an
        // abort before spawn simply drops the still-handler-owned permit
        // with no leak.
        let permit = match self.directory_listing_permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return CoreResponse::WorkspaceDirectoryList {
                    result: WorkspaceDirectoryListResultDto {
                        workspace_id: request.workspace_id,
                        prefix: request.prefix,
                        nodes: Vec::new(),
                        truncated: false,
                        truncated_reason: None,
                        examined: 0,
                        status: WorkspaceDirectoryListStatusDto::Busy,
                    },
                };
            }
        };
        let root = workspace.canonical_root.clone();
        let probe = self.directory_listing_probe.clone();
        let worker = tokio::task::spawn_blocking(move || {
            // Worker-owned admission: released only when this closure
            // exits, including error return and panic unwind. Dropping
            // or aborting the outer JoinHandle cannot release it early.
            let _permit = permit;
            // Actual-worker instrumentation: counts live blocking
            // workers, never handler/waiter counts.
            let _active = probe.enter();
            if let Some(gate) = probe.take_gate() {
                match gate.behavior() {
                    DirectoryListingGateBehavior::BlockUntilReleased => {
                        gate.wait_for_release();
                    }
                    DirectoryListingGateBehavior::PanicAfterEnter => {
                        panic!("injected directory-listing worker panic (test gate)");
                    }
                    DirectoryListingGateBehavior::FailAfterEnter => {
                        return Err("injected directory-listing worker failure");
                    }
                }
            }
            blocking_directory_scan(&root, &prefix)
        });
        let scanned = worker.await;
        // The walk never panics by construction and the containment
        // checks above already passed, so any failure here is a mid-flight
        // filesystem surprise (vanished root, I/O error), an injected test
        // failure, or a worker panic. Fail closed with no daemon paths in
        // the message. A worker panic also releases admission at actual
        // exit via the worker-owned guard above.
        let listing = match scanned {
            Ok(Ok(listing)) => listing,
            Ok(Err(_)) | Err(_) => {
                return CoreResponse::WorkspaceDirectoryList {
                    result: WorkspaceDirectoryListResultDto {
                        workspace_id: request.workspace_id,
                        prefix: request.prefix,
                        nodes: Vec::new(),
                        truncated: false,
                        truncated_reason: None,
                        examined: 0,
                        status: WorkspaceDirectoryListStatusDto::Unavailable,
                    },
                };
            }
        };
        build_bounded_listing_response(request.workspace_id, request.prefix, listing)
    }
}

/// Blocking half of the daemon directory listing. Runs on a `spawn_blocking`
/// worker: canonicalizes the daemon-owned root, confines `prefix` with the
/// same symlink/containment sequence `DocumentService::open` applies, then
/// executes the single shared walk.
fn blocking_directory_scan(
    root: &std::path::Path,
    prefix: &str,
) -> Result<crate::directory_listing::DirectoryListing, &'static str> {
    let canonical_root = std::fs::canonicalize(root).map_err(|_| "workspace root unavailable")?;
    let target = canonical_root.join(prefix);
    reject_scan_symlink_components(&canonical_root, &target)?;
    let canonical_target =
        std::fs::canonicalize(&target).map_err(|_| "listed directory unavailable")?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err("listed directory escapes the workspace root");
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    Ok(crate::directory_listing::walk_directory(
        &canonical_target,
        prefix,
        &cancel,
    ))
}

/// Mirror of `DocumentService::reject_symlink_components` for the listing
/// path: every component from the root to the target must exist without a
/// symlink, checked on raw metadata so a resolved target can never hide
/// one. Kept local (rather than sharing the private helper) so the
/// document write path and the metadata read path cannot drift through a
/// shared signature change unnoticed — both are pinned by their own tests.
fn reject_scan_symlink_components(
    root: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), &'static str> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "listed directory escapes the workspace root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if std::fs::symlink_metadata(&current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("listed directory traverses a symlink");
        }
    }
    Ok(())
}

/// Serialized byte length of the exact object the native transport
/// transmits for a listing: `CoreResponse::WorkspaceDirectoryList`,
/// via the same `serde_json` serializer the socket/stdio transports use
/// (`serde_json::to_string(&CoreFrame::Response)` wraps this payload with
/// only a routing `request_id` echo outside the listing budget).
/// Includes the response discriminator, `workspace_id`, `prefix`,
/// `status`, `examined`, `truncated`/`truncated_reason`, and every
/// retained node — never a nodes-only estimate.
pub fn serialized_listing_response_len(response: &CoreResponse) -> usize {
    serde_json::to_string(response)
        .map(|json| json.len())
        .unwrap_or(usize::MAX)
}

/// Assert a listing response fits the wire budget. Test seam: uses the
/// same serialization as transport.
pub fn listing_response_fits_budget(response: &CoreResponse) -> bool {
    serialized_listing_response_len(response) <= MAX_DIRECTORY_LISTING_RESPONSE_BYTES
}

fn listing_response(
    workspace_id: String,
    prefix: String,
    nodes: Vec<codegg_protocol::document::WorkspaceDirectoryNodeDto>,
    truncated: bool,
    truncated_reason: Option<String>,
    examined: usize,
    status: WorkspaceDirectoryListStatusDto,
) -> CoreResponse {
    CoreResponse::WorkspaceDirectoryList {
        result: WorkspaceDirectoryListResultDto {
            workspace_id,
            prefix,
            nodes,
            truncated,
            truncated_reason,
            examined,
            status,
        },
    }
}

fn combined_wire_reason(walker_reason: Option<&str>, max_bytes: usize) -> String {
    let wire = format!("listing trimmed to fit the {max_bytes}-byte response budget");
    match walker_reason {
        Some(walk) => format!("{walk}; {wire}"),
        None => wire,
    }
}

/// C006 (F2): enforce the wire cap on the **full serialized
/// `CoreResponse::WorkspaceDirectoryList**, not the nodes vector alone.
///
/// Policy (ADR-0022 frozen): trim trailing whole top-level nodes,
/// preserving prefix order, all the way to zero retained roots if even
/// the last nested root cannot fit. A retained prefix always carries
/// `truncated=true` with a bounded combined reason (walker + wire
/// notices). Never keeps one oversized root "because one root always
/// fits". Recursive child pruning requires an ADR amendment and is
/// refused here.
///
/// Cost: one full serialization to test the untrimmed response, then a
/// monotone binary search for the longest fitting top-level prefix
/// (`O(log n)` full serializations, never `O(n)` pops each
/// re-serializing an almost-entire 4096-entry tree). All candidates
/// below `n` share the same combined reason, so fitted size is monotone
/// in the retained count.
///
/// If even an empty `Ready` (with its truncation reason) cannot fit —
/// pathological metadata only, since ids/prefix/reason are bounded —
/// fails closed to `Unavailable` (tiny), and finally to a bounded
/// `Error` that echoes no ids, rather than an oversize payload or panic.
/// `Busy`/`Unavailable` shapes are tiny and always fit for validated ids.
fn build_bounded_listing_response(
    workspace_id: String,
    prefix: String,
    listing: crate::directory_listing::DirectoryListing,
) -> CoreResponse {
    let (response, _) = build_bounded_listing_response_counted(workspace_id, prefix, listing);
    response
}

/// Same as [`build_bounded_listing_response`] plus the number of full
/// `CoreResponse` serializations performed. Exposed so tests can pin the
/// `O(log n)` bound and catch an `O(n^2)` reserialization regression.
pub fn build_bounded_listing_response_counted(
    workspace_id: String,
    prefix: String,
    listing: crate::directory_listing::DirectoryListing,
) -> (CoreResponse, usize) {
    let max_bytes = MAX_DIRECTORY_LISTING_RESPONSE_BYTES;
    let mut serializations = 0usize;
    let measure = |response: &CoreResponse, count: &mut usize| {
        *count += 1;
        serialized_listing_response_len(response)
    };

    // Fast path: the untrimmed response already fits, walker reason only.
    let full = listing_response(
        workspace_id.clone(),
        prefix.clone(),
        listing.nodes.clone(),
        listing.truncated,
        listing.truncated_reason.clone(),
        listing.examined,
        WorkspaceDirectoryListStatusDto::Ready,
    );
    if measure(&full, &mut serializations) <= max_bytes {
        return (full, serializations);
    }

    let total = listing.nodes.len();
    let combined = combined_wire_reason(listing.truncated_reason.as_deref(), max_bytes);
    let candidate = |kept: usize| {
        listing_response(
            workspace_id.clone(),
            prefix.clone(),
            listing.nodes[..kept].to_vec(),
            true,
            Some(combined.clone()),
            listing.examined,
            WorkspaceDirectoryListStatusDto::Ready,
        )
    };

    // Empty `Ready` with the combined reason is the smallest trimmable
    // shape. If it cannot fit, the ids/metadata themselves are
    // pathological: fail closed, never emit oversize bytes.
    if measure(&candidate(0), &mut serializations) > max_bytes {
        let unavailable = listing_response(
            workspace_id.clone(),
            prefix.clone(),
            Vec::new(),
            false,
            None,
            0,
            WorkspaceDirectoryListStatusDto::Unavailable,
        );
        if measure(&unavailable, &mut serializations) <= max_bytes {
            return (unavailable, serializations);
        }
        return (
            doc_error(
                "document_resource_limit",
                "directory listing exceeds the response budget",
            ),
            serializations,
        );
    }

    // Binary search the longest fitting top-level prefix in `[0, total)`.
    // `total` with the combined reason is strictly larger than the
    // already-rejected full shape (same nodes plus the wire notice, or
    // one fewer node replaced by a longer reason), so it is a valid
    // non-fitting upper bound and every candidate in range shares the
    // same reason, hence monotone size in `kept`.
    let mut fitting = 0usize;
    let mut non_fitting = total;
    while fitting + 1 < non_fitting {
        let mid = fitting + (non_fitting - fitting) / 2;
        if measure(&candidate(mid), &mut serializations) <= max_bytes {
            fitting = mid;
        } else {
            non_fitting = mid;
        }
    }
    (candidate(fitting), serializations)
}

fn document_error(error: crate::document_service::DocumentServiceError) -> CoreResponse {
    let code = match error {
        crate::document_service::DocumentServiceError::NotFound => "document_not_found",
        crate::document_service::DocumentServiceError::InvalidPath => "document_invalid_path",
        crate::document_service::DocumentServiceError::InvalidText => "document_invalid_text",
        crate::document_service::DocumentServiceError::ResourceLimit => "document_resource_limit",
        crate::document_service::DocumentServiceError::WriterBusy => "document_writer_busy",
        crate::document_service::DocumentServiceError::StaleLease => "document_stale_lease",
        crate::document_service::DocumentServiceError::StaleRevision => "document_stale_revision",
        crate::document_service::DocumentServiceError::ChangeCollision => {
            "document_change_id_collision"
        }
        crate::document_service::DocumentServiceError::SaveNotReady => "document_save_not_ready",
        crate::document_service::DocumentServiceError::LockPoisoned => "document_lock_failed",
        crate::document_service::DocumentServiceError::Text(_) => "document_invalid_transaction",
        crate::document_service::DocumentServiceError::Io(_) => "document_io_failed",
    };
    doc_error(code, error.to_string())
}

fn doc_error(code: &str, message: impl Into<String>) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_owned(),
        message: message.into(),
    }
}

/// Global-agent document operations.
///
/// A separate `impl` block from the workspace handlers above, because these
/// are a genuinely different authority rather than a variant of one: a
/// different root, a different lock, a stricter filename rule, and none of
/// the workspace-coupled side effects (no managed LSP document, no
/// external-disk refresh, no workspace service).
impl CoreDaemon {
    /// Attach an existing global agent config read-only.
    ///
    /// The root must already exist: opening is a read, and materializing a
    /// directory as a side effect of a read would make a typo look like
    /// configuration.
    async fn open_global_agent(
        &self,
        project_id: &str,
        workspace_id: &str,
        relative_path: &str,
        client_id: &str,
    ) -> CoreResponse {
        let filename = match global_agent::validate(relative_path) {
            Ok(filename) => filename,
            Err(message) => return doc_error("document_invalid_path", message),
        };
        let root = match global_agent::existing_root().await {
            Ok(root) => root,
            Err(message) => return doc_error("document_not_found", message),
        };
        match self
            .documents
            .open(
                project_id.to_owned(),
                workspace_id.to_owned(),
                &root,
                &filename,
                client_id,
                false,
                DocumentScope::GlobalAgent,
            )
            .await
        {
            Ok((snapshot, writer_lease)) => CoreResponse::DocumentSnapshot {
                snapshot,
                writer_lease,
                // A global agent config is not a workspace document: it has
                // no workspace root to resolve against, and pushing it into
                // the LSP would both fail and misrepresent it as project code.
                lsp_degraded: false,
            },
            Err(error) => document_error(error),
        }
    }

    /// Create one new global agent config and attach it writable.
    async fn create_global_agent(
        &self,
        project_id: &str,
        workspace_id: &str,
        relative_path: &str,
        initial_text: &str,
        client_id: &str,
    ) -> CoreResponse {
        let filename = match global_agent::validate(relative_path) {
            Ok(filename) => filename,
            Err(message) => return doc_error("document_invalid_path", message),
        };
        let root = match global_agent::ensure_root().await {
            Ok(root) => root,
            Err(message) => return doc_error("document_io_failed", message),
        };
        let _gate = global_agent::write_gate().lock().await;
        match crate::lsp::mutation::checked_workspace_file_create(&root, &filename, initial_text)
            .await
        {
            crate::lsp::mutation::CheckedFileCreate::AlreadyExists => {
                return doc_error(
                    "document_already_exists",
                    "a global agent already uses that name — open it to edit",
                )
            }
            crate::lsp::mutation::CheckedFileCreate::Rejected(message) => {
                return doc_error("document_invalid_path", message)
            }
            crate::lsp::mutation::CheckedFileCreate::Created { .. } => {}
        }
        // Same shape as the workspace create: materialize the bytes, then
        // attach through the ordinary open path so every containment check
        // that always applied still applies.
        match self
            .documents
            .open(
                project_id.to_owned(),
                workspace_id.to_owned(),
                &root,
                &filename,
                client_id,
                true,
                DocumentScope::GlobalAgent,
            )
            .await
        {
            Ok((snapshot, writer_lease)) => CoreResponse::DocumentSnapshot {
                snapshot,
                writer_lease,
                lsp_degraded: false,
            },
            Err(error) => document_error(error),
        }
    }

    /// Remove one global agent config.
    ///
    /// Refuses a live attachment under the *global* scope only: an open
    /// workspace file that happens to share the name is unrelated and must
    /// not block this.
    async fn delete_global_agent(&self, project_id: &str, relative_path: &str) -> CoreResponse {
        let filename = match global_agent::validate(relative_path) {
            Ok(filename) => filename,
            Err(message) => return doc_error("document_invalid_path", message),
        };
        if self
            .documents
            .is_path_attached(project_id, &filename, DocumentScope::GlobalAgent)
            .await
        {
            return doc_error(
                "document_in_use",
                "this global agent is open in the editor — close it before deleting",
            );
        }
        let root = match global_agent::existing_root().await {
            Ok(root) => root,
            Err(message) => return doc_error("document_not_found", message),
        };
        let _gate = global_agent::write_gate().lock().await;
        match crate::lsp::mutation::checked_workspace_file_delete(&root, &filename).await {
            crate::lsp::mutation::CheckedFileDelete::Deleted => CoreResponse::DocumentDeleted {
                relative_path: filename,
                existed: true,
                scope: DocumentScope::GlobalAgent,
            },
            crate::lsp::mutation::CheckedFileDelete::Missing => CoreResponse::DocumentDeleted {
                relative_path: filename,
                existed: false,
                scope: DocumentScope::GlobalAgent,
            },
            crate::lsp::mutation::CheckedFileDelete::Rejected(message) => {
                doc_error("document_invalid_path", message)
            }
        }
    }

    /// Write a global agent document's buffer to its authority root.
    async fn save_global_agent(
        &self,
        save: &crate::document_service::DocumentSaveSnapshot,
    ) -> CoreResponse {
        let Ok(filename) = global_agent::validate(&save.relative_path) else {
            return doc_error(
                "document_invalid_path",
                "a global agent document must be a single .toml file",
            );
        };
        let root = match global_agent::existing_root().await {
            Ok(root) => root,
            Err(message) => return doc_error("document_save_failed", message),
        };
        let _gate = global_agent::write_gate().lock().await;
        let result = crate::lsp::mutation::checked_workspace_text_write(
            &root,
            &filename,
            &save.expected_disk_digest,
            &save.text,
        )
        .await;
        match result {
            Ok(crate::lsp::mutation::CheckedTextWrite::Conflict { actual_hash }) => {
                let _ = self
                    .documents
                    .mark_disk_conflict(&save.document_id, &save.project_id, &actual_hash)
                    .await;
                doc_error(
                    "document_disk_conflict",
                    format!("disk content changed (sha256 {actual_hash})"),
                )
            }
            Err(error) => doc_error("document_save_failed", error),
            Ok(crate::lsp::mutation::CheckedTextWrite::Written { new_hash }) => {
                let _ = self
                    .documents
                    .commit_save(
                        &save.document_id,
                        &save.project_id,
                        save.revision,
                        new_hash.clone(),
                    )
                    .await;
                // Published like any other file mutation so file-watching
                // subscribers observe the write, but never synced to the LSP:
                // a global agent config is not a workspace document.
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::FileChanged {
                        path: save.path.display().to_string(),
                        action: "Modified".to_string(),
                        old_content: None,
                    },
                );
                CoreResponse::DocumentSaved {
                    revision: save.revision,
                    disk_base_digest: new_hash,
                    lsp_degraded: false,
                }
            }
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::document::{WorkspaceDirectoryNodeDto, WorkspaceDirectoryNodeKindDto};

    fn node(name: &str, path: &str) -> WorkspaceDirectoryNodeDto {
        WorkspaceDirectoryNodeDto {
            name: name.to_owned(),
            path: path.to_owned(),
            depth: 1,
            kind: WorkspaceDirectoryNodeKindDto::File,
            children: None,
        }
    }

    fn result_of(response: &CoreResponse) -> WorkspaceDirectoryListResultDto {
        match response {
            CoreResponse::WorkspaceDirectoryList { result } => result.clone(),
            other => panic!("expected a directory listing, got {other:?}"),
        }
    }

    fn listing_with(
        nodes: Vec<WorkspaceDirectoryNodeDto>,
    ) -> crate::directory_listing::DirectoryListing {
        let examined = nodes.len();
        crate::directory_listing::DirectoryListing {
            nodes,
            truncated: false,
            truncated_reason: None,
            examined,
        }
    }

    #[test]
    fn response_budget_keeps_fitting_listings_untouched() {
        let nodes = vec![node("a.txt", "a.txt"), node("b.txt", "b.txt")];
        let response = build_bounded_listing_response(
            "ws-1".to_owned(),
            String::new(),
            listing_with(nodes.clone()),
        );
        let result = result_of(&response);
        assert_eq!(result.nodes, nodes);
        assert!(!result.truncated);
        assert!(result.truncated_reason.is_none());
        assert!(listing_response_fits_budget(&response));
    }

    #[test]
    fn response_budget_trims_trailing_nodes_with_a_reason() {
        // Build three nodes, then size the budget from a real full
        // `CoreResponse` so the trim is forced by response bytes, not by
        // a nodes-only estimate.
        let nodes = vec![
            node("a.txt", "a.txt"),
            node("b.txt", "b.txt"),
            node("c.txt", "c.txt"),
        ];
        let full = build_bounded_listing_response(
            "ws-1".to_owned(),
            String::new(),
            listing_with(nodes.clone()),
        );
        let full_len = serialized_listing_response_len(&full);
        // One-node response length is a lower bound; trimming to it must
        // keep the deterministic leading prefix plus a budget reason.
        let one_response = listing_response(
            "ws-1".to_owned(),
            String::new(),
            nodes[..1].to_vec(),
            true,
            Some(combined_wire_reason(
                None,
                MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
            )),
            3,
            WorkspaceDirectoryListStatusDto::Ready,
        );
        let one_len = serialized_listing_response_len(&one_response);
        assert!(full_len > one_len);
        // Force a trim by feeding an oversized tree: duplicate the nodes
        // until the full response exceeds the budget, then assert the
        // enforcer retains a strict prefix with a reason.
        let mut big = nodes.clone();
        while {
            let probe = build_bounded_listing_response(
                "ws-1".to_owned(),
                String::new(),
                listing_with(big.clone()),
            );
            let result = result_of(&probe);
            !result.truncated
        } {
            big.extend(nodes.clone());
            if big.len() > 10_000 {
                break;
            }
        }
        let response = build_bounded_listing_response(
            "ws-1".to_owned(),
            String::new(),
            listing_with(big.clone()),
        );
        let result = result_of(&response);
        assert!(listing_response_fits_budget(&response));
        if result.nodes.len() < big.len() {
            assert!(result.truncated);
            assert!(
                result
                    .truncated_reason
                    .as_deref()
                    .is_some_and(|r| r.contains("response budget")),
                "reason must name the budget, got {:?}",
                result.truncated_reason
            );
            // Prefix order preserved.
            for (kept, original) in result.nodes.iter().zip(big.iter()) {
                assert_eq!(kept.name, original.name);
            }
        }
    }

    #[test]
    fn symlink_components_are_rejected_without_following_them() {
        let root = tempfile::tempdir().expect("tempdir");
        let real = root.path().join("real");
        std::fs::create_dir_all(&real).expect("mkdir");
        assert!(reject_scan_symlink_components(root.path(), &real).is_ok());
        assert!(reject_scan_symlink_components(root.path(), &root.path().join("missing")).is_ok());
        // Escaping the root is refused even when nothing is a symlink.
        assert!(reject_scan_symlink_components(root.path(), std::path::Path::new("/etc")).is_err());
        #[cfg(unix)]
        {
            let link = root.path().join("linked");
            std::os::unix::fs::symlink(&real, &link).expect("symlink");
            assert!(reject_scan_symlink_components(root.path(), &link).is_err());
            // A symlink nested below a real directory is still caught.
            let nested = real.join("nested-link");
            std::os::unix::fs::symlink(&real, &nested).expect("symlink");
            assert!(reject_scan_symlink_components(root.path(), &nested).is_err());
        }
    }
}
