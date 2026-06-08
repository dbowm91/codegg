use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use tokio::process::Command;

use crate::config::schema::Config;
use crate::error::ToolError;
use crate::provider::{
    register_builtin_with_config, ChatEvent, ChatRequest, ContentPart, Message, ProviderRegistry,
};
use crate::tool::{Tool, ToolCategory};

pub struct CommitTool {
    workdir: std::path::PathBuf,
}

impl CommitTool {
    pub fn new() -> Self {
        Self {
            workdir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        }
    }

    pub fn with_workdir(mut self, dir: std::path::PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    async fn get_diff(&self, all: bool) -> Result<String, ToolError> {
        let args = if all {
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
        let output = cmd
            .args(&args)
            .current_dir(&self.workdir)
            .output()
            .await
            .map_err(|e| ToolError::Execution(format!("git diff failed: {}", e)))?;

        if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
            return Err(ToolError::Execution("no diff available".to_string()));
        }

        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        if diff.is_empty() {
            return Err(ToolError::Execution("no changes to commit".to_string()));
        }

        Ok(diff)
    }

    async fn stage_all(&self) -> Result<(), ToolError> {
        let mut cmd = Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        cmd.kill_on_drop(true);
        let output = cmd
            .args(&["add", "-A"])
            .current_dir(&self.workdir)
            .output()
            .await
            .map_err(|e| ToolError::Execution(format!("git add failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolError::Execution(format!("git add failed: {}", stderr)));
        }

        Ok(())
    }

    async fn generate_commit_message(&self, diff: &str) -> Result<String, ToolError> {
        let config = Config::load().unwrap_or_default();
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);

        let model = config.model.unwrap_or_else(|| "openai/gpt-4".to_string());

        let provider = registry
            .get(&model)
            .or_else(|| registry.list().first().copied())
            .ok_or_else(|| ToolError::Execution("no provider available".to_string()))?;

        let prompt = format!(
            "Generate a concise git commit message for the following diff. \
             Follow conventional commit format (type: description). \
             Be specific but brief. Return ONLY the commit message, nothing else.\n\nDiff:\n{}",
            diff
        );

        let request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: prompt.into(),
                }],
            }],
            model: model.clone(),
            tools: None,
            system: Some(
                "You are a git commit message generator. Return only the commit message."
                    .to_string(),
            ),
            temperature: Some(0.3),
            top_p: None,
            max_tokens: Some(200),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
        };

        let mut stream = provider
            .stream(&request)
            .await
            .map_err(|e| ToolError::Execution(format!("LLM request failed: {}", e)))?;

        let mut message = String::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(ChatEvent::TextDelta(delta)) => message.push_str(&delta),
                Ok(ChatEvent::Finish { .. }) => break,
                Ok(ChatEvent::Error(e)) => {
                    return Err(ToolError::Execution(format!("LLM error: {}", e)))
                }
                _ => {}
            }
        }

        let message = message.trim();
        if message.is_empty() {
            return Err(ToolError::Execution(
                "LLM generated empty commit message".to_string(),
            ));
        }

        Ok(message.to_string())
    }

    async fn run_commit(
        &self,
        message: &str,
        co_authored: bool,
        amend: bool,
    ) -> Result<String, ToolError> {
        let mut args = vec!["commit"];

        let full_message = if co_authored {
            format!("{}\n\nCo-Authored-By: Codegg AI <codegg@ai>", message)
        } else {
            message.to_string()
        };

        args.push("-m");
        args.push(&full_message);

        if amend {
            args.push("--amend");
        }

        let mut cmd = Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        cmd.kill_on_drop(true);
        let output = cmd
            .args(&args)
            .current_dir(&self.workdir)
            .output()
            .await
            .map_err(|e| ToolError::Execution(format!("git commit failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Committed: {}", stdout.trim()))
        } else {
            Err(ToolError::Execution(format!(
                "commit failed: {} {}",
                stdout, stderr
            )))
        }
    }
}

impl Default for CommitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CommitTool {
    fn name(&self) -> &str {
        "commit"
    }

    fn description(&self) -> &str {
        "Generate a commit message from git diff and commit changes"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "all": {
                    "type": "boolean",
                    "description": "Stage all changes before committing (default: false)"
                },
                "message": {
                    "type": "string",
                    "description": "Optional: provide a manual commit message (skips generation)"
                },
                "co_authored": {
                    "type": "boolean",
                    "description": "Add Co-Authored-By with AI agent info"
                },
                "amend": {
                    "type": "boolean",
                    "description": "Amend the previous commit (default: false)"
                },
                "allow_amend": {
                    "type": "boolean",
                    "description": "Required safety acknowledgement when amend=true"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let all = input["all"].as_bool().unwrap_or(false);
        let amend = input["amend"].as_bool().unwrap_or(false);
        let allow_amend = input["allow_amend"].as_bool().unwrap_or(false);
        let co_authored = input["co_authored"].as_bool().unwrap_or(false);
        let manual_message = input["message"].as_str();

        if amend && !allow_amend {
            return Err(ToolError::Execution(
                "amend requires explicit allow_amend=true acknowledgement".to_string(),
            ));
        }

        if all {
            self.stage_all().await?;
        }

        let diff = self.get_diff(all || !amend).await?;

        let message = if let Some(msg) = manual_message {
            msg.to_string()
        } else {
            self.generate_commit_message(&diff).await?
        };

        self.run_commit(&message, co_authored, amend).await
    }
}
