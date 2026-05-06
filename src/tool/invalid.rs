use crate::error::ToolError;
use crate::tool::Tool;
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct InvalidInput {
    tool_name: String,
    error: String,
}

pub struct InvalidTool;

#[async_trait]
impl Tool for InvalidTool {
    fn name(&self) -> &str {
        "invalid"
    }

    fn description(&self) -> &str {
        "Catch-all for malformed tool calls. Returns the tool name and error message."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool that was attempted"
                },
                "error": {
                    "type": "string",
                    "description": "Error message describing what went wrong"
                }
            },
            "required": ["tool_name", "error"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: InvalidInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid tool input: {e}")))?;

        Ok(format!(
            "Tool call failed: '{}' - {}",
            parsed.tool_name, parsed.error
        ))
    }
}
