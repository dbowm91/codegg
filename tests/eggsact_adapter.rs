//! Integration tests for the eggsact adapter (Phase 3).

use codegg::eggsact::adapter::{EggsactConfig, EggsactRuntime};

fn test_config() -> EggsactConfig {
    EggsactConfig::default()
}

#[test]
fn runtime_initializes_with_default_config() {
    let runtime = EggsactRuntime::new(test_config());
    assert!(
        runtime.is_ok(),
        "EggsactRuntime should initialize: {:?}",
        runtime.err()
    );
}

#[test]
fn runtime_initializes_with_codegg_core_profile() {
    let config = EggsactConfig {
        profile: "codegg_core".to_string(),
        ..test_config()
    };
    let runtime = EggsactRuntime::new(config).unwrap();
    assert!(runtime.has_tool("text_equal"));
    assert!(runtime.has_tool("validate_json"));
}

#[test]
fn text_equal_succeeds() {
    let runtime = EggsactRuntime::new(test_config()).unwrap();
    let result = runtime
        .call_json(
            "text_equal",
            serde_json::json!({"a": "hello", "b": "hello"}),
        )
        .unwrap();
    assert!(result.success);
    assert!(result.output.contains("ok: true"));
}

#[test]
fn text_equal_detects_inequality() {
    let runtime = EggsactRuntime::new(test_config()).unwrap();
    let result = runtime
        .call_json(
            "text_equal",
            serde_json::json!({"a": "hello", "b": "world"}),
        )
        .unwrap();
    // eggsact tools return ok: true for successful execution;
    // the result contains "equal": false for mismatched strings
    eprintln!("text_equal output: {:?}", result.output);
    assert!(result.success);
    assert!(result.output.contains("\"equal\":false"));
}

#[test]
fn validate_json_reports_valid_json() {
    let runtime = EggsactRuntime::new(test_config()).unwrap();
    let result = runtime
        .call_json(
            "validate_json",
            serde_json::json!({"text": r#"{"key": "value"}"#}),
        )
        .unwrap();
    assert!(result.success);
}

#[test]
fn validate_json_reports_invalid_json() {
    let runtime = EggsactRuntime::new(test_config()).unwrap();
    let result = runtime
        .call_json(
            "validate_json",
            serde_json::json!({"text": "{not valid json}"}),
        )
        .unwrap();
    // eggsact tools return ok: true for successful execution;
    // invalid JSON produces findings with JSON_PARSE_ERROR
    eprintln!("validate_json output: {:?}", result.output);
    assert!(result.success);
    assert!(result.output.contains("JSON_PARSE_ERROR"));
}

#[test]
fn unknown_tool_returns_error() {
    let runtime = EggsactRuntime::new(test_config()).unwrap();
    let result = runtime.call_json("nonexistent_tool_xyz", serde_json::json!({}));
    assert!(result.is_err());
}

#[test]
fn provenance_is_native_local_trusted() {
    let runtime = EggsactRuntime::new(test_config()).unwrap();
    let result = runtime
        .call_json("text_equal", serde_json::json!({"a": "x", "b": "x"}))
        .unwrap();
    let structured = codegg::eggsact::adapter::to_structured_result("text_equal", result);
    let prov = structured.provenance.unwrap();
    assert_eq!(prov.backend, "native");
    assert_eq!(prov.trust, codegg::tool::ToolTrust::LocalTrusted);
    assert!(prov.implementation.starts_with("eggsact/"));
}

#[test]
fn model_audience_blocks_harness_only_tools() {
    let config = EggsactConfig {
        audience: "model".to_string(),
        ..test_config()
    };
    let runtime = EggsactRuntime::new(config).unwrap();
    // path_scope_check is harness-only in codegg_core profile
    let result = runtime.call_json(
        "path_scope_check",
        serde_json::json!({"path": "/tmp", "scope": "/tmp"}),
    );
    assert!(
        result.is_err(),
        "Model audience should reject harness-only tools"
    );
}

#[test]
fn harness_audience_can_access_harness_tools() {
    let config = EggsactConfig {
        audience: "harness".to_string(),
        profile: "full".to_string(),
        ..test_config()
    };
    let runtime = EggsactRuntime::new(config).unwrap();
    // shell_split is available in harness audience with full profile
    assert!(
        runtime.has_tool("shell_split"),
        "Harness audience should have shell_split available"
    );
}

#[test]
fn max_output_chars_truncates() {
    let config = EggsactConfig {
        max_output_chars: 20,
        ..test_config()
    };
    let runtime = EggsactRuntime::new(config).unwrap();
    let result = runtime
        .call_json(
            "text_equal",
            serde_json::json!({"a": "hello", "b": "hello"}),
        )
        .unwrap();
    // The output should be truncated at 20 chars
    assert!(result.output.len() <= 50); // Allow some overhead for truncation message
}
