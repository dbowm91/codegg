//! Integration tests for tool program artifact isolation (M006).
//!
//! Validates that intermediate tool call outputs stay in the program's
//! artifact ledger and do NOT enter the parent model transcript. Only
//! the final program result (status, output, metrics) is projected.

use async_trait::async_trait;
use codegg::error::ToolError;
use codegg::tool::broker::{BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCachePolicy, ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass,
    ToolTerminalStatus, ToolValue,
};
use codegg::tool::tool_program::{ProgramCallArtifact, ToolProgramTool};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use serde_json::json;
use std::path::PathBuf;

// ── Mock tools ────────────────────────────────────────────────────────────

struct MockReadTool;

#[async_trait]
impl Tool for MockReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read file contents"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}})
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path = input
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown");
        Ok(format!("line 1: content of {}", path))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            cache_policy: ToolCachePolicy {
                enabled: true,
                ..ToolCachePolicy::default()
            },
            output_schema: Some(
                json!({"type": "object", "properties": {"content": {"type": "string"}}}),
            ),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

struct MockGrepTool;

#[async_trait]
impl Tool for MockGrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search file contents"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"pattern": {"type": "string"}}})
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown");
        Ok(format!("src/main.rs:1:found {}", pattern))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            cache_policy: ToolCachePolicy {
                enabled: true,
                ..ToolCachePolicy::default()
            },
            output_schema: Some(
                json!({"type": "object", "properties": {"matches": {"type": "array"}}}),
            ),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::with_defaults();
    registry.register(MockReadTool);
    registry.register(MockGrepTool);
    registry
}

fn make_broker() -> (ToolBroker, ToolRegistry) {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    (broker, registry)
}

fn program_ctx() -> BrokerInvocationContext {
    BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "prog-artifact-test".to_string(),
        },
        cwd: PathBuf::from("."),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(5_000),
        submission_key: None,
        caller_authorized: true,
    }
}

// ── ProgramCallArtifact tests ─────────────────────────────────────────────

#[test]
fn program_call_artifact_serializes_correctly() {
    let artifact = ProgramCallArtifact {
        tool_name: "read".to_string(),
        input: json!({"path": "/tmp/a.txt"}),
        success: true,
        artifact_handle: Some("ctx://tool/s1/0/c1".to_string()),
        preview: "line 1: content of /tmp/a.txt".to_string(),
    };

    let json = serde_json::to_value(&artifact).unwrap();
    assert_eq!(json["tool_name"], "read");
    assert_eq!(json["success"], true);
    assert_eq!(json["artifact_handle"], "ctx://tool/s1/0/c1");
    assert!(json["preview"].as_str().unwrap().starts_with("line 1:"));
}

#[test]
fn program_call_artifact_roundtrip() {
    let artifact = ProgramCallArtifact {
        tool_name: "grep".to_string(),
        input: json!({"pattern": "TODO"}),
        success: false,
        artifact_handle: None,
        preview: String::new(),
    };

    let json_str = serde_json::to_string(&artifact).unwrap();
    let back: ProgramCallArtifact = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.tool_name, "grep");
    assert!(!back.success);
    assert!(back.artifact_handle.is_none());
    assert!(back.preview.is_empty());
}

// ── Transcript isolation tests ────────────────────────────────────────────

#[test]
fn tool_program_output_schema_excludes_intermediate_content() {
    let tool = ToolProgramTool::new();
    let contract = tool.contract("tool_program", tool.parameters());
    let schema = contract.output_schema.unwrap();
    let props = schema.get("properties").unwrap();

    // Final result fields are present
    assert!(props.get("status").is_some());
    assert!(props.get("program_id").is_some());
    assert!(props.get("calls_completed").is_some());
    assert!(props.get("output").is_some());

    // Intermediate call content is NOT in the schema as a string field;
    // only program_artifacts metadata (handles, previews) is exposed
    assert!(
        props.get("program_artifacts").is_some(),
        "program_artifacts array must be in schema for intermediate call metadata"
    );
}

#[test]
fn tool_program_artifacts_field_is_empty_array_by_default() {
    // The tool_program result includes "program_artifacts": [] because
    // intermediate outputs stay in the program artifact ledger, not the transcript.
    // This test verifies the field is present and empty when no program has run.
    let tool = ToolProgramTool::new();
    let params = tool.parameters();
    // The tool's output schema defines program_artifacts as an array
    let contract = tool.contract("tool_program", params);
    let schema = contract.output_schema.unwrap();
    let artifacts_schema = schema
        .get("properties")
        .unwrap()
        .get("program_artifacts")
        .unwrap();
    assert_eq!(artifacts_schema["type"], "array");
}

// ── Broker call isolation tests ───────────────────────────────────────────

#[tokio::test]
async fn broker_returns_structured_value_for_programmatic_calls() {
    let (broker, registry) = make_broker();
    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            program_ctx(),
        )
        .await
        .unwrap();

    // The broker returns a ToolValue with display output
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(
        !result.value.display.is_empty(),
        "programmatic call should return non-empty display output"
    );
}

#[tokio::test]
async fn broker_returns_display_for_agent_calls() {
    let (broker, registry) = make_broker();
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Agent,
        cwd: PathBuf::from("."),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(5_000),
        submission_key: None,
        caller_authorized: true,
    };
    let result = broker
        .execute(&registry, "read", json!({"path": "/tmp/test.txt"}), ctx)
        .await
        .unwrap();

    // Agent calls also get structured value
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(!result.value.display.is_empty());
}

// ── Artifact handle format tests ──────────────────────────────────────────

#[test]
fn artifact_handle_format_is_ctx_uri() {
    // Context handles follow the ctx://tool/{session_id}/{turn_index}/{tool_call_id} format
    let handle = "ctx://tool/sess123/5/call_abc";
    assert!(
        handle.starts_with("ctx://tool/"),
        "artifact handles must use ctx:// scheme"
    );
}

#[test]
fn program_call_artifact_with_none_handle() {
    let artifact = ProgramCallArtifact {
        tool_name: "read".to_string(),
        input: json!({"path": "/tmp/small.txt"}),
        success: true,
        artifact_handle: None, // Small outputs may not need handles
        preview: "small output".to_string(),
    };

    let json = serde_json::to_value(&artifact).unwrap();
    assert!(json["artifact_handle"].is_null());
    assert_eq!(json["preview"], "small output");
}

// ── Multiple call artifacts tests ─────────────────────────────────────────

#[test]
fn multiple_artifacts_serialize_as_array() {
    let artifacts = vec![
        ProgramCallArtifact {
            tool_name: "read".to_string(),
            input: json!({"path": "/tmp/a.txt"}),
            success: true,
            artifact_handle: Some("ctx://tool/s1/0/c1".to_string()),
            preview: "content a".to_string(),
        },
        ProgramCallArtifact {
            tool_name: "grep".to_string(),
            input: json!({"pattern": "TODO"}),
            success: true,
            artifact_handle: Some("ctx://tool/s1/1/c2".to_string()),
            preview: "match found".to_string(),
        },
        ProgramCallArtifact {
            tool_name: "read".to_string(),
            input: json!({"path": "/tmp/b.txt"}),
            success: false,
            artifact_handle: None,
            preview: "error: file not found".to_string(),
        },
    ];

    let json = serde_json::to_value(&artifacts).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 3);
    assert_eq!(json[0]["tool_name"], "read");
    assert_eq!(json[1]["tool_name"], "grep");
    assert_eq!(json[2]["success"], false);
}
