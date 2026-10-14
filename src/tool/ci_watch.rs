//! Model-facing `ci_watch` tool (async-background-jobs M004).
//!
//! One small discoverable action over the daemon-owned GitHub Actions
//! watcher: `create` registers an explicit run id (or an immutable commit
//! SHA resolving to exactly one run) and returns a stable watch handle;
//! `status` reports the bounded watch state (advancing it with one
//! daemon-owned observation when due); `cancel` stops watching (never the
//! remote workflow); `list` shows the session's bounded watches. The model
//! never polls GitHub itself: one terminal M001/M003 notification arrives
//! without repeated tool calls.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::scheduler::ci_watch::{CiWatchService, CiWatchTarget, PROVIDER_GITHUB_ACTIONS};
use crate::scheduler::job_notifications::JobCompletionDisposition;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::{Tool, ToolCategory};

pub struct CiWatchTool {
    watches: Option<Arc<CiWatchService>>,
}

impl CiWatchTool {
    pub fn new() -> Self {
        Self { watches: None }
    }

    pub fn with_service(mut self, service: Arc<CiWatchService>) -> Self {
        self.watches = Some(service);
        self
    }
}

impl Default for CiWatchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CiWatchTool {
    fn name(&self) -> &str {
        "ci_watch"
    }

    fn description(&self) -> &str {
        "Watch one GitHub Actions workflow run to completion. Create with an explicit owner/repo plus an exact run_id (or an immutable commit SHA that resolves to exactly one run); a stable watch handle is returned and one terminal notification arrives without polling — do not call status in a loop and never run gh/sleep loops. status reports the bounded watch state, cancel stops watching (not the remote workflow), list shows this session's watches."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "status", "cancel", "list"],
                    "description": "Control action. create registers a watch; status reports (and advances when due); cancel stops watching; list shows this session's bounded watches."
                },
                "owner": {
                    "type": "string",
                    "description": "GitHub repository owner (create only)."
                },
                "repo": {
                    "type": "string",
                    "description": "GitHub repository name (create only)."
                },
                "run_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Exact workflow run id (create only; supply run_id or commit_sha, not both)."
                },
                "commit_sha": {
                    "type": "string",
                    "description": "Immutable 40-char commit SHA resolving to exactly one run (create only)."
                },
                "check_selector": {
                    "type": "string",
                    "description": "Optional check name filter used with commit_sha (create only)."
                },
                "watch_id": {
                    "type": "string",
                    "description": "Stable watch handle from create (status/cancel)."
                },
                "on_complete": {
                    "type": "string",
                    "enum": ["notify_only", "resume_owner"],
                    "description": "Background completion disposition for create (default notify_only; notify_only never spends model tokens. resume_owner additionally requires an explicit host-trusted continuation grant and may autonomously spend bounded tokens once; without a grant it is notify-only)."
                }
            },
            "required": ["action"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let value = self.execute_impl(input, None).await?;
        Ok(serde_json::to_string_pretty(&value).unwrap_or_default())
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let value = self.execute_impl(input, ctx).await?;
        let status = value
            .get("state")
            .and_then(|s| s.as_str())
            .or_else(|| value.get("status").and_then(|s| s.as_str()))
            .unwrap_or("unknown")
            .to_string();
        Ok(StructuredToolResult {
            output: format!("ci watch {status}"),
            value: Some(value),
            success: true,
            provenance: Some(ToolProvenance {
                backend: "native".to_string(),
                implementation: "codegg/ci_watch".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                elapsed_ms: None,
                truncated: false,
                trust: ToolTrust::LocalTrusted,
            }),
        })
    }
}

impl CiWatchTool {
    async fn execute_impl(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let service = self.watches.as_ref().ok_or_else(|| {
            ToolError::Execution("ci_watch requires the daemon watch service".into())
        })?;
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("missing required 'action'".into()))?;
        let caller_session = ctx.as_ref().and_then(|c| c.session_id.clone());
        match action {
            "create" => {
                let session_id = caller_session.filter(|s| !s.is_empty()).ok_or_else(|| {
                    ToolError::Execution(
                        "ci_watch create requires a real host-originated session context".into(),
                    )
                })?;
                let owner = input
                    .get("owner")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ToolError::Execution("create requires 'owner'".into()))?;
                let repo = input
                    .get("repo")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ToolError::Execution("create requires 'repo'".into()))?;
                let run_id = input.get("run_id").and_then(|v| v.as_i64());
                let commit_sha = input
                    .get("commit_sha")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                let check_selector = input
                    .get("check_selector")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                let on_complete = input
                    .get("on_complete")
                    .and_then(|v| v.as_str())
                    .map(JobCompletionDisposition::from_str)
                    .unwrap_or(JobCompletionDisposition::NotifyOnly);
                let target = CiWatchTarget {
                    provider: PROVIDER_GITHUB_ACTIONS.to_string(),
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    run_id,
                    commit_sha,
                    check_selector,
                };
                let turn_id = ctx.as_ref().and_then(|c| c.turn_id.clone());
                let invocation_key = ctx
                    .as_ref()
                    .and_then(|c| c.invocation_key.clone())
                    .unwrap_or_else(|| format!("ci-watch:{}", uuid::Uuid::new_v4()));
                let principal = ctx.as_ref().and_then(|c| {
                    c.origin_principal
                        .clone()
                        .or_else(|| c.principal_identity.clone())
                });
                let record = service
                    .create_watch(
                        target,
                        &session_id,
                        turn_id.as_deref(),
                        &invocation_key,
                        principal.as_deref(),
                        None,
                        on_complete,
                        None,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                Ok(json!({
                    "watch_id": record.watch_id,
                    "state": crate::scheduler::ci_watch::ci_watch_state_to_str(record.state),
                    "owner": record.owner,
                    "repo": record.repo,
                    "run_id": record.run_id,
                    "on_complete": record.on_complete.as_str(),
                    "message": "CI watch accepted; one terminal notification will arrive without polling — do not poll status in a loop",
                }))
            }
            "status" => {
                let watch_id = input
                    .get("watch_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ToolError::Execution("status requires 'watch_id'".into()))?;
                let view = service
                    .status(watch_id, caller_session.as_deref())
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                Ok(serde_json::to_value(view).unwrap_or_else(|_| json!({"watch_id": watch_id})))
            }
            "cancel" => {
                let watch_id = input
                    .get("watch_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ToolError::Execution("cancel requires 'watch_id'".into()))?;
                let view = service
                    .cancel(watch_id, caller_session.as_deref())
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                Ok(json!({
                    "watch_id": view.watch_id,
                    "state": view.state,
                    "message": "watching stopped; the remote workflow was not cancelled",
                }))
            }
            "list" => {
                let session_id = caller_session.filter(|s| !s.is_empty()).ok_or_else(|| {
                    ToolError::Execution("ci_watch list requires a session context".into())
                })?;
                let views = service
                    .list_for_session(&session_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                Ok(json!({
                    "watches": views,
                    "count": views.len(),
                }))
            }
            _ => Err(ToolError::Execution(format!(
                "unknown action '{action}'. Use create|status|cancel|list"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_watch_tool_name() {
        assert_eq!(CiWatchTool::new().name(), "ci_watch");
    }

    #[test]
    fn ci_watch_requires_action() {
        let tool = CiWatchTool::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool.execute(json!({})).await.unwrap_err();
            assert!(
                err.to_string().contains("daemon watch service")
                    || err.to_string().contains("action")
            );
        });
    }
}
