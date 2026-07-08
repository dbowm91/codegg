use async_trait::async_trait;
use serde_json::json;

use crate::eggsact::adapter::EggsactRuntime;
use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext};
use crate::tool::{Tool, ToolCategory};

/// Wrapper around eggsact `text_equal` tool.
pub struct DeterministicTextEqual {
    runtime: std::sync::Arc<EggsactRuntime>,
}

impl DeterministicTextEqual {
    pub fn new(runtime: std::sync::Arc<EggsactRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for DeterministicTextEqual {
    fn name(&self) -> &str {
        "deterministic_text_equal"
    }

    fn description(&self) -> &str {
        "Check if two text strings are equal (eggsact-backed, deterministic)"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "a": {
                    "type": "string",
                    "description": "First text string"
                },
                "b": {
                    "type": "string",
                    "description": "Second text string"
                }
            },
            "required": ["a", "b"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn expose_in_definitions(&self) -> bool {
        false // Hidden until Phase 4
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let result = self.runtime.call_json("text_equal", input)?;
        Ok(result.output)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let result = self.runtime.call_json("text_equal", input)?;
        Ok(crate::eggsact::adapter::to_structured_result(
            "text_equal",
            result,
        ))
    }
}

/// Wrapper around eggsact `validate_json` tool.
pub struct DeterministicValidateJson {
    runtime: std::sync::Arc<EggsactRuntime>,
}

impl DeterministicValidateJson {
    pub fn new(runtime: std::sync::Arc<EggsactRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for DeterministicValidateJson {
    fn name(&self) -> &str {
        "deterministic_validate_json"
    }

    fn description(&self) -> &str {
        "Validate JSON syntax and structure (eggsact-backed, deterministic)"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "JSON text to validate"
                }
            },
            "required": ["text"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn expose_in_definitions(&self) -> bool {
        false // Hidden until Phase 4
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let result = self.runtime.call_json("validate_json", input)?;
        Ok(result.output)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let result = self.runtime.call_json("validate_json", input)?;
        Ok(crate::eggsact::adapter::to_structured_result(
            "validate_json",
            result,
        ))
    }
}
