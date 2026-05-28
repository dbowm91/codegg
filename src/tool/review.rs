use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use tokio::process::Command;

use crate::config::schema::Config;
use crate::error::ToolError;
use crate::provider::{
    register_builtin_with_config, ChatEvent, ChatRequest, ContentPart, Message, ProviderRegistry,
};
use crate::tool::Tool;

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
        let args = if staged {
            vec!["diff", "--staged"]
        } else {
            vec!["diff", "HEAD"]
        };

        let mut cmd = Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        cmd.kill_on_drop(true);
        let output = cmd.args(&args)
            .current_dir(&self.workdir)
            .output()
            .await
            .map_err(|e| ToolError::Execution(format!("git diff failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolError::Execution(format!("git diff failed: {}", stderr)));
        }

        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        if diff.is_empty() {
            return Err(ToolError::Execution("no changes to review".to_string()));
        }

        Ok(diff)
    }

    async fn analyze_diff(&self, diff: &str) -> Result<String, ToolError> {
        let config = Config::load().unwrap_or_default();
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);

        let model = config
            .model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o".to_string());

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

#[async_trait]
impl Tool for ReviewTool {
    fn name(&self) -> &str {
        "review"
    }

    fn description(&self) -> &str {
        "Analyze git diff and provide structured code review feedback"
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

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let staged = input["staged"].as_bool().unwrap_or(true);
        let diff = self.get_diff(staged).await?;
        let review = self.analyze_diff(&diff).await?;
        Ok(format!("## Code Review Results\n\n{}", review))
    }
}
