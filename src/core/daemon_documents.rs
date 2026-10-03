//! Additive document.v1 protocol handler.

use super::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

impl CoreDaemon {
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
                    Ok((snapshot, writer_lease)) => Ok(CoreResponse::DocumentSnapshot {
                        snapshot,
                        writer_lease,
                    }),
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
                    Ok(snapshot) => Ok(CoreResponse::DocumentSnapshot {
                        snapshot,
                        writer_lease: None,
                    }),
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
                    Ok(revision) => Ok(CoreResponse::DocumentChanged { revision }),
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentSave {
                project_id,
                document_id,
                writer_lease,
                ..
            }
            | CoreRequest::DocumentReload {
                project_id,
                document_id,
                writer_lease,
                ..
            } => {
                match self
                    .documents
                    .save_or_reload_not_ready(&document_id, &project_id, client_id, &writer_lease)
                    .await
                {
                    Ok(()) => Ok(CoreResponse::Ack),
                    Err(error) => Ok(document_error(error)),
                }
            }
            CoreRequest::DocumentClose {
                project_id,
                document_id,
            } => {
                self.documents
                    .detach_document(&document_id, &project_id, client_id)
                    .await;
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
