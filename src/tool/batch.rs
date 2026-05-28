use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::tool::{Tool, ToolRegistry};
use std::sync::Arc;

const MAX_CALL_INPUT_SIZE: usize = 100_000;
const MAX_BATCH_OUTPUT_SIZE: usize = 500_000;

pub struct BatchTool {
    registry: Arc<ToolRegistry>,
}

impl BatchTool {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

impl Default for BatchTool {
    fn default() -> Self {
        Self::new(Arc::new(ToolRegistry::new()))
    }
}

#[async_trait]
impl Tool for BatchTool {
    fn name(&self) -> &str {
        "batch"
    }

    fn description(&self) -> &str {
        "Execute multiple tool calls in parallel (up to 25)"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": { "type": "string" },
                            "input": { "type": "object" }
                        },
                        "required": ["tool", "input"]
                    },
                    "description": "List of tool calls to execute in parallel"
                }
            },
            "required": ["calls"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let calls = input["calls"]
            .as_array()
            .ok_or_else(|| ToolError::Execution("missing 'calls' parameter".to_string()))?;

        if calls.is_empty() {
            return Err(ToolError::Execution(
                "'calls' must not be empty".to_string(),
            ));
        }

        if calls.len() > 25 {
            return Err(ToolError::Execution(
                "maximum 25 parallel calls allowed".to_string(),
            ));
        }

        let mut futures = Vec::new();
        for call in calls {
            let tool_name = call["tool"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("call missing 'tool' field".to_string()))?
                .to_string();

            if tool_name.is_empty() || tool_name.len() > 100 {
                return Err(ToolError::Execution("invalid tool name length".to_string()));
            }

            if !tool_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(ToolError::Execution(
                    "tool name contains invalid characters".to_string(),
                ));
            }

            let tool_input = call["input"].clone();
            let input_size = tool_input.to_string().len();
            if input_size > MAX_CALL_INPUT_SIZE {
                return Err(ToolError::Execution(format!(
                    "call input exceeds maximum size of {} bytes",
                    MAX_CALL_INPUT_SIZE
                )));
            }

            if tool_name.starts_with("mcp__") {
                return Err(ToolError::Execution(format!(
                    "MCP tool '{}' cannot be used through the batch tool (MCP tools are dispatched separately)",
                    tool_name
                )));
            }

            let registry = self.registry.clone();
            let fut = async move {
                let tool = registry
                    .get(&tool_name)
                    .ok_or_else(|| ToolError::NotFound(tool_name.clone()))?;

                let result = tool.execute(tool_input).await;
                Ok::<(String, Result<String, ToolError>), ToolError>((tool_name, result))
            };
            futures.push(fut);
        }

        let results = futures::future::join_all(futures).await;

        let mut output = String::from("Batch results:\n");
        for (i, result) in results.iter().enumerate() {
            if output.len() > MAX_BATCH_OUTPUT_SIZE {
                output.push_str(&format!(
                    "\n... [output truncated, {} results omitted] ...\n",
                    results.len() - i
                ));
                break;
            }
            match result {
                Ok((name, Ok(content))) => {
                    let content = if content.len() > 10000 {
                        format!(
                            "{}... [{} chars truncated]",
                            &content[..10000],
                            content.len() - 10000
                        )
                    } else {
                        content.to_string()
                    };
                    output.push_str(&format!("{}. [{}] {}\n", i + 1, name, content));
                }
                Ok((name, Err(e))) => {
                    output.push_str(&format!("{}. [{}] Error: {}\n", i + 1, name, e));
                }
                Err(e) => {
                    output.push_str(&format!("{}. Error: {}\n", i + 1, e));
                }
            }
        }

        Ok(output)
    }
}
