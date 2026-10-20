use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::tool::backend::ToolExecutionContext;
use crate::tool::writer_fence::{
    acquire_writer_permit, settle_writer_fence, writer_fence_key, NativeEffectClass,
    WriterFenceOutcome,
};
use crate::tool::{Tool, ToolCategory};

pub struct WriteTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl WriteTool {
    pub fn new() -> Self {
        Self {
            allowed_root: crate::tool::util::unresolved_workspace_root(),
            unrestricted: false,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self.unrestricted = false;
        self
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("write")
    }

    fn description(&self) -> &str {
        "Create or overwrite a file with the given content. Runs auto-formatting after write if configured."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_inner(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<crate::tool::StructuredToolResult, ToolError> {
        let scratch_scope = ctx
            .as_ref()
            .and_then(|ctx| ctx.execution_authorization.as_ref())
            .and_then(|authorization| authorization.scratch_scope.as_ref());
        let scratch = if let Some(scope) = scratch_scope {
            Some(scope.private_root.clone())
        } else {
            ctx.as_ref()
                .and_then(|ctx| ctx.session_id.as_deref())
                .filter(|session_id| !session_id.is_empty())
                .map(crate::scratch::session_scratch_dir)
                .transpose()
                .map_err(|error| ToolError::Execution(error.to_string()))?
        };
        let shared_roots = scratch_scope
            .into_iter()
            .flat_map(|scope| scope.shared.iter().map(|root| root.path.clone()))
            .collect::<Vec<_>>();
        let requested = input["path"].as_str().map(Path::new);
        let output = if let (Some(path), Some(scratch)) = (requested, scratch) {
            let root = std::iter::once(scratch.clone())
                .chain(shared_roots)
                .find(|root| path.is_absolute() && path.starts_with(root));
            if let Some(root) = root {
                // The shared scratch root is a different write root than this
                // tool's own, so the effect has to run against *that* root —
                // including the fence, which must name the path this root
                // resolves to.
                let tool = Self::new().with_allowed_root(root);
                tool.execute_inner(input, ctx.as_ref()).await?
            } else {
                if crate::tool::util::validate_target_path(path, &self.allowed_root).is_err() {
                    if let Some(error) = temp_path_recovery_error(path, Some(&scratch)) {
                        return Err(error);
                    }
                }
                self.execute_inner(input, ctx.as_ref()).await?
            }
        } else {
            self.execute_inner(input, ctx.as_ref()).await?
        };
        Ok(crate::tool::StructuredToolResult::legacy("write", output))
    }
}

impl WriteTool {
    /// The write effect, with the checkout writer fence held across it.
    ///
    /// The fence is an *additional* refusal on top of the existing path
    /// boundary: it never widens what this tool may write. A context without
    /// a fence mints no permit, so the legacy path behaves exactly as
    /// before.
    async fn execute_inner(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?
            .to_string();

        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'content' parameter".to_string()))?
            .to_string();

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;
        let content_for_write = content.clone();

        // Resolve the target before the permit is taken: the fence has to
        // name the very path the write lands on, and the target may not
        // exist yet, so this is the tool's own `validate_target_path`
        // resolution — not a second, looser one.
        let validated_path = tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_str);
            if !unrestricted {
                crate::tool::util::validate_target_path(original_path, &allowed_root)
            } else {
                let parent_to_validate = original_path
                    .parent()
                    .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
                crate::tool::util::check_path_for_symlinks(parent_to_validate)?;
                let parent_canonical = parent_to_validate.canonicalize().map_err(|_| {
                    ToolError::Execution(format!("invalid path: {}", parent_to_validate.display()))
                })?;
                let file_name = original_path
                    .file_name()
                    .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
                Ok::<_, ToolError>(parent_canonical.join(file_name))
            }
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        // The permit is taken after the path boundary approved the target
        // and immediately before the first mutation, and it is held until
        // the write has settled.
        let targets = vec![validated_path.clone()];
        let permit = acquire_writer_permit(
            ctx,
            NativeEffectClass::FileWrite,
            targets.clone(),
            writer_fence_key(self.name(), &targets, ctx),
        )
        .await?;

        let effect = match tokio::task::spawn_blocking(move || {
            if let Some(parent) = validated_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    (
                        ToolError::Execution(e.to_string()),
                        WriterFenceOutcome::NoEffect,
                    )
                })?;
            }

            let existed = std::path::Path::new(&validated_path).exists();
            let old_content = if existed {
                std::fs::read_to_string(&validated_path).unwrap_or_default()
            } else {
                String::new()
            };

            // `fs::write` truncates before it writes, so a failure here can
            // leave a partially written file behind: the effect is unproven,
            // not absent.
            std::fs::write(&validated_path, content_for_write.as_bytes()).map_err(|e| {
                (
                    ToolError::Execution(e.to_string()),
                    WriterFenceOutcome::Unproven(e.to_string()),
                )
            })?;

            let formatted = std::fs::read_to_string(&validated_path).unwrap_or_default();

            Ok::<_, (ToolError, WriterFenceOutcome)>((
                existed,
                old_content,
                validated_path,
                formatted,
            ))
        })
        .await
        {
            Ok(Ok(written)) => Ok(written),
            Ok(Err(failure)) => Err(failure),
            Err(join_error) => Err((
                ToolError::Execution(format!("join error: {}", join_error)),
                WriterFenceOutcome::Unproven(format!("join error: {}", join_error)),
            )),
        };

        let (existed, old_content, final_path, formatted) = match effect {
            Ok(written) => {
                // The effect landed and was observed, so the permit is
                // released. A failure to release it is reported: the caller
                // has to know the checkout was not handed on.
                settle_writer_fence(permit, WriterFenceOutcome::Settled, false).await?;
                written
            }
            Err((error, outcome)) => {
                // Settle before the error propagates: a failed effect must
                // still release (or park) the permit it held. The tool's own
                // error is the one the caller sees.
                settle_writer_fence(permit, outcome, true).await?;
                return Err(error);
            }
        };

        let action_str = if existed { "Modified" } else { "Created" };
        let path_display = final_path.to_string_lossy().to_string();
        GlobalEventBus::publish(AppEvent::FileChanged {
            path: path_display.clone(),
            action: action_str.to_string(),
            old_content: if old_content.is_empty() {
                None
            } else {
                Some(old_content.clone())
            },
        });

        let mut format_note = String::new();
        if formatted != content {
            format_note = "\n\n[auto-formatted]".to_string();
        }

        let action = if existed { "Updated" } else { "Created" };
        let diff = generate_diff(&old_content, &content, &path_display);

        Ok(format!(
            "{} {}{}{}",
            action,
            path_display,
            format_note,
            if diff.is_empty() {
                String::new()
            } else {
                format!("\n\n{diff}")
            }
        ))
    }
}

fn temp_path_recovery_error(path: &Path, private_scratch: Option<&Path>) -> Option<ToolError> {
    // The wording is shared with the permission layer, which may answer the
    // model with the same offer before the human is asked. One message, one
    // contract: it names a suggestion and grants nothing.
    Some(ToolError::Permission(
        crate::tool::util::temp_root_redirect_message(path, private_scratch)?,
    ))
}

impl Default for WriteTool {
    fn default() -> Self {
        Self {
            allowed_root: crate::tool::util::unresolved_workspace_root(),
            unrestricted: false,
        }
    }
}

fn generate_diff(old: &str, new: &str, path: &str) -> String {
    use similar::TextDiff;
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("--- a/{}\n", path));
    result.push_str(&format!("+++ b/{}\n", path));
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            result.push_str("...\n");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };
                let line_num = change.old_index().or(change.new_index()).unwrap_or(0) + 1;
                result.push_str(&format!(
                    "{}{:>4} {}\n",
                    sign,
                    line_num,
                    change.value().trim_end_matches('\n'),
                ));
            }
        }
    }
    result
}

#[cfg(test)]
mod delegated_scratch_tests {
    use super::*;

    fn context(
        scope: crate::scratch::AgentScratchScope,
    ) -> crate::tool::backend::ToolExecutionContext {
        let mut context = crate::tool::backend::ToolExecutionContext::with_backend(
            crate::tool::backend::ToolBackendKind::Native,
        );
        context.session_id = Some(scope.session_id.clone());
        context.execution_authorization =
            Some(crate::tool::authorization::ExecutionAuthorizationContext {
                snapshot: codegg_core::approval::ExecutionPolicySnapshot::capture(
                    codegg_core::approval::ApprovalMode::Interactive,
                    codegg_core::approval::SandboxProfile::WorkspaceWrite,
                    None,
                    Some(scope.session_id.clone()),
                    Some("test-child".into()),
                    None,
                    None,
                ),
                receipt: crate::permission::PermissionDecisionReceipt::allowed("test", None),
                command_preflight: None,
                scratch_scope: Some(scope),
            });
        context
    }

    #[tokio::test]
    async fn only_explicitly_granted_child_tools_can_read_or_write_shared_scratch() {
        use crate::tool::Tool;

        let session_id = format!("shared-tool-session-{}", uuid::Uuid::new_v4());
        let scratch_id = codegg_core::identity::ScratchId::new();
        let shared_dir = tempfile::tempdir().unwrap();
        let resource_root = shared_dir.path().to_path_buf();
        let file = resource_root.join("handoff.txt");
        let mut private_dirs = Vec::new();
        let mut make_scope = |granted: bool| {
            let run_id = codegg_core::identity::AgentRunId::new();
            let private = tempfile::tempdir().unwrap();
            let private_root = private.path().to_path_buf();
            private_dirs.push(private);
            crate::scratch::AgentScratchScope {
                session_id: session_id.clone(),
                run_id,
                private_root,
                shared: if granted {
                    vec![crate::scratch::AuthorizedSharedScratchRoot {
                        scratch_id: scratch_id.clone(),
                        alias: "handoff".into(),
                        path: resource_root.clone(),
                    }]
                } else {
                    Vec::new()
                },
            }
        };

        WriteTool::default()
            .execute_structured(
                serde_json::json!({"path": file, "content": "artifact"}),
                Some(context(make_scope(true))),
            )
            .await
            .expect("granted child writes through structured tool");
        let read = crate::tool::read::ReadTool::new()
            .execute_structured(
                serde_json::json!({"path": file}),
                Some(context(make_scope(true))),
            )
            .await
            .expect("second granted child reads through structured tool");
        assert!(read.output.contains("artifact"));

        assert!(WriteTool::default()
            .execute_structured(
                serde_json::json!({"path": file, "content": "forbidden"}),
                Some(context(make_scope(false))),
            )
            .await
            .is_err());
        assert!(crate::tool::read::ReadTool::new()
            .execute_structured(
                serde_json::json!({"path": file}),
                Some(context(make_scope(false))),
            )
            .await
            .is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "artifact");
    }

    #[tokio::test]
    async fn unauthorized_temp_write_suggests_private_scratch_without_rewriting() {
        use crate::tool::Tool;

        let session_id = format!("temp-hint-session-{}", uuid::Uuid::new_v4());
        let private = tempfile::tempdir().unwrap();
        let private_root = private.path().canonicalize().unwrap();
        let requested_root = tempfile::tempdir().unwrap();
        let requested_path = requested_root.path().join("original.txt");
        let scope = crate::scratch::AgentScratchScope {
            session_id: session_id.clone(),
            run_id: codegg_core::identity::AgentRunId::new(),
            private_root: private_root.clone(),
            shared: Vec::new(),
        };
        let error = WriteTool::default()
            .execute_structured(
                serde_json::json!({"path": requested_path, "content": "do not redirect"}),
                Some(context(scope)),
            )
            .await
            .expect_err("ungranted temp writes remain denied by the path boundary");
        let message = error.to_string();
        assert!(message.contains("retry at"), "{message}");
        assert!(
            message.contains(private_root.to_str().unwrap()),
            "{message}"
        );
        assert!(message.contains("request access"), "{message}");
        assert!(
            !requested_path.exists(),
            "the original path was not written"
        );
        assert_eq!(std::fs::read_dir(private.path()).unwrap().count(), 0);

        let original = requested_root.path().join("real-read.txt");
        std::fs::write(&original, "host temp data").unwrap();
        let read_scope = crate::scratch::AgentScratchScope {
            session_id,
            run_id: codegg_core::identity::AgentRunId::new(),
            private_root,
            shared: Vec::new(),
        };
        assert!(
            crate::tool::read::ReadTool::new()
                .execute_structured(
                    serde_json::json!({"path": original}),
                    Some(context(read_scope)),
                )
                .await
                .is_err(),
            "reading the actual temp path is not redirected"
        );
    }

    #[tokio::test]
    async fn workspace_write_under_host_temp_is_not_misclassified_as_unauthorized() {
        use crate::tool::Tool;

        let session_id = format!("temp-workspace-session-{}", uuid::Uuid::new_v4());
        let workspace = tempfile::tempdir().unwrap();
        let workspace_root = workspace.path().canonicalize().unwrap();
        let private = tempfile::tempdir().unwrap();
        let target = workspace_root.join("workspace-file.txt");
        let scope = crate::scratch::AgentScratchScope {
            session_id,
            run_id: codegg_core::identity::AgentRunId::new(),
            private_root: private.path().canonicalize().unwrap(),
            shared: Vec::new(),
        };

        WriteTool::new()
            .with_allowed_root(workspace_root)
            .execute_structured(
                serde_json::json!({"path": target, "content": "workspace write"}),
                Some(context(scope)),
            )
            .await
            .expect("a temp-root workspace remains an authorized write root");
        assert_eq!(std::fs::read_to_string(target).unwrap(), "workspace write");
    }
}
