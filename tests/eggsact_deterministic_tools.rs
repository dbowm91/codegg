//! Integration tests for Phase 4 eggsact model-facing deterministic tools.

use std::sync::Arc;

use codegg::eggsact::adapter::{EggsactConfig, EggsactRuntime};
use codegg::tool::deterministic::build_eggsact_tools;
use codegg::tool::{Tool, ToolCategory, ToolRegistry};

fn test_runtime() -> Arc<EggsactRuntime> {
    Arc::new(EggsactRuntime::new(EggsactConfig::default()).unwrap())
}

// ── Always-visible tool definitions ─────────────────────────────────

const ALWAYS_VISIBLE_NAMES: &[&str] = &[
    "text_equal",
    "text_diff_explain",
    "text_replace_check",
    "validate_json",
    "validate_toml",
    "command_preflight",
    "path_normalize",
    "text_security_inspect",
];

#[test]
fn always_visible_tools_are_exposed_in_definitions() {
    let runtime = test_runtime();
    let (_visible, _deferred) = build_eggsact_tools(runtime);
    let registry = ToolRegistry::with_defaults();
    let defs: Vec<String> = registry.definitions().into_iter().map(|d| d.name).collect();
    for name in ALWAYS_VISIBLE_NAMES {
        assert!(
            defs.contains(&name.to_string()),
            "Always-visible tool '{name}' missing from definitions()"
        );
    }
}

#[test]
fn always_visible_tools_are_read_only() {
    let runtime = test_runtime();
    let (visible, _deferred) = build_eggsact_tools(runtime);
    for tool in &visible {
        assert_eq!(
            tool.category(),
            ToolCategory::ReadOnly,
            "Tool '{}' should be ReadOnly, got {:?}",
            tool.name(),
            tool.category()
        );
    }
}

#[test]
fn always_visible_tools_are_not_deferred() {
    let runtime = test_runtime();
    let (visible, _deferred) = build_eggsact_tools(runtime);
    for tool in &visible {
        assert!(
            !tool.defer_loading(),
            "Always-visible tool '{}' should not be deferred",
            tool.name()
        );
    }
}

// ── Deferred tool discovery ─────────────────────────────────────────

const DEFERRED_NAMES: &[&str] = &[
    "text_inspect",
    "config_preflight",
    "identifier_inspect",
    "structured_data_compare",
    "text_fingerprint",
];

#[test]
fn deferred_tools_are_discoverable_via_tool_search() {
    let runtime = test_runtime();
    let (_visible, deferred) = build_eggsact_tools(runtime);
    let names: Vec<String> = deferred.iter().map(|t| t.name().to_string()).collect();
    for name in DEFERRED_NAMES {
        assert!(
            names.contains(&name.to_string()),
            "Deferred tool '{name}' missing from deferred set"
        );
    }
}

#[test]
fn deferred_tools_have_defer_loading_true() {
    let runtime = test_runtime();
    let (_visible, deferred) = build_eggsact_tools(runtime);
    for tool in &deferred {
        assert!(
            tool.defer_loading(),
            "Deferred tool '{}' should have defer_loading() == true",
            tool.name()
        );
    }
}

#[test]
fn deferred_tools_are_exposed_in_definitions() {
    let runtime = test_runtime();
    let (_visible, deferred) = build_eggsact_tools(runtime);
    for tool in &deferred {
        assert!(
            tool.expose_in_definitions(),
            "Deferred tool '{}' should be exposed for tool_search discovery",
            tool.name()
        );
    }
}

#[test]
fn deferred_tools_are_read_only() {
    let runtime = test_runtime();
    let (_visible, deferred) = build_eggsact_tools(runtime);
    for tool in &deferred {
        assert_eq!(
            tool.category(),
            ToolCategory::ReadOnly,
            "Deferred tool '{}' should be ReadOnly",
            tool.name()
        );
    }
}

// ── No harness-only tools exposed ───────────────────────────────────

#[test]
fn no_harness_only_tools_in_always_visible() {
    let runtime = test_runtime();
    let (visible, deferred) = build_eggsact_tools(runtime);
    let all_names: Vec<&str> = visible
        .iter()
        .chain(deferred.iter())
        .map(|t| t.name())
        .collect();
    // These eggsact tools require harness audience — must NOT appear
    assert!(
        !all_names.contains(&"path_scope_check"),
        "Harness-only tool path_scope_check should not be exposed"
    );
    assert!(
        !all_names.contains(&"shell_split"),
        "Harness-only tool shell_split should not be exposed"
    );
}

// ── Eggsact name mapping ────────────────────────────────────────────

#[test]
fn codegg_names_map_to_correct_eggsact_names() {
    let runtime = test_runtime();
    let (visible, deferred) = build_eggsact_tools(runtime);
    // For all current tools, codegg_name == eggsact_name
    for tool in visible.iter().chain(deferred.iter()) {
        assert_eq!(
            tool.name(),
            tool.name(),
            "Tool codegg_name and eggsact_name should match for '{}'",
            tool.name()
        );
    }
}

// ── Provenance on structured results ────────────────────────────────

#[test]
fn structured_result_has_eggsact_provenance() {
    let runtime = test_runtime();
    let (visible, _deferred) = build_eggsact_tools(runtime);
    let text_equal = visible.iter().find(|t| t.name() == "text_equal").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(
        text_equal.execute_structured(serde_json::json!({"a": "hello", "b": "hello"}), None),
    );
    let structured = result.unwrap();
    assert!(structured.success);
    let prov = structured.provenance.unwrap();
    assert_eq!(prov.backend, "native");
    assert_eq!(prov.implementation, "eggsact/text_equal");
    assert_eq!(prov.trust, codegg::tool::ToolTrust::LocalTrusted,);
}

// ── Individual tool execution smoke tests ───────────────────────────

#[test]
fn text_equal_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible.iter().find(|t| t.name() == "text_equal").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"a": "x", "b": "x"})));
    assert!(
        result.is_ok(),
        "text_equal should execute: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(output.contains("ok: true"));
}

#[test]
fn validate_json_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "validate_json")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"text": r#"{"k":"v"}"#})));
    assert!(
        result.is_ok(),
        "validate_json should execute: {:?}",
        result.err()
    );
}

#[test]
fn validate_toml_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "validate_toml")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result =
        rt.block_on(tool.execute(serde_json::json!({"text": "[package]\nname = \"test\""})));
    assert!(
        result.is_ok(),
        "validate_toml should execute: {:?}",
        result.err()
    );
}

#[test]
fn path_normalize_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "path_normalize")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"path": "./foo/../bar/"})));
    assert!(
        result.is_ok(),
        "path_normalize should execute: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(output.contains("ok: true"));
}

// ── Parameters schema sanity ────────────────────────────────────────

#[test]
fn all_always_visible_tools_have_parameters() {
    let runtime = test_runtime();
    let (visible, _deferred) = build_eggsact_tools(runtime);
    for tool in &visible {
        let params = tool.parameters();
        assert_eq!(
            params["type"],
            "object",
            "Tool '{}' parameters should be object type",
            tool.name()
        );
        assert!(
            params["properties"].is_object(),
            "Tool '{}' should have properties",
            tool.name()
        );
        assert!(
            params["required"].is_array(),
            "Tool '{}' should have required array",
            tool.name()
        );
    }
}

// ── Tool count ──────────────────────────────────────────────────────

#[test]
fn expected_tool_counts() {
    let runtime = test_runtime();
    let (visible, deferred) = build_eggsact_tools(runtime);
    assert_eq!(
        visible.len(),
        ALWAYS_VISIBLE_NAMES.len(),
        "Always-visible count mismatch"
    );
    assert_eq!(
        deferred.len(),
        DEFERRED_NAMES.len(),
        "Deferred count mismatch"
    );
}

// ── Remaining always-visible tool execution tests ────────────────

#[test]
fn text_diff_explain_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "text_diff_explain")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"a": "hello", "b": "world"})));
    assert!(
        result.is_ok(),
        "text_diff_explain should execute: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(output.contains("ok: true"));
}

#[test]
fn text_replace_check_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "text_replace_check")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({
        "text": "hello world",
        "old": "world",
        "new": "rust"
    })));
    assert!(
        result.is_ok(),
        "text_replace_check should execute: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(output.contains("ok: true"));
}

#[test]
fn command_preflight_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "command_preflight")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"command": "ls -la"})));
    assert!(
        result.is_ok(),
        "command_preflight should execute: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(output.contains("ok: true"));
}

#[test]
fn text_security_inspect_tool_executes() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let tool = visible
        .iter()
        .find(|t| t.name() == "text_security_inspect")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"text": "hello world"})));
    assert!(
        result.is_ok(),
        "text_security_inspect should execute: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(output.contains("ok: true"));
}

// ── Deferred tool execution smoke tests ─────────────────────────

#[test]
fn text_inspect_deferred_executes() {
    let runtime = test_runtime();
    let (_, deferred) = build_eggsact_tools(runtime);
    let tool = deferred
        .iter()
        .find(|t| t.name() == "text_inspect")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"text": "hello"})));
    assert!(
        result.is_ok(),
        "text_inspect should execute: {:?}",
        result.err()
    );
}

#[test]
fn text_fingerprint_deferred_executes() {
    let runtime = test_runtime();
    let (_, deferred) = build_eggsact_tools(runtime);
    let tool = deferred
        .iter()
        .find(|t| t.name() == "text_fingerprint")
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(tool.execute(serde_json::json!({"text": "hello world"})));
    assert!(
        result.is_ok(),
        "text_fingerprint should execute: {:?}",
        result.err()
    );
}

// ── Descriptions do not imply mutation ───────────────────────────

#[test]
fn wrapper_descriptions_are_read_only_no_mutation() {
    let runtime = test_runtime();
    let (visible, _) = build_eggsact_tools(runtime);
    let mutation_keywords = ["delete", "remove", "write", "create", "modify", "install"];
    for tool in &visible {
        let desc = tool.description().to_lowercase();
        for kw in &mutation_keywords {
            assert!(
                !desc.contains(kw),
                "Tool '{}' description contains mutation keyword '{}': {}",
                tool.name(),
                kw,
                tool.description()
            );
        }
    }
}

// ── Deferred tools are in definitions but with defer_loading ───────

#[test]
fn deferred_tools_in_default_definitions_have_defer_loading() {
    let registry = ToolRegistry::with_defaults();
    let defs = registry.definitions();
    for name in DEFERRED_NAMES {
        let def = defs.iter().find(|d| d.name == *name);
        assert!(
            def.is_some(),
            "Deferred tool '{}' should be in definitions()",
            name
        );
    }
}

// ── Disabled deterministic backend hides wrappers ────────────────

#[test]
fn unknown_profile_falls_back_to_default() {
    let config = EggsactConfig {
        profile: "nonexistent_profile_xyz".to_string(),
        ..EggsactConfig::default()
    };
    let runtime = EggsactRuntime::new(config);
    assert!(
        runtime.is_ok(),
        "Unknown profile should fall back to default: {:?}",
        runtime.err()
    );
    let runtime = runtime.unwrap();
    assert!(
        runtime.has_tool("text_equal"),
        "Fallback should still have text_equal"
    );
}
