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

/// A large existing file a whole-file `write` must not silently destroy.
///
/// Overwriting above this size requires explicit `overwrite:true` and
/// preserves the previous version at `<path>.bak` (F20). The bound is a
/// friction point, not a storage cap: small writes keep the historical
/// default behavior.
pub(crate) const LARGE_OVERWRITE_THRESHOLD_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteMode {
    Overwrite,
    Append,
    CreateOnly,
}

fn parse_write_mode(raw: Option<&str>) -> Result<WriteMode, ToolError> {
    match raw.unwrap_or("overwrite") {
        "overwrite" => Ok(WriteMode::Overwrite),
        "append" => Ok(WriteMode::Append),
        "create_only" => Ok(WriteMode::CreateOnly),
        other => Err(ToolError::Execution(format!(
            "unknown write mode '{other}': expected overwrite, append, or create_only"
        ))),
    }
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
        "Create or overwrite a file with the given content. Modes: overwrite (default), append, create_only (refuse when the file exists). Overwriting a large existing file requires explicit overwrite:true and preserves the previous version at <path>.bak. Runs auto-formatting after write if configured."
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
                    "description": "Content to write to the file (appended after existing content in append mode)"
                },
                "mode": {
                    "type": "string",
                    "description": "Write mode: overwrite (default), append, or create_only (fail when the file exists)",
                    "enum": ["overwrite", "append", "create_only"],
                    "default": "overwrite"
                },
                "overwrite": {
                    "type": "boolean",
                    "description": "Explicit confirmation required to overwrite a large existing file (over the large-file threshold); ignored in append/create_only modes",
                    "default": false
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

        let mode = parse_write_mode(input["mode"].as_str())?;
        let overwrite_confirmed = input["overwrite"].as_bool().unwrap_or(false);

        let allowed_root = self.allowed_root.clone();
        let allowed_root_for_diff = self.allowed_root.clone();
        let unrestricted = self.unrestricted;
        let content_for_write = content.clone();
        let path_for_header = path_str.clone();

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
            let old_len = old_content.len() as u64;

            if mode == WriteMode::CreateOnly && existed {
                return Err((
                    ToolError::Execution(
                        "file exists; create_only refuses to overwrite (use mode overwrite or append)"
                            .to_string(),
                    ),
                    WriterFenceOutcome::NoEffect,
                ));
            }
            // F20: a whole-file overwrite of a large existing artifact is
            // destructive with no recovery, so it requires an explicit
            // `overwrite:true`. The mode flag alone is not confirmation.
            if mode == WriteMode::Overwrite
                && existed
                && old_len > LARGE_OVERWRITE_THRESHOLD_BYTES
                && !overwrite_confirmed
            {
                return Err((
                    ToolError::Execution(format!(
                        "refusing to overwrite large existing file ({} bytes > {} bytes) without explicit overwrite:true; re-read the file and retry with overwrite:true (the previous version is preserved at <path>.bak)",
                        old_len, LARGE_OVERWRITE_THRESHOLD_BYTES
                    )),
                    WriterFenceOutcome::NoEffect,
                ));
            }

            // F20 recovery: preserve the previous version before a
            // destructive overwrite of a large file. The sidecar is written
            // inside the same fenced effect as the overwrite, so the fence
            // covers both. Only one backup generation is kept.
            let backup_path: Option<std::path::PathBuf> = if mode == WriteMode::Overwrite
                && existed
                && old_len > LARGE_OVERWRITE_THRESHOLD_BYTES
            {
                let backup =
                    std::path::PathBuf::from(format!("{}.bak", validated_path.display()));
                std::fs::write(&backup, old_content.as_bytes()).map_err(|e| {
                    (
                        ToolError::Execution(format!(
                            "backup write failed for '{}': {}",
                            backup.display(),
                            e
                        )),
                        WriterFenceOutcome::NoEffect,
                    )
                })?;
                Some(backup)
            } else {
                None
            };

            let bytes_to_write: Vec<u8> = if mode == WriteMode::Append && existed {
                let mut combined = Vec::with_capacity(old_content.len() + content_for_write.len());
                combined.extend_from_slice(old_content.as_bytes());
                combined.extend_from_slice(content_for_write.as_bytes());
                combined
            } else {
                content_for_write.as_bytes().to_vec()
            };

            // `fs::write` truncates before it writes, so a failure here can
            // leave a partially written file behind: the effect is unproven,
            // not absent.
            std::fs::write(&validated_path, &bytes_to_write).map_err(|e| {
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
                backup_path,
                mode,
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

        let (existed, old_content, final_path, formatted, backup_path, mode) = match effect {
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
        // In append mode the expected on-disk bytes are old+new, not the
        // staged chunk alone.
        let expected_final = if mode == WriteMode::Append && existed {
            format!("{old_content}{content}")
        } else {
            content.clone()
        };
        if formatted != expected_final {
            format_note = "\n\n[auto-formatted]".to_string();
        }

        let action = if !existed {
            "Created"
        } else if mode == WriteMode::Append {
            "Appended"
        } else {
            "Updated"
        };
        let diff = generate_diff(
            &old_content,
            &expected_final,
            &path_for_header,
            Some(allowed_root_for_diff.as_path()),
        );
        let diff = crate::tool::diff::truncate_grouped_preview(diff, 4000);

        let mut output = format!(
            "{} {}{}{}",
            action,
            path_display,
            format_note,
            if diff.is_empty() {
                String::new()
            } else {
                format!("\n\n{diff}")
            }
        );
        // F20 harness advisory: concurrent writers are last-writer-wins, so
        // a shared path must be re-read before overwriting. The sidecar is
        // the recovery path when the advisory arrives too late.
        if let Some(backup) = backup_path {
            output.push_str(&format!(
                "\n\n[backup: previous version preserved at {} — concurrent writes are last-writer-wins; re-read before overwriting shared paths.]",
                backup.display()
            ));
        }
        Ok(output)
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

fn generate_diff(old: &str, new: &str, path: &str, workspace_root: Option<&Path>) -> String {
    crate::tool::diff::format_grouped_diff(old, new, path, workspace_root)
}

#[cfg(test)]
mod diff_format_tests {
    use super::generate_diff;
    use std::path::Path;

    #[test]
    fn write_diff_uses_relative_headers_and_new_side_numbering() {
        let root = Path::new("/repo");
        let preview = generate_diff(
            "A\nB\nC\n",
            "A\nINSERTED\nB\nC\n",
            "/repo/diffnum.txt",
            Some(root),
        );
        assert!(preview.contains("--- a/diffnum.txt"), "got: {preview}");
        assert!(preview.contains("+++ b/diffnum.txt"), "got: {preview}");
        assert!(!preview.contains("[CWD]"), "got: {preview}");
        assert!(
            !preview.contains("/repo/"),
            "must be repo-relative, got: {preview}"
        );
        assert!(preview.contains("   2 INSERTED"), "got: {preview}");
        assert!(preview.contains("   3 B"), "got: {preview}");
    }

    #[test]
    fn write_diff_strips_cwd_placeholder_from_header() {
        let preview = generate_diff("a\n", "a\nb\n", "[CWD]/tmp/a.txt", None);
        assert!(preview.contains("--- a/tmp/a.txt"), "got: {preview}");
        assert!(!preview.contains("[CWD]"), "got: {preview}");
    }
}

#[cfg(test)]
mod write_mode_tests {
    use super::*;

    fn tool_in(dir: &tempfile::TempDir) -> WriteTool {
        WriteTool::new().with_allowed_root(dir.path().to_path_buf())
    }

    fn input(path: &std::path::Path, content: &str) -> serde_json::Value {
        serde_json::json!({"path": path.to_string_lossy(), "content": content})
    }

    #[tokio::test(flavor = "current_thread")]
    async fn append_mode_appends_to_existing_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("append.txt");
        std::fs::write(&path, "head\n").expect("seed");
        let mut args = input(&path, "tail\n");
        args["mode"] = serde_json::json!("append");
        let output = tool_in(&dir).execute(args).await.expect("append executes");
        assert!(output.starts_with("Appended"), "got: {output}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "head\ntail\n"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_only_refuses_an_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("exists.txt");
        std::fs::write(&path, "original").expect("seed");
        let mut args = input(&path, "replacement");
        args["mode"] = serde_json::json!("create_only");
        let err = tool_in(&dir)
            .execute(args)
            .await
            .expect_err("create_only must refuse");
        assert!(err.to_string().contains("file exists"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "original"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn overwrite_of_a_large_file_needs_explicit_confirmation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("large.txt");
        let seed = "x".repeat((LARGE_OVERWRITE_THRESHOLD_BYTES + 8) as usize);
        std::fs::write(&path, &seed).expect("seed large file");
        let err = tool_in(&dir)
            .execute(input(&path, "replacement"))
            .await
            .expect_err("large overwrite must need confirmation");
        assert!(
            err.to_string().contains("overwrite:true"),
            "must name the confirmation flag, got: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            seed,
            "the refused overwrite must leave the file untouched"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn confirmed_large_overwrite_preserves_a_bak_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("large.txt");
        let seed = "y".repeat((LARGE_OVERWRITE_THRESHOLD_BYTES + 8) as usize);
        std::fs::write(&path, &seed).expect("seed large file");
        let mut args = input(&path, "replacement");
        args["overwrite"] = serde_json::json!(true);
        let output = tool_in(&dir)
            .execute(args)
            .await
            .expect("confirmed overwrite executes");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "replacement"
        );
        let backup = std::path::PathBuf::from(format!("{}.bak", path.display()));
        assert_eq!(
            std::fs::read_to_string(&backup).expect("read sidecar"),
            seed
        );
        assert!(output.contains(".bak"), "got: {output}");
        assert!(output.contains("last-writer-wins"), "got: {output}");
    }

    #[test]
    fn unknown_mode_is_rejected() {
        let err = parse_write_mode(Some("merge")).expect_err("unknown mode");
        assert!(err.to_string().contains("unknown write mode"));
    }
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
