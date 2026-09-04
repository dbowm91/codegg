//! Daemon-owned application of reviewed LSP workspace edits.
//!
//! LSP servers only propose edits.  This module is the mutation boundary:
//! it revalidates the reviewed digest and file hashes, holds the canonical
//! workspace lock, records a checked edit checkpoint, publishes file-change
//! events, and finally synchronizes open LSP documents.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codegg_core::snapshot::checkpoint::{EditCheckpoint, EditFileState, FileState};
use codegg_core::workspace_services::WorkspaceLockTable;
use codegg_protocol::lsp::{LspPreviewApplyRequestDto, LspPreviewApplyResultDto};

const MAX_PATCHES: usize = 100;
const MAX_PATCH_BYTES: usize = 50_000;
const MAX_STRING_BYTES: usize = 4_096;

#[derive(Debug, thiserror::Error)]
pub enum LspMutationApplyError {
    #[error("invalid LSP preview: {0}")]
    Invalid(String),
    #[error("LSP preview is stale: {0}")]
    Stale(String),
    #[error("LSP preview apply failed: {0}")]
    Apply(String),
}

/// Apply a reviewed LSP preview under the daemon's workspace authority.
pub async fn apply_preview(
    request: LspPreviewApplyRequestDto,
    workspace_root: PathBuf,
    locks: Arc<WorkspaceLockTable>,
    pool: sqlx::SqlitePool,
    lsp_service: Option<Arc<crate::lsp::service::LspService>>,
) -> Result<LspPreviewApplyResultDto, LspMutationApplyError> {
    validate_request_shape(&request)?;

    let canonical_root = workspace_root
        .canonicalize()
        .map_err(|e| LspMutationApplyError::Invalid(format!("workspace root: {e}")))?;

    let mut normalized = Vec::with_capacity(request.patches.len());
    let mut relative_paths = Vec::with_capacity(request.patches.len());
    let mut seen = HashSet::new();
    for patch in &request.patches {
        let path = crate::tool::util::validate_path(Path::new(&patch.path), &canonical_root)
            .map_err(|e| LspMutationApplyError::Invalid(e.to_string()))?;
        let relative = path
            .strip_prefix(&canonical_root)
            .map_err(|_| LspMutationApplyError::Invalid("path is outside workspace".into()))?
            .to_string_lossy()
            .into_owned();
        if !seen.insert(relative.clone()) {
            return Err(LspMutationApplyError::Invalid(format!(
                "duplicate affected path: {relative}"
            )));
        }
        relative_paths.push(relative);
        normalized.push((path, patch));
    }

    let patch_views: Vec<egglsp::context::PreviewFilePatch> = request
        .patches
        .iter()
        .map(|patch| egglsp::context::PreviewFilePatch {
            path: patch.path.clone(),
            patch: patch.patch.clone(),
            original_hash: patch.original_hash.clone(),
        })
        .collect();
    let hashes: HashMap<String, String> = request
        .patches
        .iter()
        .map(|patch| (patch.path.clone(), patch.original_hash.clone()))
        .collect();
    let expected_digest = egglsp::preview_registry::preview_digest_for_candidate(
        &request.kind,
        &request.title,
        &request.provenance,
        &request
            .patches
            .iter()
            .map(|p| p.path.clone())
            .collect::<Vec<_>>(),
        &hashes,
        &patch_views,
    );
    if expected_digest != request.preview_digest {
        return Err(LspMutationApplyError::Invalid(
            "preview digest does not match the reviewed candidate".into(),
        ));
    }

    let _guard = locks.acquire_repository(&canonical_root).await;
    let manager =
        codegg_core::snapshot::checkpoint::EditCheckpointManager::new(pool, canonical_root.clone());
    let pre_states = manager
        .capture_states(&relative_paths)
        .await
        .map_err(LspMutationApplyError::Apply)?;

    let mut planned = Vec::with_capacity(normalized.len());
    for ((path, patch), relative) in normalized.iter().zip(&relative_paths) {
        let pre = pre_states.get(relative).ok_or_else(|| {
            LspMutationApplyError::Stale(format!("missing pre-state for {relative}"))
        })?;
        let FileState::Present { hash, content } = pre else {
            return Err(LspMutationApplyError::Stale(format!(
                "affected file is not present: {relative}"
            )));
        };
        if hash != &patch.original_hash {
            return Err(LspMutationApplyError::Stale(format!(
                "{relative} changed since preview (expected {}, got {hash})",
                patch.original_hash
            )));
        }
        let new_content = crate::tool::patch_util::apply_unified_diff(content, &patch.patch)
            .map_err(|e| LspMutationApplyError::Apply(format!("{relative}: {e}")))?;
        planned.push((path.clone(), relative.clone(), content.clone(), new_content));
    }

    for (path, relative, _, new_content) in &planned {
        if let Err(error) = write_atomic(path, new_content) {
            rollback_files(&planned);
            return Err(LspMutationApplyError::Apply(format!(
                "{relative}: {error}; prior writes were rolled back"
            )));
        }
    }

    let post_states = match manager.capture_states(&relative_paths).await {
        Ok(states) => states,
        Err(error) => {
            rollback_files(&planned);
            return Err(LspMutationApplyError::Apply(format!(
                "post-state capture failed: {error}; writes were rolled back"
            )));
        }
    };
    let files = planned
        .iter()
        .map(|(_, relative, old_content, new_content)| {
            let pre = FileState::Present {
                hash: sha256(old_content),
                content: old_content.clone(),
            };
            let post = post_states.get(relative).cloned().ok_or_else(|| {
                LspMutationApplyError::Apply(format!("missing post-state for {relative}"))
            })?;
            let expected_post_hash = sha256(new_content);
            if post.hash() != Some(expected_post_hash.as_str()) {
                return Err(LspMutationApplyError::Apply(format!(
                    "post-state mismatch for {relative}; writes were rolled back"
                )));
            }
            Ok(EditFileState {
                path: relative.clone(),
                pre,
                post,
            })
        })
        .collect::<Result<Vec<_>, _>>();
    let files = match files {
        Ok(files) => files,
        Err(error) => {
            rollback_files(&planned);
            return Err(error);
        }
    };

    let checkpoint_id = uuid::Uuid::new_v4().to_string();
    let checkpoint = EditCheckpoint {
        id: checkpoint_id.clone(),
        workspace_id: request.workspace_id.clone(),
        session_id: request.session_id.clone(),
        turn_id: request.turn_id.clone(),
        batch_seq: chrono::Utc::now().timestamp_millis(),
        created_at: chrono::Utc::now().timestamp_millis(),
        files,
    };
    if let Err(error) = manager.persist_checkpoint(checkpoint).await {
        rollback_files(&planned);
        return Err(LspMutationApplyError::Apply(format!(
            "checkpoint persistence failed: {error}; writes were rolled back"
        )));
    }

    for (path, _, old_content, _) in &planned {
        crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::FileChanged {
            path: path.display().to_string(),
            action: "Modified".to_string(),
            old_content: Some(old_content.clone()),
        });
    }

    let mut synchronization_errors = Vec::new();
    if let Some(service) = lsp_service {
        for (path, _, _, new_content) in &planned {
            if let Err(error) = service.update_file(path, new_content).await {
                synchronization_errors.push(format!("{}: {error}", path.display()));
            }
        }
    }

    Ok(LspPreviewApplyResultDto {
        preview_id: request.preview_id,
        preview_revision: request.preview_revision,
        preview_digest: request.preview_digest,
        kind: request.kind,
        title: request.title,
        written_files: planned
            .iter()
            .map(|(path, _, _, _)| path.display().to_string())
            .collect(),
        checkpoint_id,
        synchronization_errors,
    })
}

fn validate_request_shape(
    request: &LspPreviewApplyRequestDto,
) -> Result<(), LspMutationApplyError> {
    if request.preview_id.is_empty()
        || request.preview_id.len() > MAX_STRING_BYTES
        || request.preview_digest.len() != 64
        || request.preview_revision == 0
        || request.workspace_id.is_empty()
        || request.session_id.is_empty()
    {
        return Err(LspMutationApplyError::Invalid(
            "missing or malformed preview identity".into(),
        ));
    }
    if !matches!(
        request.kind.as_str(),
        "rename" | "code_action" | "formatting"
    ) {
        return Err(LspMutationApplyError::Invalid(format!(
            "unsupported preview kind: {}",
            request.kind
        )));
    }
    if request.patches.is_empty() || request.patches.len() > MAX_PATCHES {
        return Err(LspMutationApplyError::Invalid(format!(
            "preview must contain 1..={MAX_PATCHES} text patches"
        )));
    }
    if request.title.len() > MAX_STRING_BYTES || request.provenance.len() > MAX_STRING_BYTES {
        return Err(LspMutationApplyError::Invalid(
            "preview metadata exceeds bounds".into(),
        ));
    }
    for patch in &request.patches {
        if patch.path.is_empty()
            || patch.path.len() > MAX_STRING_BYTES
            || patch.patch.len() > MAX_PATCH_BYTES
            || patch.original_hash.len() != 64
        {
            return Err(LspMutationApplyError::Invalid(
                "malformed or oversized text patch".into(),
            ));
        }
    }
    Ok(())
}

fn sha256(content: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension(format!("codegg-lsp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn rollback_files(planned: &[(PathBuf, String, String, String)]) {
    for (path, _, old_content, _) in planned.iter().rev() {
        let _ = write_atomic(path, old_content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::session::schema::migrate;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        migrate(&pool).await.expect("migrate");
        pool
    }

    fn request(root: &Path, path: &Path, original: &str) -> LspPreviewApplyRequestDto {
        let preview = egglsp::edit::preview_text_edits_for_file(
            "rename",
            path,
            vec![egglsp::lsp_types::TextEdit {
                range: egglsp::lsp_types::Range {
                    start: egglsp::lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                    end: egglsp::lsp_types::Position {
                        line: 0,
                        character: 3,
                    },
                },
                new_text: "new".into(),
            }],
            Some(root),
        )
        .expect("preview");
        let file = preview.files.into_iter().next().expect("file preview");
        let patches = vec![egglsp::context::PreviewFilePatch {
            path: path.display().to_string(),
            patch: file.patch,
            original_hash: file.original_hash,
        }];
        let hashes = HashMap::from([(path.display().to_string(), sha256(original))]);
        let paths = vec![path.display().to_string()];
        let digest = egglsp::preview_registry::preview_digest_for_candidate(
            "rename",
            "rename",
            "rust-analyzer:textDocument/rename",
            &paths,
            &hashes,
            &patches,
        );
        LspPreviewApplyRequestDto {
            preview_id: "preview-test".into(),
            preview_revision: 1,
            preview_digest: digest,
            kind: "rename".into(),
            title: "rename".into(),
            provenance: "rust-analyzer:textDocument/rename".into(),
            workspace_id: "workspace-test".into(),
            session_id: "session-test".into(),
            turn_id: Some("turn-test".into()),
            patches: patches
                .into_iter()
                .map(|p| codegg_protocol::lsp::LspPreviewPatchDto {
                    path: p.path,
                    patch: p.patch,
                    original_hash: p.original_hash,
                })
                .collect(),
        }
    }

    fn refresh_digest(request: &mut LspPreviewApplyRequestDto) {
        let patches: Vec<egglsp::context::PreviewFilePatch> = request
            .patches
            .iter()
            .map(|patch| egglsp::context::PreviewFilePatch {
                path: patch.path.clone(),
                patch: patch.patch.clone(),
                original_hash: patch.original_hash.clone(),
            })
            .collect();
        let paths = request
            .patches
            .iter()
            .map(|patch| patch.path.clone())
            .collect::<Vec<_>>();
        let hashes = request
            .patches
            .iter()
            .map(|patch| (patch.path.clone(), patch.original_hash.clone()))
            .collect::<HashMap<_, _>>();
        request.preview_digest = egglsp::preview_registry::preview_digest_for_candidate(
            &request.kind,
            &request.title,
            &request.provenance,
            &paths,
            &hashes,
            &patches,
        );
    }

    #[tokio::test]
    async fn applies_preview_records_checkpoint_and_supports_checked_undo() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("main.rs");
        let original = "old\n";
        std::fs::write(&path, original).expect("write original");
        let request = request(root.path(), &path, original);
        let pool = pool().await;
        let result = apply_preview(
            request,
            root.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
            pool.clone(),
            None,
        )
        .await
        .expect("apply");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        let manager = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
            pool,
            root.path().to_path_buf(),
        );
        let checkpoint = manager
            .get(&result.checkpoint_id)
            .await
            .expect("get checkpoint")
            .expect("checkpoint exists");
        assert_eq!(checkpoint.workspace_id, "workspace-test");
        assert_eq!(checkpoint.session_id, "session-test");
        let undo = manager
            .checked_undo(
                &result.checkpoint_id,
                "workspace-test",
                Some("session-test"),
            )
            .await
            .expect("undo");
        assert!(undo.is_applied());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[tokio::test]
    async fn stale_preview_is_rejected_before_any_write() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("main.rs");
        let original = "old\n";
        std::fs::write(&path, original).expect("write original");
        let request = request(root.path(), &path, original);
        std::fs::write(&path, "changed\n").expect("change file");
        let error = apply_preview(
            request,
            root.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
            pool().await,
            None,
        )
        .await
        .expect_err("stale preview must fail closed");
        assert!(matches!(error, LspMutationApplyError::Stale(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed\n");
    }

    #[tokio::test]
    async fn edit_only_code_action_uses_the_same_checked_apply_path() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("main.rs");
        std::fs::write(&path, "old\n").expect("write original");
        let mut request = request(root.path(), &path, "old\n");
        request.kind = "code_action".into();
        request.title = "Remove unused import".into();
        request.provenance = "rust-analyzer:textDocument/codeAction".into();
        refresh_digest(&mut request);

        let result = apply_preview(
            request,
            root.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
            pool().await,
            None,
        )
        .await
        .expect("apply code action edit");

        assert_eq!(result.kind, "code_action");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    }

    #[tokio::test]
    async fn cross_workspace_preview_is_rejected_before_write() {
        let root = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let path = root.path().join("main.rs");
        let outside_path = outside.path().join("outside.rs");
        std::fs::write(&path, "old\n").expect("write original");
        std::fs::write(&outside_path, "outside\n").expect("write outside");
        let mut request = request(root.path(), &path, "old\n");
        request.patches[0].path = outside_path.display().to_string();
        refresh_digest(&mut request);

        let error = apply_preview(
            request,
            root.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
            pool().await,
            None,
        )
        .await
        .expect_err("cross-workspace preview must fail closed");
        assert!(matches!(error, LspMutationApplyError::Invalid(_)));
        assert_eq!(std::fs::read_to_string(&outside_path).unwrap(), "outside\n");
    }
}
