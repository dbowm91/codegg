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
        }
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
}
