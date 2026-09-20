//! Read-only extension catalog discovery. Installation is host-mediated.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::error::ToolError;
use crate::plugin::marketplace::MarketplaceService;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};

pub struct ExtensionSearchTool {
    service: Arc<MarketplaceService>,
}

impl ExtensionSearchTool {
    pub fn new(service: Arc<MarketplaceService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl Tool for ExtensionSearchTool {
    fn name(&self) -> &str {
        "extension_search"
    }
    fn description(&self) -> &str {
        "Search the resolved extension catalog. Results are metadata only; installation requires an explicit host action."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type":"object","properties":{"query":{"type":"string","maxLength":256},"component":{"type":"string","enum":["skill","mcp","tool","browser"]},"limit":{"type":"integer","minimum":1,"maximum":16}},"required":["query"]})
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn defer_loading(&self) -> bool {
        true
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        Ok(self.execute_structured(input, None).await?.output)
    }
    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let object = input.as_object().ok_or_else(|| {
            ToolError::Execution("extension_search input must be an object".into())
        })?;
        let query = object
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("query is required".into()))?;
        if query.len() > 256 {
            return Err(ToolError::Execution("query exceeds 256 bytes".into()));
        }
        let component = object.get("component").and_then(|v| v.as_str());
        if component.is_some_and(|value| !matches!(value, "skill" | "mcp" | "tool" | "browser")) {
            return Err(ToolError::Execution("unsupported component filter".into()));
        }
        let limit = object
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 16) as usize;
        let entries = self
            .service
            .search_extensions(query, component, limit)
            .await;
        let value = serde_json::Value::Array(entries.iter().map(|entry| json!({
            "id": entry.id, "name": entry.name, "version": entry.version,
            "description": entry.description, "format": entry.format,
            "components": entry.components, "prerequisites": entry.prerequisites,
            "security_notes": entry.security_notes, "source": {"id": entry.source_id, "tier": entry.tier.to_string()},
            "install_requires_host_action": true,
        })).collect());
        Ok(StructuredToolResult::with_value(
            value.to_string(),
            value,
            true,
            Some(crate::tool::ToolProvenance {
                backend: "native".into(),
                implementation: "extension-catalog".into(),
                version: Some("1".into()),
                elapsed_ms: None,
                truncated: false,
                trust: crate::tool::ToolTrust::ExternalUntrusted,
            }),
        ))
    }
}
