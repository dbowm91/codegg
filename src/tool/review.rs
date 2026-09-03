use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use std::fmt::Write as _;

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
            workdir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
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

    async fn get_diff(&self, staged: bool) -> Result<String, ToolError> {
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
        let result = svc
            .execute(&operation, &self.workdir)
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
            return analyze_diff_with_provider(diff, provider, &model, context).await;
        }

        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        let provider = registry
            .get(&model)
            .or_else(|| registry.list().first().copied())
            .ok_or_else(|| ToolError::Execution("no provider available".to_string()))?;

        analyze_diff_with_provider(diff, provider, &model, context).await
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
        .map_err(|e| ToolError::Execution(format!("LLM request failed: {}", e)))?;

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
        "Read-only tool that analyzes git diff and provides structured code review feedback"
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

impl ReviewTool {
    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        context: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let staged = input["staged"].as_bool().unwrap_or(true);
        let diff = self.get_diff(staged).await?;
        let provider_context = provider_request_context(context);
        let review = self
            .analyze_diff_with_context(&diff, provider_context)
            .await?;
        Ok(format!("## Code Review Results\n\n{}", review))
    }
}
