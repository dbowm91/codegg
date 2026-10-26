use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::agent::EMERGENCY_DEFAULT_MODEL;
use crate::config::schema::Config;
use crate::error::ToolError;
use crate::git_service::{DiffFilePayload, DiffResultPayload, GitExecutionService, GitPayload};
use crate::provider::{
    register_builtin_with_config, ChatEvent, ChatRequest, ContentPart, Message, Provider,
    ProviderRegistry, ProviderRequestContext,
};
use crate::tool::backend::provider_request_context;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};

use codegg_git::GitOperation;

pub struct ReviewTool {
    workdir: std::path::PathBuf,
    provider: Option<std::sync::Arc<dyn Provider>>,
}

impl ReviewTool {
    pub fn new() -> Self {
        Self {
            workdir: crate::tool::util::unresolved_workspace_root(),
            provider: None,
        }
    }

    pub fn with_workdir(mut self, dir: std::path::PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    /// Override provider selection for callers that already own a provider
    /// and for focused nested-request tests.
    pub fn with_provider(mut self, provider: std::sync::Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Effective repository root: the broker execution-context cwd
    /// (workspace root installed by `BrokerAdapter`) when it names a
    /// directory, else the registry-installed tool root. A context-free
    /// direct call with no installed root fails closed with the
    /// workspace-root error rather than reading the daemon's cwd.
    /// `git_query` resolves the same way; the earlier asymmetry (where
    /// `review` ignored the context) is removed.
    fn effective_root(&self, ctx: Option<&ToolExecutionContext>) -> PathBuf {
        if let Some(context) = ctx {
            if context.cwd.is_dir() {
                return context.cwd.clone();
            }
        }
        self.workdir.clone()
    }

    async fn get_diff_with_context(
        &self,
        staged: bool,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let svc = GitExecutionService::new();
        let operation = if staged {
            GitOperation::DiffStaged {
                stat: false,
                name_only: false,
                paths: Vec::new(),
            }
        } else {
            GitOperation::Diff {
                staged: false,
                stat: false,
                name_only: false,
                base_ref: None,
                paths: Vec::new(),
            }
        };
        let root = self.effective_root(ctx);
        let result = svc
            .execute(&operation, &root)
            .await
            .map_err(|e| ToolError::Execution(format!("git diff failed: {e}")))?;
        // Prefer the typed DiffText payload; fall back to stdout when the
        // service returns a parsed-only payload (DiffSummary/DiffResult).
        let text = match result.payload {
            Some(GitPayload::DiffText(text)) => text,
            Some(GitPayload::DiffResult(diff)) => {
                // Render a textual representation from parsed fields.
                render_structured_diff(&diff)
            }
            Some(GitPayload::DiffSummary(summary)) => {
                let mut s = format!(
                    "{} files changed, {} insertions(+), {} deletions(-)\n",
                    summary.files_changed, summary.insertions, summary.deletions
                );
                for f in &summary.files {
                    s.push_str(&format!("  {} {}\n", f.kind, f.path));
                }
                s
            }
            _ => result.stdout,
        };
        if text.trim().is_empty() {
            return Err(ToolError::Execution("no changes to review".to_string()));
        }
        Ok(text)
    }

    async fn analyze_diff_with_context(
        &self,
        diff: &str,
        context: ProviderRequestContext,
    ) -> Result<String, ToolError> {
        let config = Config::load_or_default();
        let model = config
            .model
            .clone()
            .unwrap_or_else(|| EMERGENCY_DEFAULT_MODEL.to_string());

        if let Some(provider) = self.provider.as_deref() {
            let model_id = split_model_target(&model)?.1;
            return analyze_diff_with_provider(diff, provider, &model_id, context).await;
        }

        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        let (provider, model_id) = resolve_review_provider(&registry, &model)?;

        analyze_diff_with_provider(diff, provider, &model_id, context).await
    }
}

/// Split a `provider/model` string into its registry provider name and bare
/// model id. The registry is keyed by provider id
/// (`provider_core.rs::get` is an exact match), so looking up the full
/// `provider/model` string never matches and must not fall through to an
/// arbitrary provider.
fn split_model_target(model: &str) -> Result<(String, String), ToolError> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(ToolError::Execution(
            "Model must be specified as 'provider/model'. Set `model` via `codegg edit` or pass `-m provider/model`; run `codegg doctor` to check wiring."
                .to_string(),
        ));
    }
    let raw_id = match trimmed.split_once('/') {
        Some((_, id)) => id.trim(),
        None => {
            return Err(ToolError::Execution(format!(
                "Model '{trimmed}' must be specified as 'provider/model'. Set `model` via `codegg edit` or pass `-m provider/model`; run `codegg doctor` to check wiring."
            )));
        }
    };
    let provider_name =
        crate::core::session_selection::runtime_model_provider_segment(trimmed).trim();
    if provider_name.is_empty() || raw_id.is_empty() {
        return Err(ToolError::Execution(format!(
            "Model '{trimmed}' must be specified as 'provider/model' with a non-empty model id. Set `model` via `codegg edit` or pass `-m provider/model`; run `codegg doctor` to check wiring."
        )));
    }
    Ok((provider_name.to_string(), raw_id.to_string()))
}

fn resolve_review_provider<'a>(
    registry: &'a ProviderRegistry,
    model: &str,
) -> Result<(&'a dyn Provider, String), ToolError> {
    let (provider_name, model_id) = split_model_target(model)?;
    let provider = registry.get(&provider_name).ok_or_else(|| {
        ToolError::Execution(format!(
            "No active connection matches provider '{provider_name}' (model '{model}'). Open /connect to create one, or set `model` via `codegg edit` / `-m provider/model`; run `codegg doctor` to check wiring."
        ))
    })?;
    Ok((provider, model_id))
}

fn map_llm_request_error(e: &crate::provider::ProviderError, model: &str) -> ToolError {
    if e.error_class() == "model_not_found" {
        return ToolError::Execution(format!(
            "Model '{model}' was not found by the provider ({e}). Open /connect to create a connection, or set `model` via `codegg edit` / `-m provider/model`; run `codegg doctor` to check wiring."
        ));
    }
    ToolError::Execution(format!("LLM request failed: {e}"))
}

async fn analyze_diff_with_provider(
    diff: &str,
    provider: &dyn Provider,
    model: &str,
    context: ProviderRequestContext,
) -> Result<String, ToolError> {
    let prompt = format!(
        "Review the following git diff and provide structured feedback. \
             Identify potential bugs, performance issues, style violations, and improvements. \
             Use the following emoji categories:\n\
             🐛 Bug: for functional issues\n\
             🚀 Performance: for efficiency improvements\n\
             🎨 Style: for readability and convention issues\n\
             💡 Suggestion: for general improvements\n\n\
             Diff:\n{}",
        diff
    );

    let request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text { text: prompt.into() }],
            }],
            model: model.to_string(),
            tools: None,
            system: Some("You are a professional code reviewer. Provide constructive, concise feedback using emojis.".to_string()),
            temperature: Some(0.3),
            top_p: None,
            max_tokens: Some(1000),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context,
        };

    let mut stream = provider
        .stream(&request)
        .await
        .map_err(|e| map_llm_request_error(&e, &request.model))?;

    let mut review = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(ChatEvent::TextDelta(delta)) => review.push_str(&delta),
            Ok(ChatEvent::Finish { .. }) => break,
            Ok(ChatEvent::Error(e)) => {
                return Err(ToolError::Execution(format!("LLM error: {}", e)))
            }
            _ => {}
        }
    }

    let review = review.trim();
    if review.is_empty() {
        return Err(ToolError::Execution(
            "LLM generated empty review".to_string(),
        ));
    }

    Ok(review.to_string())
}

impl Default for ReviewTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a structured diff result as plain text for LLM consumption.
fn render_structured_diff(diff: &DiffResultPayload) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} files changed, {} insertions(+), {} deletions(-)",
        diff.files.len(),
        diff.total_insertions,
        diff.total_deletions
    );
    for DiffFilePayload { path, .. } in &diff.files {
        let _ = writeln!(out, "  {path}");
    }
    out
}

#[async_trait]
impl Tool for ReviewTool {
    fn name(&self) -> &str {
        "review"
    }

    fn description(&self) -> &str {
        "Read-only tool that analyzes the git diff for the session workspace and provides \
         structured code review feedback. Requires an active LLM connection (provider/model from \
         `model`, e.g. `openai/gpt-4o`): without a reachable provider it returns an actionable \
         offline fallback (diff stats plus a review checklist) instead of failing silently. \
         Reads the working-tree diff through the \
         session workspace root, not the process cwd; fails when no workspace is \
         bound (use git_query status to check the binding)."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "staged": {
                    "type": "boolean",
                    "description": "Review staged changes (default: true)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    /// M002: LLM synthesis review is deferred; discover via `tool_search`.
    fn defer_loading(&self) -> bool {
        crate::tool::disclosure::is_deferred_by_default(self.name())
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_with_context(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        context: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute_with_context(input, context.as_ref()).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}

/// Offline fallback when no LLM provider is reachable: diff stats plus a
/// review checklist that echoes the configured model value, so the agent can
/// act without burning another call discovering the provider requirement.
///
/// Single counting rule: `files` is the count of `diff --git` headers in the
/// structured file list. Untracked files are listed separately and never
/// added to the diff count: `untracked files: listed, not diffed (N: ...)`.
fn offline_review_fallback(
    diff: &str,
    model: Option<&str>,
    reason: &str,
    untracked: &[String],
) -> String {
    let mut files = 0usize;
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for line in diff.lines() {
        if line.starts_with("diff --git") {
            files += 1;
        } else if line.starts_with("+++") || line.starts_with("---") {
            // File headers, not content lines.
        } else if line.starts_with('+') {
            insertions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    let untracked_line = if untracked.is_empty() {
        "untracked files: listed, not diffed (0)".to_string()
    } else {
        let shown: Vec<&str> = untracked.iter().take(20).map(String::as_str).collect();
        let suffix = if untracked.len() > shown.len() {
            format!(", +{} more", untracked.len() - shown.len())
        } else {
            String::new()
        };
        format!(
            "untracked files: listed, not diffed ({}: {}{})",
            untracked.len(),
            shown.join(", "),
            suffix
        )
    };
    let mut end = diff.len().min(4000);
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    let excerpt = &diff[..end];
    let truncated = diff.len() > end;
    let model_line = match model {
        None => format!(
            "no model configured; defaulted to {} (emergency default). {reason}. Open /connect to create a connection, or set `model` via `codegg edit` / `-m provider/model`; run `codegg doctor` to check wiring.",
            EMERGENCY_DEFAULT_MODEL
        ),
        Some(model) => format!(
            "Configured model `{model}` is unreachable: {reason}. Open /connect to create a connection, or set `model` via `codegg edit` / `-m provider/model`."
        ),
    };
    format!(
        "## Code Review (offline fallback — no active LLM connection)\n\n\
         {model_line}\n\n\
         Diff stats: {files} file(s) changed, +{insertions}/-{deletions} lines, {} bytes.\n\
         {untracked_line}\n\n\
         Checklist:\n\
         - Confirm the change compiles and existing tests cover the touched paths.\n\
         - Look for functional issues (wrong branches, missing error handling).\n\
         - Look for performance issues (extra clones, unbounded loops).\n\
         - Look for style issues (naming, dead code, stale comments).\n\
         - Note anything needing a human decision as a suggestion, not a verdict.\n\n\
         Diff excerpt{}:\n{excerpt}",
        diff.len(),
        if truncated {
            " (truncated at 4000 chars)"
        } else {
            ""
        },
    )
}

/// Best-effort untracked-file listing via the status read path.
///
/// Parses `git status --short` stdout for `?? ` entries. Never fails the
/// fallback: any status error yields an empty list. Untracked entries are
/// reported separately and never added to the diff file count.
async fn untracked_files_for_root(root: &std::path::Path) -> Vec<String> {
    let svc = GitExecutionService::new();
    let result = svc
        .execute(&GitOperation::Status { short: true }, root)
        .await;
    match result {
        Ok(execution) => execution
            .stdout
            .lines()
            .filter_map(|line| line.strip_prefix("?? ").map(|p| p.trim().to_string()))
            .filter(|p| !p.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

impl ReviewTool {
    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        context: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let staged = input["staged"].as_bool().unwrap_or(true);
        let diff = self.get_diff_with_context(staged, context).await?;
        let provider_context = provider_request_context(context);
        match self
            .analyze_diff_with_context(&diff, provider_context)
            .await
        {
            Ok(review) => Ok(format!("## Code Review Results\n\n{}", review)),
            Err(e) => {
                let config = Config::load_or_default();
                let model_opt = config.model.clone();
                let root = self.effective_root(context);
                let untracked = untracked_files_for_root(&root).await;
                Ok(offline_review_fallback(
                    &diff,
                    model_opt.as_deref(),
                    &e.to_string(),
                    &untracked,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct ContextCaptureProvider {
        contexts: Arc<Mutex<Vec<Option<String>>>>,
    }

    #[async_trait]
    impl Provider for ContextCaptureProvider {
        fn id(&self) -> &str {
            "context-capture"
        }

        fn name(&self) -> &str {
            "Context Capture"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self {
                contexts: self.contexts.clone(),
            })
        }

        async fn stream(
            &self,
            request: &ChatRequest,
        ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
            self.contexts
                .lock()
                .unwrap()
                .push(request.context.session_id.as_deref().map(ToOwned::to_owned));
            Ok(Box::pin(futures_util::stream::iter(vec![Ok(
                ChatEvent::TextDelta(Arc::new("review".to_string())),
            )])))
        }

        async fn models(
            &self,
        ) -> Result<Vec<crate::provider::ModelInfo>, crate::provider::ProviderError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn nested_review_request_uses_tool_execution_session() {
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(ContextCaptureProvider {
            contexts: contexts.clone(),
        });
        let mut execution =
            ToolExecutionContext::with_backend(crate::tool::ToolBackendKind::Native);
        execution.session_id = Some("session-review".to_string());
        let tool = ReviewTool::new().with_provider(provider);

        let result = tool
            .analyze_diff_with_context(
                "diff --git a/src/lib.rs b/src/lib.rs",
                provider_request_context(Some(&execution)),
            )
            .await
            .expect("capture provider should return a review");

        assert_eq!(result, "review");
        assert_eq!(
            contexts.lock().unwrap().as_slice(),
            &[Some("session-review".to_string())]
        );
    }

    #[tokio::test]
    async fn structured_review_execution_preserves_tool_execution_session() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let run_git = |args: &[&str]| {
            // execution-ownership: test_only
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(workspace.path())
                .status()
                .expect("git should be installed");
            assert!(status.success(), "git command failed: {args:?}");
        };
        run_git(&["init", "-q"]);
        std::fs::write(workspace.path().join("lib.rs"), "pub fn test() {}\n")
            .expect("write staged file");
        run_git(&["add", "lib.rs"]);

        let contexts = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(ContextCaptureProvider {
            contexts: contexts.clone(),
        });
        let mut execution =
            ToolExecutionContext::with_backend(crate::tool::ToolBackendKind::Native);
        execution.session_id = Some("session-review-structured".to_string());
        let tool = ReviewTool::new()
            .with_workdir(workspace.path().to_path_buf())
            .with_provider(provider);

        tool.execute_structured(json!({"staged": true}), Some(execution))
            .await
            .expect("structured review should succeed");

        assert_eq!(
            contexts.lock().unwrap().as_slice(),
            &[Some("session-review-structured".to_string())]
        );
    }

    #[test]
    fn review_unknown_provider_points_at_connect() {
        let registry = ProviderRegistry::new();
        let err = match resolve_review_provider(&registry, "openai/gpt-4o") {
            Ok(_) => panic!("unknown provider must fail"),
            Err(err) => err,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("/connect"),
            "unknown provider must name /connect, got: {msg}"
        );
        assert!(msg.contains("openai"), "must name provider, got: {msg}");
    }

    #[test]
    fn review_model_not_found_error_is_actionable() {
        let api_err = crate::provider::ProviderError::api("404", "Model 'openai/gpt-4o' not found");
        assert_eq!(api_err.error_class(), "model_not_found");
        let mapped = map_llm_request_error(&api_err, "gpt-4o");
        let msg = mapped.to_string();
        assert!(
            msg.contains("-m provider/model") || msg.contains("/connect"),
            "model_not_found must carry fix guidance, got: {msg}"
        );
        assert!(
            msg.contains("codegg doctor"),
            "model_not_found must name doctor, got: {msg}"
        );
    }

    #[tokio::test]
    async fn review_without_provider_returns_offline_fallback() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let run_git = |args: &[&str]| {
            // execution-ownership: test_only
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(workspace.path())
                .status()
                .expect("git should be installed");
            assert!(status.success(), "git command failed: {args:?}");
        };
        run_git(&["init", "-q"]);
        std::fs::write(workspace.path().join("lib.rs"), "pub fn test() {}\n")
            .expect("write staged file");
        run_git(&["add", "lib.rs"]);

        // No with_provider: resolution fails in this checkout, so the tool
        // must return the offline fallback instead of an error.
        let tool = ReviewTool::new().with_workdir(workspace.path().to_path_buf());
        let output = tool
            .execute_with_context(json!({"staged": true}), None)
            .await
            .expect("offline fallback must succeed");
        let expected_model = Config::load_or_default()
            .model
            .clone()
            .unwrap_or_else(|| EMERGENCY_DEFAULT_MODEL.to_string());
        assert!(
            output.contains("offline fallback"),
            "fallback must be labelled offline, got: {output}"
        );
        assert!(
            output.contains(&expected_model),
            "fallback must echo the current model value, got: {output}"
        );
        assert!(
            output.contains("Diff stats:"),
            "fallback must carry diff stats, got: {output}"
        );
        assert!(
            output.contains("Checklist:"),
            "fallback must carry a review checklist, got: {output}"
        );
    }

    #[test]
    fn offline_fallback_lists_untracked_without_diffing_them() {
        let untracked = vec!["probe_untracked.txt".to_string()];
        let output = offline_review_fallback("", Some("openai/gpt-4o"), "no provider", &untracked);
        assert!(
            output.contains("0 file(s) changed"),
            "empty diff must count zero files, never force to one, got: {output}"
        );
        assert!(
            output.contains("untracked files: listed, not diffed (1: probe_untracked.txt)"),
            "untracked file must be listed separately, got: {output}"
        );
        assert!(
            !output.contains("1 file(s) changed"),
            "untracked entries must not inflate the diff count, got: {output}"
        );
    }

    #[test]
    fn offline_fallback_branches_on_model_config() {
        let untracked: Vec<String> = vec![];
        let with_model = offline_review_fallback(
            "diff --git a/b b/b",
            Some("openai/gpt-4o"),
            "no provider",
            &untracked,
        );
        assert!(
            with_model.contains("Configured model `openai/gpt-4o` is unreachable"),
            "configured model must keep the unreachable wording, got: {with_model}"
        );
        assert!(with_model.contains("offline fallback"));
        let without_model =
            offline_review_fallback("diff --git a/b b/b", None, "no provider", &untracked);
        assert!(
            without_model
                .contains("no model configured; defaulted to openai/gpt-4o (emergency default)"),
            "missing model must name the emergency default, got: {without_model}"
        );
        assert!(without_model.contains("/connect"));
        assert!(without_model.contains("offline fallback"));
    }
}
