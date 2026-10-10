//! M002: daemon authority for bounded `/init` drafts and guarded publication.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::core::{
    CoreRequest, CoreResponse, ProjectInitDiagnosticDto, ProjectInitDraftDto,
    ProjectInitEvidenceDto, ProjectInitOperationDto, MAX_PROJECT_INIT_DRAFT_BYTES,
};

const MAX_INIT_CONTENT_BYTES: usize = 64 * 1024;
const MAX_TARGET_BYTES: u64 = 64 * 1024;

impl CoreDaemon {
    pub(crate) async fn handle_project_init_request(
        &self,
        request: CoreRequest,
        client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::ProjectInitDraftGet {
                project_id,
                workspace_id,
            } => {
                self.project_init_draft(&project_id, &workspace_id, client_id)
                    .await
            }
            CoreRequest::ProjectInitPublish {
                project_id,
                workspace_id,
                draft_token,
            } => {
                self.project_init_publish(&project_id, &workspace_id, &draft_token, client_id)
                    .await
            }
            _ => Ok(CoreResponse::Error {
                code: "project_init_unimplemented".into(),
                message: "Unsupported repository initialization request".into(),
            }),
        }
    }

    async fn project_init_draft(
        &self,
        project_id: &str,
        workspace_id: &str,
        client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        let (project, workspace, root) = match self
            .resolve_project_workspace(project_id, workspace_id)
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => return Ok(*error),
        };
        let context = crate::agent::bootstrap::ProjectBootstrapContext {
            project_id: project.to_string(),
            workspace_id: workspace.to_string(),
            workspace_root: root,
        };
        let draft = match crate::agent::bootstrap::analyze_project_context(&context) {
            Ok(draft) => draft,
            Err(error) => return Ok(init_error("project_init_analysis_failed", error)),
        };

        let target = context.workspace_root.join("AGENTS.md");
        let old_content =
            match read_observed_target(&target, draft.observed_target_digest.as_deref()) {
                Ok(content) => content.unwrap_or_default(),
                Err(error) => return Ok(init_error("project_init_stale", error)),
            };
        let unified_diff = render_diff(&old_content, &draft.candidate_markdown);
        if draft.candidate_markdown.len() > MAX_INIT_CONTENT_BYTES
            || draft
                .candidate_markdown
                .len()
                .saturating_add(unified_diff.len())
                > MAX_PROJECT_INIT_DRAFT_BYTES
        {
            return Ok(init_error(
                "project_init_resource_limit",
                "Draft exceeds the preview size limit",
            ));
        }
        let token = if draft.operation == crate::agent::bootstrap::DraftOperation::Noop {
            None
        } else {
            match self.project_init_drafts.issue(
                client_id.to_owned(),
                project.to_string(),
                workspace.to_string(),
                context.workspace_root.clone(),
                draft.observed_target_digest.clone(),
                draft.candidate_markdown.clone(),
            ) {
                Ok(token) => Some(token),
                Err(()) => {
                    return Ok(init_error(
                        "project_init_busy",
                        "Too many unexpired initialization previews; wait and retry",
                    ));
                }
            }
        };
        let response = ProjectInitDraftDto {
            project_id: project.to_string(),
            workspace_id: workspace.to_string(),
            operation: match draft.operation {
                crate::agent::bootstrap::DraftOperation::Create => ProjectInitOperationDto::Create,
                crate::agent::bootstrap::DraftOperation::Update => ProjectInitOperationDto::Update,
                crate::agent::bootstrap::DraftOperation::Noop => ProjectInitOperationDto::Noop,
            },
            draft_token: token,
            target_relative_path: "AGENTS.md".into(),
            observed_target_digest: draft.observed_target_digest,
            candidate_markdown: draft.candidate_markdown,
            unified_diff,
            evidence: draft
                .evidence
                .into_iter()
                .map(|item| ProjectInitEvidenceDto {
                    source: item.source,
                    fact: item.fact,
                    confidence: match item.confidence {
                        crate::agent::bootstrap::EvidenceConfidence::Observed => "observed",
                        crate::agent::bootstrap::EvidenceConfidence::Inferred => "inferred",
                        crate::agent::bootstrap::EvidenceConfidence::Unknown => "unknown",
                    }
                    .into(),
                })
                .collect(),
            diagnostics: draft
                .diagnostics
                .into_iter()
                .map(|item| ProjectInitDiagnosticDto {
                    source: item.source,
                    message: item.message,
                })
                .collect(),
        };
        Ok(CoreResponse::ProjectInitDraft { draft: response })
    }

    async fn project_init_publish(
        &self,
        project_id: &str,
        workspace_id: &str,
        draft_token: &str,
        client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        if draft_token.len() != 32 || !draft_token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(init_error(
                "project_init_draft_invalid",
                "Draft token is invalid or expired",
            ));
        }
        let pending =
            match self
                .project_init_drafts
                .consume(draft_token, client_id, project_id, workspace_id)
            {
                Ok(pending) => pending,
                Err(crate::core::project_init::ConsumeDraftError::MissingOrExpired) => {
                    return Ok(init_error(
                        "project_init_draft_expired",
                        "Draft token is invalid, expired, or already used",
                    ));
                }
                Err(crate::core::project_init::ConsumeDraftError::ScopeMismatch) => {
                    return Ok(init_error(
                        "project_init_draft_scope",
                        "Draft token does not match this project, workspace, or client",
                    ));
                }
            };
        let (project, workspace, root) = match self
            .resolve_project_workspace(project_id, workspace_id)
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => return Ok(*error),
        };
        if root != pending.workspace_root {
            return Ok(init_error(
                "project_init_stale",
                "Selected workspace root changed; regenerate the preview",
            ));
        }
        let _document_guards = match self
            .documents
            .lock_clean_paths(workspace.as_ref(), &["AGENTS.md".into()])
            .await
        {
            Ok(guards) => guards,
            Err(_) => {
                return Ok(init_error(
                    "project_init_editor_conflict",
                    "AGENTS.md has unsaved editor changes; save and regenerate the preview",
                ))
            }
        };
        let Ok(services) = self.workspace_services.acquire(&workspace).await else {
            return Ok(init_error(
                "project_init_workspace_unavailable",
                "Workspace services are unavailable",
            ));
        };
        let _workspace = services.locks().acquire_repository(&root).await;
        let (old_content, new_digest) = match publish_checked(
            &root,
            pending.expected_digest.as_deref(),
            &pending.candidate_markdown,
        ) {
            Ok(written) => written,
            Err(error) => return Ok(init_error("project_init_publish_failed", error)),
        };
        crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::FileChanged {
            path: root.join("AGENTS.md").display().to_string(),
            action: if old_content.is_some() {
                "Modified"
            } else {
                "Created"
            }
            .into(),
            old_content,
        });
        Ok(CoreResponse::ProjectInitPublished {
            project_id: project.to_string(),
            workspace_id: workspace.to_string(),
            target_digest: new_digest,
        })
    }

    async fn resolve_project_workspace(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<
        (
            codegg_core::identity::ProjectId,
            codegg_core::workspace::WorkspaceId,
            PathBuf,
        ),
        Box<CoreResponse>,
    > {
        let project = codegg_core::identity::ProjectId::parse(project_id).map_err(|_| {
            Box::new(init_error(
                "project_not_found",
                "Project or workspace not found",
            ))
        })?;
        let workspace = codegg_core::workspace::WorkspaceId::parse(workspace_id).map_err(|_| {
            Box::new(init_error(
                "project_not_found",
                "Project or workspace not found",
            ))
        })?;
        let Some(pool) = &self.pool else {
            return Err(Box::new(init_error(
                "project_init_unavailable",
                "Repository initialization requires the project catalog",
            )));
        };
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        match catalog.list_workspaces_for_project(&project).await {
            Ok(workspaces)
                if workspaces
                    .iter()
                    .any(|entry| entry.workspace_id == workspace) => {}
            _ => {
                return Err(Box::new(init_error(
                    "project_not_found",
                    "Project or workspace not found",
                )))
            }
        }
        let Some(workspace_record) = self.workspaces.resolve(&workspace).await else {
            return Err(Box::new(init_error(
                "project_not_found",
                "Project or workspace not found",
            )));
        };
        Ok((project, workspace, workspace_record.canonical_root.clone()))
    }
}

fn read_observed_target(
    target: &Path,
    expected_digest: Option<&str>,
) -> Result<Option<String>, String> {
    match fs::symlink_metadata(target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && expected_digest.is_none() => {
            Ok(None)
        }
        Err(_) => Err("AGENTS.md changed after analysis; regenerate the preview".into()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.file_type().is_file() => {
            Err("AGENTS.md is no longer a regular file".into())
        }
        Ok(meta) if meta.len() > MAX_TARGET_BYTES => {
            Err("AGENTS.md exceeds the preview size limit".into())
        }
        Ok(_) => {
            let bytes = fs::read(target).map_err(|_| "AGENTS.md could not be read for preview")?;
            let actual = digest(&bytes);
            if Some(actual.as_str()) != expected_digest {
                return Err("AGENTS.md changed after analysis; regenerate the preview".into());
            }
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "AGENTS.md is not valid UTF-8".into())
        }
    }
}

fn publish_checked(
    root: &Path,
    expected_digest: Option<&str>,
    content: &str,
) -> Result<(Option<String>, String), String> {
    if content.len() > MAX_INIT_CONTENT_BYTES {
        return Err("Draft exceeds the publication size limit".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "Workspace root is unavailable".to_string())?;
    let target = root.join("AGENTS.md");
    let old_content = read_observed_target(&target, expected_digest)?;
    let original_permissions = fs::symlink_metadata(&target)
        .ok()
        .map(|metadata| metadata.permissions());
    let temp = root.join(format!(".codegg-init-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| "Could not create an atomic temporary file".to_string())?;
        file.write_all(content.as_bytes())
            .map_err(|_| "Could not write the initialization draft".to_string())?;
        if let Some(permissions) = &original_permissions {
            file.set_permissions(permissions.clone())
                .map_err(|_| "Could not preserve AGENTS.md permissions".to_string())?;
        }
        file.sync_all()
            .map_err(|_| "Could not sync the initialization draft".to_string())?;
        drop(file);
        if expected_digest.is_some() {
            fs::rename(&temp, &target)
                .map_err(|_| "Could not atomically replace AGENTS.md".to_string())?;
        } else {
            fs::hard_link(&temp, &target).map_err(|_| {
                "AGENTS.md was created concurrently or atomic creation failed".to_string()
            })?;
            let _ = fs::remove_file(&temp);
        }
        #[cfg(unix)]
        let _ = fs::File::open(&root).and_then(|directory| directory.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    Ok((old_content, digest(content.as_bytes())))
}

fn render_diff(old: &str, new: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut rendered = "--- a/AGENTS.md\n+++ b/AGENTS.md\n".to_owned();
    for group in diff.grouped_ops(3) {
        for op in group {
            for change in diff.iter_changes(&op) {
                let prefix = match change.tag() {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };
                rendered.push_str(prefix);
                rendered.push_str(&change.to_string());
                if !change.to_string().ends_with('\n') {
                    rendered.push('\n');
                }
            }
        }
        rendered.push('\n');
    }
    rendered
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn init_error(code: &str, message: impl Into<String>) -> CoreResponse {
    CoreResponse::Error {
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(content: &str) -> String {
        digest(content.as_bytes())
    }

    #[test]
    fn create_and_update_are_guarded_by_exact_digest() {
        let root = tempfile::tempdir().unwrap();
        let created = publish_checked(root.path(), None, "# New\n").unwrap();
        assert!(created.0.is_none());
        assert_eq!(
            fs::read_to_string(root.path().join("AGENTS.md")).unwrap(),
            "# New\n"
        );
        let expected = digest_of("# New\n");
        let updated =
            publish_checked(root.path(), Some(&expected), "# New\n\nHuman notes\n").unwrap();
        assert_eq!(updated.0.as_deref(), Some("# New\n"));
        assert_eq!(
            fs::read_to_string(root.path().join("AGENTS.md")).unwrap(),
            "# New\n\nHuman notes\n"
        );
    }

    #[test]
    fn stale_update_and_symlink_target_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("AGENTS.md"), "external edit\n").unwrap();
        assert!(publish_checked(root.path(), Some(&digest_of("old")), "replacement").is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("AGENTS.md")).unwrap(),
            "external edit\n"
        );
        #[cfg(unix)]
        {
            let outside = tempfile::NamedTempFile::new().unwrap();
            fs::remove_file(root.path().join("AGENTS.md")).unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("AGENTS.md")).unwrap();
            assert!(publish_checked(root.path(), None, "replacement").is_err());
            assert_eq!(outside.path().metadata().unwrap().len(), 0);
        }
    }

    #[test]
    fn concurrent_creates_have_exactly_one_winner() {
        let root = tempfile::tempdir().unwrap();
        let first_root = root.path().to_path_buf();
        let second_root = root.path().to_path_buf();
        let first = std::thread::spawn(move || publish_checked(&first_root, None, "first"));
        let second = std::thread::spawn(move || publish_checked(&second_root, None, "second"));
        let successes = usize::from(first.join().unwrap().is_ok())
            + usize::from(second.join().unwrap().is_ok());
        assert_eq!(successes, 1);
    }
}
