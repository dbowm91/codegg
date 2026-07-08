use std::time::Instant;

use serde_json::Value;

use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolProvenance, ToolTrust};

/// Configuration for the eggsact runtime.
#[derive(Debug, Clone)]
pub struct EggsactConfig {
    /// Profile name (e.g. "codegg_core_min", "default", "full").
    pub profile: String,
    /// Tool audience ("model" or "harness").
    pub audience: String,
    /// Maximum output characters before truncation.
    pub max_output_chars: usize,
}

impl Default for EggsactConfig {
    fn default() -> Self {
        Self {
            profile: "codegg_core".to_string(),
            audience: "model".to_string(),
            max_output_chars: 12_000,
        }
    }
}

/// In-process eggsact runtime wrapping `eggsact::agent::ToolRegistry`.
pub struct EggsactRuntime {
    registry: eggsact::agent::ToolRegistry,
    config: EggsactConfig,
}

impl EggsactRuntime {
    /// Create a new runtime with the given configuration.
    pub fn new(config: EggsactConfig) -> Result<Self, ToolError> {
        let profile = eggsact::agent::Profile::from_str_opt(&config.profile)
            .unwrap_or(eggsact::agent::Profile::Default);
        let audience = match config.audience.to_lowercase().as_str() {
            "harness" => eggsact::agent::ToolAudience::Harness,
            _ => eggsact::agent::ToolAudience::Model,
        };
        let registry = eggsact::agent::ToolRegistry::with_profile_and_audience(profile, audience);
        Ok(Self { registry, config })
    }

    /// Call an eggsact tool by name with JSON arguments.
    pub fn call_json(&self, tool: &str, args: Value) -> Result<EggsactCallResult, ToolError> {
        let start = Instant::now();
        let response = self
            .registry
            .call_json(tool, args)
            .map_err(|e| ToolError::Execution(format!("eggsact tool call failed: {e}")))?;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        let output = format_response(&response, self.config.max_output_chars);
        let truncated = output.len() >= self.config.max_output_chars;

        Ok(EggsactCallResult {
            output,
            success: response.ok,
            elapsed_ms,
            truncated,
            machine_code: response.machine_code.clone(),
        })
    }

    /// Check if a tool is available in the current profile/audience.
    pub fn has_tool(&self, tool: &str) -> bool {
        self.registry.has_tool(tool)
    }

    /// Get the configuration.
    pub fn config(&self) -> &EggsactConfig {
        &self.config
    }
}

/// Result of an eggsact tool call.
pub struct EggsactCallResult {
    pub output: String,
    pub success: bool,
    pub elapsed_ms: u64,
    pub truncated: bool,
    pub machine_code: Option<String>,
}

/// Build a `StructuredToolResult` from an eggsact call result.
pub fn to_structured_result(tool_name: &str, result: EggsactCallResult) -> StructuredToolResult {
    let provenance = ToolProvenance {
        backend: "native".to_string(),
        implementation: format!("eggsact/{tool_name}"),
        version: None,
        elapsed_ms: Some(result.elapsed_ms),
        truncated: result.truncated,
        trust: ToolTrust::LocalTrusted,
    };
    StructuredToolResult::with_provenance(result.output, result.success, provenance)
}

/// Format an eggsact `ToolResponse` into a deterministic text envelope.
fn format_response(response: &eggsact::mcp::response::ToolResponse, max_chars: usize) -> String {
    let mut parts = Vec::new();

    parts.push(format!("ok: {}", response.ok));

    if let Some(ref mc) = response.machine_code {
        parts.push(format!("machine_code: {mc}"));
    }

    if let Some(ref result) = response.result {
        parts.push(format!("result: {result}"));
    }

    if let Some(ref findings) = response.findings {
        if !findings.is_empty() {
            parts.push(format!(
                "findings: {}",
                serde_json::to_string_pretty(findings).unwrap_or_default()
            ));
        }
    }

    if let Some(ref limits) = response.limits_applied {
        if !limits.is_empty() {
            parts.push(format!("limits_applied: {}", limits.join(", ")));
        }
    }

    let output = parts.join("\n");

    if output.len() > max_chars {
        let truncated = &output[..max_chars];
        format!("{truncated}\n... [truncated at {max_chars} chars]")
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_response(result: Option<serde_json::Value>) -> eggsact::mcp::response::ToolResponse {
        eggsact::mcp::response::ToolResponse {
            ok: true,
            tool: None,
            result,
            error_type: None,
            error: None,
            hints: None,
            warnings: None,
            limits_applied: None,
            findings: None,
            machine_code: None,
            recommended_next_tool: None,
        }
    }

    #[test]
    fn format_response_ok_with_result() {
        let response = ok_response(Some(serde_json::json!("hello")));
        let output = format_response(&response, 1000);
        assert!(output.contains("ok: true"));
        assert!(output.contains("result:"));
        assert!(output.contains("hello"));
    }

    #[test]
    fn format_response_with_machine_code() {
        let mut response = ok_response(None);
        response.machine_code = Some("JSON_PARSE_ERROR".to_string());
        let output = format_response(&response, 1000);
        assert!(output.contains("machine_code: JSON_PARSE_ERROR"));
    }

    #[test]
    fn format_response_truncates_long_output() {
        let long_result = "x".repeat(500);
        let response = ok_response(Some(serde_json::Value::String(long_result)));
        let output = format_response(&response, 100);
        assert!(output.len() < 200);
        assert!(output.contains("truncated"));
    }

    #[test]
    fn to_structured_result_has_correct_provenance() {
        let result = EggsactCallResult {
            output: "test output".to_string(),
            success: true,
            elapsed_ms: 42,
            truncated: false,
            machine_code: None,
        };
        let structured = to_structured_result("text_equal", result);
        assert!(structured.success);
        let prov = structured.provenance.unwrap();
        assert_eq!(prov.backend, "native");
        assert_eq!(prov.implementation, "eggsact/text_equal");
        assert_eq!(prov.trust, ToolTrust::LocalTrusted);
        assert_eq!(prov.elapsed_ms, Some(42));
        assert!(!prov.truncated);
    }

    #[test]
    fn eggsact_config_default_values() {
        let config = EggsactConfig::default();
        assert_eq!(config.profile, "codegg_core");
        assert_eq!(config.audience, "model");
        assert_eq!(config.max_output_chars, 12_000);
    }
}
