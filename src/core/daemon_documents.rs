//! Additive document.v1 protocol handler.

use super::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

impl CoreDaemon {
    async fn sync_managed_document(
        &self,
        snapshot: &codegg_protocol::document::DocumentSnapshotDto,
    ) -> bool {
        let Some(service) = &self.deps.lsp_service else {
            return false;
        };
        let Ok(workspace_id) = codegg_core::workspace::WorkspaceId::parse(&snapshot.workspace_id)
        else {
            return true;
        };
        let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
            return true;
        };
        let path = workspace.canonical_root.join(&snapshot.relative_path);
        service
            .set_managed_document(&path, &snapshot.text, snapshot.dirty)
            .await
            .is_err()
    }

    async fn refresh_external_disk(
        &self,
        mut snapshot: codegg_protocol::document::DocumentSnapshotDto,
        client_id: &str,
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
        let _guard = services
            .locks()
            .acquire_repository(&workspace.canonical_root)
            .await;
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
            } => {
                let workspace_id = match codegg_core::workspace::WorkspaceId::parse(&workspace_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(doc_error("document_invalid_workspace", error.to_string()))
                    }
                };
                let Some(workspace) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(doc_error("document_not_found", "workspace not found"));
                };
                if let Some(pool) = &self.pool {
                    let Ok(project_id_parsed) =
                        codegg_core::identity::ProjectId::parse(&project_id)
                    else {
                        return Ok(doc_error("document_not_found", "project not found"));
                    };
                    let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
                    match catalog
                        .list_workspaces_for_project(&project_id_parsed)
                        .await
                    {
                        Ok(workspaces)
                            if workspaces.iter().any(|ws| ws.workspace_id == workspace_id) => {}
                        _ => {
                            return Ok(doc_error(
                                "document_not_found",
                                "project or workspace not found",
                            ))
                        }
                    }
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
                    )
                    .await
                {
                    Ok((snapshot, writer_lease)) => {
                        let snapshot = self.refresh_external_disk(snapshot, client_id).await;
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
                        let snapshot = self.refresh_external_disk(snapshot, client_id).await;
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
            _ => Ok(doc_error(
                "document_unimplemented",
                "unsupported document request",
            )),
        }
    }
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

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
