use std::time::Instant;

use serde_json::Value;

use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolProvenance, ToolTrust};

/// Result of a UTF-8 safe truncation operation.
pub struct TruncatedText {
    pub text: String,
    pub truncated: bool,
}

/// Truncate a string to at most `max_chars` characters without splitting
/// multibyte UTF-8 sequences. If the marker fits within the limit, it is
/// appended; otherwise the marker is appended anyway (overflow is acceptable
/// when the limit is very small).
pub fn truncate_utf8_safe(input: &str, max_chars: usize, marker: &str) -> TruncatedText {
    let char_count = input.chars().count();
    if char_count <= max_chars {
        return TruncatedText {
            text: input.to_string(),
            truncated: false,
        };
    }

    let truncated_text: String = input.chars().take(max_chars).collect();

    let mut result = truncated_text;
    if !marker.is_empty() {
        let marker_chars: usize = marker.chars().count();
        if marker_chars <= max_chars {
            result = input.chars().take(max_chars - marker_chars).collect();
            result.push_str(marker);
        } else {
            result.push_str(marker);
        }
    }

    TruncatedText {
        text: result,
        truncated: true,
    }
}

/// The return type of `format_response`, bundling output text with a
/// truncation indicator so callers can surface the fact reliably.
pub struct FormattedEggsactResponse {
    pub output: String,
    pub truncated: bool,
}

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

        let formatted = format_response(&response, self.config.max_output_chars);
        Ok(EggsactCallResult {
            output: formatted.output,
            success: response.ok,
            elapsed_ms,
            truncated: formatted.truncated,
            machine_code: response.machine_code.clone(),
            result: response.result.clone(),
            findings: response
                .findings
                .as_ref()
                .and_then(|f| serde_json::to_value(f).ok()),
            warnings: response
                .warnings
                .as_ref()
                .and_then(|w| serde_json::to_value(w).ok()),
            error_type: response.error_type.clone(),
            error: response.error.clone(),
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
    /// Raw JSON result from the tool (e.g. match count, verdict).
    pub result: Option<serde_json::Value>,
    /// Structured findings from the tool response.
    pub findings: Option<serde_json::Value>,
    /// Warnings from the tool response.
    pub warnings: Option<serde_json::Value>,
    /// Error type if the tool returned an error.
    pub error_type: Option<String>,
    /// Error message if the tool returned an error.
    pub error: Option<String>,
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
fn format_response(
    response: &eggsact::mcp::response::ToolResponse,
    max_chars: usize,
) -> FormattedEggsactResponse {
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
    let marker = format!("\n... [truncated at {max_chars} chars]");
    let truncated = truncate_utf8_safe(&output, max_chars, &marker);

    FormattedEggsactResponse {
        output: truncated.text,
        truncated: truncated.truncated,
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
        let formatted = format_response(&response, 1000);
        assert!(formatted.output.contains("ok: true"));
        assert!(formatted.output.contains("result:"));
        assert!(formatted.output.contains("hello"));
        assert!(!formatted.truncated);
    }

    #[test]
    fn format_response_with_machine_code() {
        let mut response = ok_response(None);
        response.machine_code = Some("JSON_PARSE_ERROR".to_string());
        let formatted = format_response(&response, 1000);
        assert!(formatted.output.contains("machine_code: JSON_PARSE_ERROR"));
        assert!(!formatted.truncated);
    }

    #[test]
    fn format_response_truncates_long_output() {
        let long_result = "x".repeat(500);
        let response = ok_response(Some(serde_json::Value::String(long_result)));
        let formatted = format_response(&response, 100);
        assert!(formatted.truncated);
        assert!(formatted.output.contains("truncated"));
    }

    #[test]
    fn format_response_at_limit_not_truncated() {
        let result_str = "a".repeat(90);
        let response = ok_response(Some(serde_json::Value::String(result_str)));
        // Output includes "ok: true\nresult: \"aaa...\"" ~ 112 chars, so use 200
        let formatted = format_response(&response, 200);
        assert!(!formatted.truncated);
    }

    #[test]
    fn to_structured_result_has_correct_provenance() {
        let result = EggsactCallResult {
            output: "test output".to_string(),
            success: true,
            elapsed_ms: 42,
            truncated: false,
            machine_code: None,
            result: None,
            findings: None,
            warnings: None,
            error_type: None,
            error: None,
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

    #[test]
    fn call_result_has_structured_fields() {
        let result = EggsactCallResult {
            output: "ok: true".to_string(),
            success: true,
            elapsed_ms: 10,
            truncated: false,
            machine_code: None,
            result: Some(serde_json::json!({"match_count": 1})),
            findings: Some(serde_json::json!([{"severity": "warn"}])),
            warnings: Some(serde_json::json!(["low memory"])),
            error_type: None,
            error: None,
        };
        assert!(result.result.is_some());
        assert_eq!(result.result.unwrap()["match_count"], 1);
        assert!(result.findings.is_some());
        assert!(result.warnings.is_some());
        assert!(result.error_type.is_none());
        assert!(result.error.is_none());
    }

    #[test]
    fn call_result_structured_fields_default_to_none() {
        let result = EggsactCallResult {
            output: "ok: true".to_string(),
            success: true,
            elapsed_ms: 10,
            truncated: false,
            machine_code: None,
            result: None,
            findings: None,
            warnings: None,
            error_type: None,
            error: None,
        };
        assert!(result.result.is_none());
        assert!(result.findings.is_none());
        assert!(result.warnings.is_none());
        assert!(result.error_type.is_none());
        assert!(result.error.is_none());
    }

    // --- truncate_utf8_safe tests ---

    #[test]
    fn truncate_utf8_safe_under_limit_not_truncated() {
        let result = truncate_utf8_safe("hello", 10, "...");
        assert_eq!(result.text, "hello");
        assert!(!result.truncated);
    }

    #[test]
    fn truncate_utf8_safe_at_limit_not_truncated() {
        let result = truncate_utf8_safe("hello", 5, "...");
        assert_eq!(result.text, "hello");
        assert!(!result.truncated);
    }

    #[test]
    fn truncate_utf8_safe_over_limit_truncated() {
        let result = truncate_utf8_safe("hello world", 5, "...");
        assert!(result.truncated);
        // Marker (3 chars) is subtracted from budget: take 5-3=2 chars, then append marker
        assert_eq!(result.text, "he...");
    }

    #[test]
    fn truncate_utf8_safe_multibyte_chars() {
        let input = "🌍🌎🌏";
        assert_eq!(input.chars().count(), 3);
        let result = truncate_utf8_safe(input, 2, "...");
        assert!(result.truncated);
        assert_eq!(result.text.chars().count(), 5);
        assert!(result.text.starts_with("🌍"));
    }

    #[test]
    fn truncate_utf8_safe_does_not_panic_on_multibyte_boundary() {
        let input = "ñéñéñé";
        assert_eq!(input.chars().count(), 6);
        let result = truncate_utf8_safe(input, 3, "...");
        assert!(result.truncated);
        assert_eq!(result.text, "...");
        assert!(std::str::from_utf8(result.text.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_utf8_safe_combining_marks() {
        let input = "e\u{0301}e\u{0301}e\u{0301}";
        assert_eq!(input.chars().count(), 6);
        let result = truncate_utf8_safe(input, 3, "...");
        assert!(result.truncated);
        assert!(std::str::from_utf8(result.text.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_utf8_safe_small_cap() {
        let input = "hello world";
        let result = truncate_utf8_safe(input, 1, "...");
        assert!(result.truncated);
        assert!(std::str::from_utf8(result.text.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_utf8_safe_empty_marker() {
        let result = truncate_utf8_safe("hello world", 5, "");
        assert!(result.truncated);
        assert_eq!(result.text, "hello");
    }

    #[test]
    fn truncate_utf8_safe_empty_input() {
        let result = truncate_utf8_safe("", 10, "...");
        assert_eq!(result.text, "");
        assert!(!result.truncated);
    }

    #[test]
    fn truncate_utf8_safe_result_is_valid_utf8() {
        let inputs = vec![
            "plain ascii",
            "日本語テスト",
            "émojis: 😀😃😄",
            "\u{0000}null\u{0000}byte",
            "a\u{0301}b\u{0301}c\u{0301}",
        ];
        let limits = [1, 3, 5, 10, 50, 100];
        for input in &inputs {
            for &limit in &limits {
                let result = truncate_utf8_safe(input, limit, "...");
                assert!(
                    std::str::from_utf8(result.text.as_bytes()).is_ok(),
                    "Invalid UTF-8 for input={input:?}, limit={limit}"
                );
            }
        }
    }

    #[test]
    fn formatted_response_truncation_metadata_correct() {
        let long_result = "x".repeat(500);
        let response = ok_response(Some(serde_json::Value::String(long_result)));
        let formatted = format_response(&response, 100);
        assert!(formatted.truncated);
        assert!(formatted.output.contains("truncated"));
    }
}
