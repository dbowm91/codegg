use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PlanEnterInput {
    #[serde(default)]
    topic: Option<String>,
}

pub struct PlanEnterTool;

#[async_trait]
impl Tool for PlanEnterTool {
    fn name(&self) -> &str {
        "plan_enter"
    }

    fn description(&self) -> &str {
        "Enter plan mode. While in plan mode, mutating tools (edit, write, etc.) are hidden; you have access to read-only inspection tools, todowrite/todoread (recommended planning surface), and read-only bash. Use plan_exit to switch back to build mode."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Optional topic or focus for the planning session"
                }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: PlanEnterInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid plan_enter input: {e}")))?;

        let topic = parsed.topic.as_deref().unwrap_or("general planning");

        Ok(format!(
            "__PLAN_MODE_ENTER__:{topic}\n\n\
            In plan mode you can:\n\
            - Read files and analyze code (read, glob, grep, list, codesearch, webfetch, lsp, skill)\n\
            - Use todowrite to record plan steps (this is the recommended planning surface)\n\
            - Use todoread to inspect existing todos\n\
            - Run read-only bash commands (ls, cat, grep, git status, cargo check, etc.)\n\n\
            You CANNOT:\n\
            - Edit, write, or modify source files\n\
            - Run mutating shell commands (rm, mv, install scripts, etc.)\n\
            - Spawn subagents that modify state\n\n\
            Use plan_exit when ready to switch back to build mode."
        ))
    }
}

#[derive(Debug, Deserialize)]
struct PlanExitInput {
    #[serde(default)]
    plan_file: Option<String>,
}

pub struct PlanExitTool;

#[async_trait]
impl Tool for PlanExitTool {
    fn name(&self) -> &str {
        "plan_exit"
    }

    fn description(&self) -> &str {
        "Experimental: Exit plan mode and switch to build agent. Optionally specify a plan file to use."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "plan_file": {
                    "type": "string",
                    "description": "Optional path to the plan file to use for building"
                }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: PlanExitInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid plan_exit input: {e}")))?;

        let plan_info = match &parsed.plan_file {
            Some(file) => {
                let path = std::path::Path::new(file);
                if path.exists() {
                    let content = tokio::fs::read_to_string(path)
                        .await
                        .unwrap_or_else(|_| "[could not read plan file]".to_string());
                    format!("\nPlan file ({file}):\n{content}")
                } else {
                    format!("\nWarning: Plan file '{file}' not found")
                }
            }
            None => String::from("\nNo plan file specified."),
        };

        Ok(format!(
            "__PLAN_MODE_EXIT__\n\nExiting plan mode. Switching to build agent.{plan_info}"
        ))
    }
}

pub fn detect_plan_mode_change(output: &str) -> Option<PlanModeChange> {
    if let Some(topic) = output.strip_prefix("__PLAN_MODE_ENTER__:") {
        let topic = topic.lines().next().unwrap_or("").trim();
        Some(PlanModeChange::Enter(if topic.is_empty() {
            None
        } else {
            Some(topic.to_string())
        }))
    } else if output.starts_with("__PLAN_MODE_EXIT__") {
        Some(PlanModeChange::Exit)
    } else {
        None
    }
}

pub enum PlanModeChange {
    Enter(Option<String>),
    Exit,
}
