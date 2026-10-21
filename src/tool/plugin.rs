//! Adapter from an activated plugin tool descriptor into the canonical Tool
//! trait.  Runtime dispatch remains exclusively in PluginService.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::error::ToolError;
use crate::plugin::registry::PluginToolRegistration;
use crate::plugin::service::PluginService;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};

#[derive(Clone)]
pub struct PluginToolAdapter {
    descriptor: PluginToolRegistration,
    service: Arc<PluginService>,
}

impl PluginToolAdapter {
    pub fn new(descriptor: PluginToolRegistration, service: Arc<PluginService>) -> Self {
        Self {
            descriptor,
            service,
        }
    }
}

#[async_trait]
impl Tool for PluginToolAdapter {
    #[allow(clippy::misnamed_getters)]
    fn name(&self) -> &str {
        &self.descriptor.canonical_name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.descriptor.input_schema.clone()
    }

    fn category(&self) -> ToolCategory {
        // Plugin hints never lower host permission requirements.
        ToolCategory::Mutating
    }

    fn defer_loading(&self) -> bool {
        true
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let response = self
            .service
            .invoke_tool(&self.descriptor.canonical_name, input)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        if !response.ok {
            return Err(ToolError::Execution(
                response
                    .diagnostics
                    .first()
                    .map(|diagnostic| diagnostic.message.clone())
                    .unwrap_or_else(|| "plugin tool failed".into()),
            ));
        }
        Ok(response.data.to_string())
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        // M005 C005: a process-backed plugin is a process surface, and the
        // restricted shared-checkout profile refuses it before the plugin
        // runs. The classification is the plugin's *runtime kind*, not its
        // advertised metadata: a plugin that describes itself as read-only
        // cannot opt a process back in.
        // c005-denial: process-backed-plugin
        if self
            .service
            .is_process_backed(&self.descriptor.plugin_id)
            .await
        {
            crate::tool::process_admission::admit_process_for_context(
                ctx.as_ref(),
                codegg_core::process_admission::ProcessKind::PluginProcess,
            )
            .await?;
        }
        let response = self
            .service
            .invoke_tool(&self.descriptor.canonical_name, input)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let output = response.data.to_string();
        Ok(StructuredToolResult::with_value(
            output,
            response.data,
            response.ok,
            Some(crate::tool::ToolProvenance {
                backend: "native".into(),
                implementation: format!("plugin:{}", self.descriptor.plugin_id),
                version: None,
                elapsed_ms: None,
                truncated: response
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("truncated")),
                trust: crate::tool::ToolTrust::MutatingSideEffect,
            }),
        ))
    }
}

pub fn descriptor_metadata(descriptor: &PluginToolRegistration) -> serde_json::Value {
    json!({
        "plugin_id": descriptor.plugin_id,
        "canonical_name": descriptor.canonical_name,
        "name": descriptor.name,
        "effect_hint": descriptor.effect_hint,
        "disclosure": "deferred",
        "schema_available": true
    })
}
