use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::fmt::Write as _;

use crate::agent::EMERGENCY_DEFAULT_MODEL;
use crate::config::schema::Config;
use crate::error::ToolError;
use crate::git_service::{DiffFilePayload, DiffResultPayload, GitExecutionService, GitPayload};
use crate::provider::{
    register_builtin_with_config, ChatEvent, ChatRequest, ContentPart, Message, ProviderRegistry,
};
use crate::tool::{Tool, ToolCategory};

use codegg_git::GitOperation;

pub struct ReviewTool {
    workdir: std::path::PathBuf,
}

impl ReviewTool {
    pub fn new() -> Self {
        Self {
            workdir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        }
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

    async fn analyze_diff(&self, diff: &str) -> Result<String, ToolError> {
        let config = Config::load().unwrap_or_default();
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);

        let model = config
            .model
            .clone()
            .unwrap_or_else(|| EMERGENCY_DEFAULT_MODEL.to_string());

        let provider = registry
            .get(&model)
            .or_else(|| registry.list().first().copied())
            .ok_or_else(|| ToolError::Execution("no provider available".to_string()))?;

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
            model: model.clone(),
            tools: None,
            system: Some("You are a professional code reviewer. Provide constructive, concise feedback using emojis.".to_string()),
            temperature: Some(0.3),
            top_p: None,
            max_tokens: Some(1000),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
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
        let staged = input["staged"].as_bool().unwrap_or(true);
        let diff = self.get_diff(staged).await?;
        let review = self.analyze_diff(&diff).await?;
        Ok(format!("## Code Review Results\n\n{}", review))
    }
}
