//! Native model-facing adapter for checked LSP preview application.
//!
//! The model supplies only an opaque preview identifier.  Every revision,
//! digest, patch, path, provenance, and execution identity is resolved from
//! the turn-local host-owned state before the canonical checked mutation
//! service is called.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use codegg_protocol::lsp::{LspPreviewApplyRequestDto, LspPreviewApplyResultDto};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::ToolError;
use crate::lsp::mutation::LspMutationApplyError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance};
use crate::tool::contract::{
    IdempotencyClass, ToolCallerPolicy, ToolContract, ToolEffectClass, ToolRetryPolicy,
};
use crate::tool::{LspPreviewRegistryHandle, Tool, ToolCategory, ToolTrust};

const MAX_PREVIEW_ID_BYTES: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LspPreviewApplyInput {
    preview_id: String,
}

#[derive(Debug, Serialize)]
struct LspPreviewApplyOutput {
    operation: &'static str,
    result: LspPreviewApplyResultDto,
}

/// The narrow native tool that applies one reviewed, still-current LSP
/// preview through the daemon-owned checked mutation service.
pub struct LspPreviewApplyTool {
    pool: sqlx::SqlitePool,
    workspace_root: PathBuf,
    workspace_id: String,
    session_id: String,
    turn_id: Option<String>,
    workspace_locks: Arc<codegg_core::workspace_services::WorkspaceLockTable>,
    lsp_service: Arc<crate::lsp::service::LspService>,
    preview_registry: LspPreviewRegistryHandle,
    #[cfg(test)]
    apply_barrier: Option<Arc<tokio::sync::Barrier>>,
}

impl LspPreviewApplyTool {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: sqlx::SqlitePool,
        workspace_root: PathBuf,
        workspace_id: String,
        session_id: String,
        turn_id: Option<String>,
        workspace_locks: Arc<codegg_core::workspace_services::WorkspaceLockTable>,
        lsp_service: Arc<crate::lsp::service::LspService>,
        preview_registry: LspPreviewRegistryHandle,
    ) -> Self {
        Self {
            pool,
            workspace_root,
            workspace_id,
            session_id,
            turn_id,
            workspace_locks,
            lsp_service,
            preview_registry,
            #[cfg(test)]
            apply_barrier: None,
        }
    }

    #[cfg(test)]
    fn with_apply_barrier(mut self, barrier: Arc<tokio::sync::Barrier>) -> Self {
        self.apply_barrier = Some(barrier);
        self
    }

    pub(crate) fn parameters_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "preview_id": {
                    "type": "string",
                    "description": "Opaque LSP preview identifier returned by a previous preview operation"
                }
            },
            "required": ["preview_id"],
            "additionalProperties": false
        })
    }

    fn output_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {"const": "lsp_preview_apply"},
                "result": {
                    "type": "object",
                    "properties": {
                        "preview_id": {"type": "string"},
                        "preview_revision": {"type": "integer", "minimum": 0},
                        "preview_digest": {"type": "string"},
                        "kind": {"type": "string", "enum": ["rename", "formatting", "code_action"]},
                        "title": {"type": "string"},
                        "written_files": {"type": "array", "items": {"type": "string"}},
                        "checkpoint_id": {"type": "string"},
                        "synchronization_errors": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["preview_id", "preview_revision", "preview_digest", "kind", "title", "written_files", "checkpoint_id", "synchronization_errors"],
                    "additionalProperties": false
                }
            },
            "required": ["operation", "result"],
            "additionalProperties": false
        })
    }

    fn parse_input(input: serde_json::Value) -> Result<LspPreviewApplyInput, ToolError> {
        let parsed: LspPreviewApplyInput = serde_json::from_value(input).map_err(|error| {
            ToolError::Execution(format!("invalid lsp_preview_apply input: {error}"))
        })?;
        if parsed.preview_id.is_empty() || parsed.preview_id.len() > MAX_PREVIEW_ID_BYTES {
            return Err(ToolError::Execution(
                "preview_id must be non-empty and at most 256 bytes".to_string(),
            ));
        }
        Ok(parsed)
    }

    fn validate_execution_context(
        &self,
        context: Option<&ToolExecutionContext>,
    ) -> Result<(), ToolError> {
        let Some(context) = context else {
            return Ok(());
        };
        if let Some(session_id) = context.session_id.as_deref() {
            if session_id != self.session_id {
                return Err(ToolError::Execution(
                    "lsp preview belongs to a different session".to_string(),
                ));
            }
        }
        if let (Some(expected), Some(actual)) =
            (self.turn_id.as_deref(), context.turn_id.as_deref())
        {
            if expected != actual {
                return Err(ToolError::Execution(
                    "lsp preview belongs to a different turn".to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn apply(
        &self,
        input: serde_json::Value,
        context: Option<&ToolExecutionContext>,
    ) -> Result<LspPreviewApplyResultDto, ToolError> {
        self.validate_execution_context(context)?;
        let input = Self::parse_input(input)?;
        let candidate = {
            let registry = self.preview_registry.lock();
            egglsp::tui_summary::export_preview_apply_candidate(&registry, &input.preview_id)
        }
        .ok_or_else(|| {
            ToolError::Execution("LSP preview was not found or has expired".to_string())
        })?;

        #[cfg(test)]
        if let Some(barrier) = &self.apply_barrier {
            barrier.wait().await;
        }

        if candidate.applied {
            return Err(ToolError::Execution(
                "LSP preview has already been applied".to_string(),
            ));
        }
        if candidate.stale_base {
            return Err(ToolError::Execution(
                "LSP preview is stale and must be regenerated".to_string(),
            ));
        }
        if candidate.patches.is_empty() {
            return Err(ToolError::Execution(
                "LSP preview contains no applicable file patches".to_string(),
            ));
        }
        if !matches!(
            candidate.kind.as_str(),
            "rename" | "formatting" | "code_action"
        ) {
            return Err(ToolError::Execution(
                "LSP preview kind is not supported by checked application".to_string(),
            ));
        }

        crate::lsp::mutation::validate_session_workspace_binding(
            &self.pool,
            &self.session_id,
            &self.workspace_id,
        )
        .await
        .map_err(|error| ToolError::Execution(format!("LSP preview scope rejected: {error}")))?;

        let request = candidate_to_request(
            candidate,
            self.workspace_id.clone(),
            self.session_id.clone(),
            self.turn_id.clone(),
        )?;
        let preview_id = request.preview_id.clone();
        let result = crate::lsp::mutation::apply_preview(
            request,
            self.workspace_root.clone(),
            self.workspace_locks.clone(),
            self.pool.clone(),
            Some(self.lsp_service.clone()),
        )
        .await
        .map_err(map_mutation_error)?;

        // The registry is deliberately marked only after the checked mutation
        // service reports success. Failed validation or writes remain retryable
        // only through an explicit fresh model call, never broker retry.
        self.preview_registry.lock().mark_applied(&preview_id);
        Ok(result)
    }
}

fn candidate_to_request(
    candidate: egglsp::tui_summary::PreviewApplyCandidate,
    workspace_id: String,
    session_id: String,
    turn_id: Option<String>,
) -> Result<LspPreviewApplyRequestDto, ToolError> {
    if candidate.patches.is_empty() {
        return Err(ToolError::Execution(
            "LSP preview contains no applicable file patches".to_string(),
        ));
    }
    let patches = candidate
        .patches
        .into_iter()
        .map(|patch| codegg_protocol::lsp::LspPreviewPatchDto {
            path: patch.path,
            patch: patch.patch,
            original_hash: patch.original_hash,
        })
        .collect();
    Ok(LspPreviewApplyRequestDto {
        preview_id: candidate.preview_id,
        preview_revision: candidate.preview_revision,
        preview_digest: candidate.preview_digest,
        kind: candidate.kind,
        title: candidate.title,
        provenance: candidate.provenance,
        workspace_id,
        session_id,
        turn_id,
        patches,
    })
}

fn map_mutation_error(error: LspMutationApplyError) -> ToolError {
    ToolError::Execution(error.to_string())
}

#[async_trait]
impl Tool for LspPreviewApplyTool {
    fn name(&self) -> &str {
        "lsp_preview_apply"
    }

    fn description(&self) -> &str {
        "Apply one current reviewed LSP preview by opaque preview ID through checked workspace mutation."
    }

    fn parameters(&self) -> serde_json::Value {
        Self::parameters_schema()
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let result = self.apply(input, None).await?;
        serde_json::to_string(&LspPreviewApplyOutput {
            operation: "lsp_preview_apply",
            result,
        })
        .map_err(|error| ToolError::Execution(format!("serialize LSP preview result: {error}")))
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        context: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let started = Instant::now();
        let result = self.apply(input, context.as_ref()).await?;
        let value = serde_json::to_value(LspPreviewApplyOutput {
            operation: "lsp_preview_apply",
            result,
        })
        .map_err(|error| ToolError::Execution(format!("serialize LSP preview result: {error}")))?;
        let output = serde_json::to_string(&value).map_err(|error| {
            ToolError::Execution(format!("serialize LSP preview result: {error}"))
        })?;
        Ok(StructuredToolResult::with_value(
            output,
            value,
            true,
            Some(ToolProvenance {
                backend: "native".to_string(),
                implementation: "codegg/lsp_preview_apply".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                truncated: false,
                trust: ToolTrust::MutatingSideEffect,
            }),
        ))
    }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::NonIdempotent,
            idempotency: IdempotencyClass::NonIdempotent,
            retry_policy: ToolRetryPolicy::none(),
            cache_policy: Default::default(),
            projection_policy: Default::default(),
            implementation_id: "codegg/lsp_preview_apply".to_string(),
            implementation_version: env!("CARGO_PKG_VERSION").to_string(),
            input_schema,
            output_schema: Some(Self::output_schema()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::session::schema::migrate;
    use codegg_core::workspace_services::WorkspaceLockTable;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::collections::HashMap;
    use tokio::sync::Barrier;

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        migrate(&pool).await.expect("migrate");
        sqlx::query(
            "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
             VALUES ('project-test', '/tmp', 0, 0, '[]')",
        )
        .execute(&pool)
        .await
        .expect("project");
        sqlx::query(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, workspace_id) \
             VALUES ('session-test', 'project-test', 'test', '/tmp', 'test', '1', 0, 0, 'workspace-test')",
        )
        .execute(&pool)
        .await
        .expect("session");
        pool
    }

    async fn factory_pool(workspace_id: &str, session_id: &str) -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        migrate(&pool).await.expect("migrate");
        sqlx::query(
            "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
             VALUES ('project-test', ?, 0, 0, '[]')",
        )
        .bind("/tmp")
        .execute(&pool)
        .await
        .expect("project");
        sqlx::query(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, workspace_id) \
             VALUES (?, 'project-test', 'test', '/tmp', 'test', '1', 0, 0, ?)",
        )
        .bind(session_id)
        .bind(workspace_id)
        .execute(&pool)
        .await
        .expect("session");
        pool
    }

    fn register_formatting_preview(
        registry: &LspPreviewRegistryHandle,
        root: &std::path::Path,
        path: &std::path::Path,
    ) -> String {
        let preview = egglsp::edit::preview_text_edits_for_file(
            "formatting",
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
        let original_hash = file.original_hash.clone();
        let path_string = path.display().to_string();
        let patch = egglsp::context::PreviewFilePatch {
            path: path_string.clone(),
            patch: file.patch,
            original_hash: original_hash.clone(),
        };
        registry.lock().register(
            egglsp::context::LspPreviewArtifact::Formatting {
                description: "format old -> new".to_string(),
                content_hash: None,
                edit_count: 1,
                patches: vec![patch],
            },
            vec![path_string.clone()],
            HashMap::from([(path_string, original_hash)]),
            "test:lsp".to_string(),
        )
    }

    fn production_registry(
        pool: sqlx::SqlitePool,
        root: &std::path::Path,
        workspace_id: &str,
        session_id: &str,
        locks: Arc<WorkspaceLockTable>,
        lsp_service: Arc<crate::lsp::service::LspService>,
    ) -> crate::tool::ToolRegistry {
        let workspace = Arc::new(codegg_core::workspace::WorkspaceRecord {
            id: codegg_core::workspace::WorkspaceId::parse(workspace_id).expect("workspace id"),
            canonical_root: root.to_path_buf(),
            display_name: "preview-test".to_string(),
            created_at: chrono::Utc::now(),
            last_opened_at: chrono::Utc::now(),
            archived_at: None,
        });
        let execution = codegg_core::workspace::ExecutionContext::new(
            workspace,
            Some(session_id.to_string()),
            tokio_util::sync::CancellationToken::new(),
        );
        let (registry, _) = crate::tool::factory::build_session_tool_registry(
            &crate::config::schema::Config::default(),
            Some(pool),
            session_id,
            None,
            crate::model_profile::types::TaskStatePolicy::explicit_todo(),
            None,
            execution,
            crate::tool::factory::SessionToolContext {
                workspace_locks: Some(locks),
                lsp_service: Some(lsp_service),
                turn_id: Some("turn-test".to_string()),
                ..Default::default()
            },
        );
        registry
    }

    #[test]
    fn public_schema_accepts_only_preview_id() {
        let schema = LspPreviewApplyTool::parameters_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["preview_id"]));
        assert!(serde_json::from_value::<LspPreviewApplyInput>(json!({
            "preview_id": "preview-1"
        }))
        .is_ok());
        assert!(serde_json::from_value::<LspPreviewApplyInput>(json!({
            "preview_id": "preview-1",
            "patches": []
        }))
        .is_err());
    }

    #[test]
    fn candidate_conversion_keeps_identity_host_bound() {
        let candidate = egglsp::tui_summary::PreviewApplyCandidate {
            preview_id: "preview-1".to_string(),
            preview_revision: 7,
            preview_digest: "digest".to_string(),
            kind: "formatting".to_string(),
            title: "format".to_string(),
            affected_files: vec!["src/lib.rs".to_string()],
            original_hashes: Default::default(),
            edit_count: 1,
            stale_base: false,
            provenance: "server".to_string(),
            applied: false,
            patches: vec![egglsp::context::PreviewFilePatch {
                path: "src/lib.rs".to_string(),
                patch: "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                original_hash: "hash".to_string(),
            }],
        };
        let request = candidate_to_request(
            candidate,
            "workspace-1".to_string(),
            "session-1".to_string(),
            Some("turn-1".to_string()),
        )
        .unwrap();
        assert_eq!(request.workspace_id, "workspace-1");
        assert_eq!(request.session_id, "session-1");
        assert_eq!(request.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(request.patches.len(), 1);
    }

    #[tokio::test]
    async fn applies_shared_preview_and_marks_only_after_success() {
        for kind in ["rename", "formatting", "code_action"] {
            let root = tempfile::tempdir().expect("tempdir");
            let path = root.path().join("main.rs");
            std::fs::write(&path, "old\n").expect("write original");
            let preview = egglsp::edit::preview_text_edits_for_file(
                kind,
                &path,
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
                Some(root.path()),
            )
            .expect("preview");
            let file = preview.files.into_iter().next().expect("file preview");
            let patch = egglsp::context::PreviewFilePatch {
                path: path.display().to_string(),
                patch: file.patch,
                original_hash: file.original_hash.clone(),
            };
            let registry = Arc::new(parking_lot::Mutex::new(
                egglsp::preview_registry::PreviewArtifactRegistry::new(),
            ));
            let artifact = match kind {
                "rename" => egglsp::context::LspPreviewArtifact::Rename {
                    description: kind.to_string(),
                    edit_count: 1,
                    patches: vec![patch],
                },
                "formatting" => egglsp::context::LspPreviewArtifact::Formatting {
                    description: kind.to_string(),
                    content_hash: None,
                    edit_count: 1,
                    patches: vec![patch],
                },
                "code_action" => egglsp::context::LspPreviewArtifact::CodeAction {
                    description: kind.to_string(),
                    kind: Some("quickfix".to_string()),
                    edit_count: 1,
                    patches: vec![patch],
                },
                _ => unreachable!(),
            };
            let preview_id = registry.lock().register(
                artifact,
                vec![path.display().to_string()],
                HashMap::from([(path.display().to_string(), file.original_hash)]),
                "test:lsp".to_string(),
            );
            let pool = test_pool().await;
            let tool = LspPreviewApplyTool::new(
                pool,
                root.path().to_path_buf(),
                "workspace-test".to_string(),
                "session-test".to_string(),
                Some("turn-test".to_string()),
                Arc::new(WorkspaceLockTable::new()),
                crate::lsp::service::LspService::new_arc(crate::lsp::config_lsp_to_egglsp(
                    crate::config::schema::LspConfig::default(),
                )),
                registry.clone(),
            );
            let contract = tool.contract(tool.name(), tool.parameters());
            assert_eq!(contract.caller_policy, ToolCallerPolicy::DirectOnly);
            assert_eq!(contract.effect_class, ToolEffectClass::NonIdempotent);
            assert_eq!(contract.retry_policy.max_retries, 0);

            if kind == "rename" {
                registry.lock().mark_stale(&preview_id);
                let stale = tool
                    .execute(json!({"preview_id": preview_id}))
                    .await
                    .expect_err("stale preview must be rejected before mutation");
                assert!(stale.to_string().contains("stale"));
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
                registry.lock().get_mut(&preview_id).unwrap().stale_base = false;
            }

            let output = tool
                .execute(json!({"preview_id": preview_id}))
                .await
                .expect("apply");
            assert!(output.contains("lsp_preview_apply"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
            assert!(registry.lock().get(&preview_id).unwrap().applied);
            let second = tool
                .execute(json!({"preview_id": preview_id}))
                .await
                .expect_err("applied preview must not replay");
            assert!(second.to_string().contains("already been applied"));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn same_preview_concurrent_apply_commits_once() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("main.rs");
        std::fs::write(&path, "old\n").expect("write original");
        let pool = test_pool().await;
        let registry = Arc::new(parking_lot::Mutex::new(
            egglsp::preview_registry::PreviewArtifactRegistry::new(),
        ));
        let preview_id = register_formatting_preview(&registry, root.path(), &path);
        let tool = Arc::new(
            LspPreviewApplyTool::new(
                pool.clone(),
                root.path().to_path_buf(),
                "workspace-test".to_string(),
                "session-test".to_string(),
                Some("turn-test".to_string()),
                Arc::new(WorkspaceLockTable::new()),
                crate::lsp::service::LspService::new_arc(crate::lsp::config_lsp_to_egglsp(
                    crate::config::schema::LspConfig::default(),
                )),
                registry.clone(),
            )
            .with_apply_barrier(Arc::new(Barrier::new(2))),
        );
        let contract = tool.contract(tool.name(), tool.parameters());
        assert_eq!(contract.caller_policy, ToolCallerPolicy::DirectOnly);
        assert_eq!(contract.effect_class, ToolEffectClass::NonIdempotent);
        assert_eq!(contract.idempotency, IdempotencyClass::NonIdempotent);
        assert_eq!(contract.retry_policy.max_retries, 0);

        let checkpoint_count_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM edit_checkpoint WHERE session_id = ?")
                .bind("session-test")
                .fetch_one(&pool)
                .await
                .expect("checkpoint count before");
        assert_eq!(checkpoint_count_before, 0);

        let (left, right) = tokio::join!(
            tool.execute(json!({"preview_id": preview_id.clone()})),
            tool.execute(json!({"preview_id": preview_id.clone()})),
        );
        let outcomes = [left, right];
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
            1,
            "same-preview contention must produce exactly one success: {outcomes:?}"
        );
        let loser = outcomes
            .iter()
            .find_map(|outcome| outcome.as_ref().err())
            .expect("one contention caller must lose");
        let loser_text = loser.to_string();
        assert!(
            loser_text.contains("already been applied") || loser_text.contains("stale"),
            "unexpected same-preview loser error: {loser_text}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        assert!(registry.lock().get(&preview_id).unwrap().applied);

        let checkpoint_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM edit_checkpoint WHERE session_id = ? ORDER BY created_at",
        )
        .bind("session-test")
        .fetch_all(&pool)
        .await
        .expect("checkpoint query");
        assert_eq!(checkpoint_ids.len(), checkpoint_count_before as usize + 1);
        println!(
            "same_preview_concurrent_apply_commits_once: success=1 loser={loser_text} pre_checkpoints={checkpoint_count_before} post_checkpoints={} checkpoint_id={}",
            checkpoint_ids.len(),
            checkpoint_ids[0]
        );
    }

    #[tokio::test]
    async fn fresh_tool_registry_expires_prior_preview_id() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("main.rs");
        std::fs::write(&path, "old\n").expect("write original");
        let workspace_id = codegg_core::workspace::WorkspaceId::new().into_string();
        let session_id = "session-fresh-registry-test";
        let pool = factory_pool(&workspace_id, session_id).await;
        let locks = Arc::new(WorkspaceLockTable::new());
        let lsp_service = crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        );

        let (preview_id, first_handle_identity) = {
            let registry_a = production_registry(
                pool.clone(),
                root.path(),
                &workspace_id,
                session_id,
                locks.clone(),
                lsp_service.clone(),
            );
            let handle_a = registry_a
                .lsp_preview_registry()
                .expect("registry A preview handle");
            let preview_id = register_formatting_preview(&handle_a, root.path(), &path);
            assert!(handle_a.lock().get(&preview_id).is_some());
            assert!(registry_a.get("lsp_preview_apply").is_some());
            (preview_id, Arc::as_ptr(&handle_a) as usize)
        };

        let registry_b = production_registry(
            pool.clone(),
            root.path(),
            &workspace_id,
            session_id,
            locks,
            lsp_service,
        );
        let handle_b = registry_b
            .lsp_preview_registry()
            .expect("registry B preview handle");
        assert_ne!(first_handle_identity, Arc::as_ptr(&handle_b) as usize);
        assert!(handle_b.lock().get(&preview_id).is_none());

        let checkpoints_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM edit_checkpoint WHERE session_id = ?")
                .bind(session_id)
                .fetch_one(&pool)
                .await
                .expect("checkpoint count before");
        let error = registry_b
            .get("lsp_preview_apply")
            .expect("registered preview apply tool")
            .execute(json!({"preview_id": preview_id}))
            .await
            .expect_err("old preview ID must expire with registry A");
        assert!(error.to_string().contains("not found or has expired"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
        let checkpoints_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM edit_checkpoint WHERE session_id = ?")
                .bind(session_id)
                .fetch_one(&pool)
                .await
                .expect("checkpoint count after");
        assert_eq!(checkpoints_after, checkpoints_before);
        println!(
            "fresh_tool_registry_expires_prior_preview_id: handles_distinct=true old_id_error={} checkpoints={checkpoints_after}",
            error
        );
    }
}
