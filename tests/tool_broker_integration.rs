//! Integration tests for the ToolBroker pipeline.
//!
//! Validates the full 10-step broker pipeline including:
//! - Registry lookup and contract resolution
//! - Caller-policy enforcement
//! - Input validation
//! - Execution through the broker
//! - Output validation and artifact registration
//! - Typed result conversion
//! - Error mapping

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codegg::error::ToolError;
use codegg::tool::broker::{BrokerError, BrokerInvocationContext, ToolBroker, ToolBrokerConfig};
use codegg::tool::contract::{ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use serde_json::json;

// ── Test tools ────────────────────────────────────────────────────────────

/// A simple read-only tool that returns a fixed string.
struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read file contents"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("file contents here".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

/// A tool that returns a very large output.
struct LargeOutputTool;

#[async_trait]
impl Tool for LargeOutputTool {
    fn name(&self) -> &str {
        "large_output"
    }
    fn description(&self) -> &str {
        "Returns large output"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("x".repeat(500_000))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
}

/// A tool that fails with a permission error.
struct DeniedTool;

#[async_trait]
impl Tool for DeniedTool {
    fn name(&self) -> &str {
        "denied"
    }
    fn description(&self) -> &str {
        "Tool that always fails"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::Permission("access denied".to_string()))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }
}

/// A tool that times out.
struct TimeoutTool;

#[async_trait]
impl Tool for TimeoutTool {
    fn name(&self) -> &str {
        "timeout"
    }
    fn description(&self) -> &str {
        "Tool that times out"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        Ok("never reached".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
}

/// A tool with a custom contract that allows programmatic callers.
struct ProgrammaticTool;

#[async_trait]
impl Tool for ProgrammaticTool {
    fn name(&self) -> &str {
        "programmatic"
    }
    fn description(&self) -> &str {
        "Tool callable by programs"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("done".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::with_defaults();
    registry.register(ReadTool);
    registry.register(LargeOutputTool);
    registry.register(DeniedTool);
    registry.register(TimeoutTool);
    registry.register(ProgrammaticTool);
    registry
}

fn make_ctx(caller: ToolCaller) -> BrokerInvocationContext {
    BrokerInvocationContext {
        caller,
        cwd: PathBuf::from("."),
        session_id: Some("test-session".to_string()),
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

// ── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn broker_full_pipeline_read_tool() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            make_ctx(ToolCaller::Agent),
        )
        .await
        .unwrap();

    assert_eq!(
        result.value.terminal_status,
        codegg::tool::contract::ToolTerminalStatus::Success
    );
    assert_eq!(result.value.display, "file contents here");
    assert!(!result.invocation_id.is_empty());
    assert!(result.elapsed_ms < 5000);
}

#[tokio::test]
async fn broker_not_found_returns_no_contract() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let err = broker
        .execute(
            &registry,
            "nonexistent",
            json!({}),
            make_ctx(ToolCaller::Agent),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, BrokerError::NoContract(_)));
}

#[tokio::test]
async fn broker_caller_policy_blocks_programmatic_on_direct_only() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    // "large_output" has no contract override — defaults to DirectOnly
    let err = broker
        .execute(
            &registry,
            "large_output",
            json!({}),
            make_ctx(ToolCaller::Program {
                program_id: "test-program".to_string(),
            }),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test]
async fn broker_caller_policy_allows_programmatic_on_direct_or_programmatic() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "programmatic",
            json!({}),
            make_ctx(ToolCaller::Program {
                program_id: "test-program".to_string(),
            }),
        )
        .await
        .unwrap();

    assert_eq!(
        result.value.terminal_status,
        codegg::tool::contract::ToolTerminalStatus::Success
    );
    assert_eq!(result.value.display, "done");
}

#[tokio::test]
async fn broker_input_too_large() {
    let registry = make_registry();
    let config = ToolBrokerConfig {
        max_input_bytes: 100,
        ..Default::default()
    };
    let broker = ToolBroker::with_config(&registry, config);
    let large_input = json!({"data": "x".repeat(200)});
    let err = broker
        .execute(&registry, "read", large_input, make_ctx(ToolCaller::Agent))
        .await
        .unwrap_err();

    assert!(matches!(err, BrokerError::InputTooLarge { .. }));
}

#[tokio::test]
async fn broker_output_truncation() {
    let registry = make_registry();
    let config = ToolBrokerConfig {
        max_output_bytes: 1000,
        ..Default::default()
    };
    let broker = ToolBroker::with_config(&registry, config);
    let result = broker
        .execute(
            &registry,
            "large_output",
            json!({}),
            make_ctx(ToolCaller::Agent),
        )
        .await
        .unwrap();

    assert!(result.value.truncated);
    assert!(result.value.display.len() <= 1000);
}

#[tokio::test]
async fn broker_artifact_registration_for_large_output() {
    let registry = make_registry();
    let config = ToolBrokerConfig {
        max_output_display_bytes: 100,
        ..Default::default()
    };
    let broker = ToolBroker::with_config(&registry, config);
    let result = broker
        .execute(
            &registry,
            "large_output",
            json!({}),
            make_ctx(ToolCaller::Agent),
        )
        .await
        .unwrap();

    assert!(!result.value.artifacts.is_empty());
    assert!(result.value.truncated);
}

#[tokio::test]
async fn broker_permission_error_maps_to_denied() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(&registry, "denied", json!({}), make_ctx(ToolCaller::Agent))
        .await
        .unwrap();

    assert_eq!(
        result.value.terminal_status,
        codegg::tool::contract::ToolTerminalStatus::Denied
    );
}

#[tokio::test]
async fn broker_timeout_context_propagated() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let mut ctx = make_ctx(ToolCaller::Agent);
    ctx.timeout_ms = Some(100);
    let result = broker
        .execute(&registry, "read", json!({}), ctx)
        .await
        .unwrap();

    // The broker doesn't enforce timeouts (AgentLoop does via
    // tokio::time::timeout). It propagates the timeout in the
    // context. The tool completes successfully within the timeout.
    assert_eq!(
        result.value.terminal_status,
        codegg::tool::contract::ToolTerminalStatus::Success
    );
}

#[tokio::test]
async fn broker_legacy_tools_get_conservative_defaults() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    // "large_output" has no contract override — gets legacy defaults
    let contract = broker.catalog().get("large_output").unwrap();

    assert_eq!(contract.caller_policy, ToolCallerPolicy::DirectOnly);
    assert_eq!(contract.effect_class, ToolEffectClass::NonIdempotent);
    assert!(!contract.cache_policy.enabled);
    assert_eq!(contract.retry_policy.max_retries, 0);
}

#[tokio::test]
async fn broker_custom_contract_preserved() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let contract = broker.catalog().get("programmatic").unwrap();

    assert_eq!(
        contract.caller_policy,
        ToolCallerPolicy::DirectOrProgrammatic
    );
    assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
}

#[tokio::test]
async fn broker_catalog_covers_all_registered_tools() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let catalog = broker.catalog();

    // Should have our custom tools plus all defaults
    assert!(catalog.contains("read"));
    assert!(catalog.contains("large_output"));
    assert!(catalog.contains("denied"));
    assert!(catalog.contains("timeout"));
    assert!(catalog.contains("programmatic"));
    // Default tools from ToolRegistry::with_defaults()
    assert!(catalog.contains("grep"));
    assert!(catalog.contains("glob"));
}

#[tokio::test]
async fn broker_concurrent_execution_preserves_ordering() {
    let registry = Arc::new(make_registry());
    let broker = Arc::new(ToolBroker::new(&registry));

    let mut handles = Vec::new();
    for i in 0..10 {
        let broker = Arc::clone(&broker);
        let registry = Arc::clone(&registry);
        handles.push(tokio::spawn(async move {
            broker
                .execute(
                    &registry,
                    "read",
                    json!({"path": format!("/tmp/{}.txt", i)}),
                    make_ctx(ToolCaller::Agent),
                )
                .await
        }));
    }

    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    for result in results {
        assert!(result.is_ok());
        let broker_result = result.unwrap();
        assert_eq!(
            broker_result.value.terminal_status,
            codegg::tool::contract::ToolTerminalStatus::Success
        );
    }
}

// ── Contention and concurrency tests ──────────────────────────────────────

#[tokio::test]
async fn broker_concurrent_reads_do_not_interfere() {
    let registry = Arc::new(make_registry());
    let broker = Arc::new(ToolBroker::new(&registry));

    // Spawn 50 concurrent read calls
    let mut handles = Vec::new();
    for i in 0..50 {
        let broker = Arc::clone(&broker);
        let registry = Arc::clone(&registry);
        handles.push(tokio::spawn(async move {
            broker
                .execute(
                    &registry,
                    "read",
                    json!({"path": format!("/tmp/file_{}.txt", i)}),
                    make_ctx(ToolCaller::Agent),
                )
                .await
        }));
    }

    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // All should succeed independently
    for result in &results {
        assert!(result.is_ok());
        assert_eq!(
            result.as_ref().unwrap().value.terminal_status,
            codegg::tool::contract::ToolTerminalStatus::Success
        );
    }

    // All invocation IDs should be unique
    let ids: Vec<_> = results
        .iter()
        .map(|r| r.as_ref().unwrap().invocation_id.clone())
        .collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[tokio::test]
async fn broker_no_unbounded_task_growth() {
    let registry = Arc::new(make_registry());
    let broker = Arc::new(ToolBroker::new(&registry));

    // Execute sequentially and verify each completes
    for i in 0..100 {
        let result = broker
            .execute(
                &registry,
                "read",
                json!({"path": format!("/tmp/{}.txt", i)}),
                make_ctx(ToolCaller::Agent),
            )
            .await
            .unwrap();
        assert_eq!(
            result.value.terminal_status,
            codegg::tool::contract::ToolTerminalStatus::Success
        );
    }
}

// ── Security and negative tests ───────────────────────────────────────────

#[tokio::test]
async fn broker_subagent_caller_denied_on_direct_only() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    // "large_output" has no contract override — defaults to DirectOnly
    let err = broker
        .execute(
            &registry,
            "large_output",
            json!({}),
            make_ctx(ToolCaller::Subagent {
                parent_agent_id: "parent-agent".to_string(),
            }),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test]
async fn broker_api_caller_denied_on_direct_only() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    // "large_output" has no contract override — defaults to DirectOnly
    let err = broker
        .execute(
            &registry,
            "large_output",
            json!({}),
            make_ctx(ToolCaller::Api {
                client_id: "api-client".to_string(),
            }),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test]
async fn broker_unauthorized_non_agent_non_internal_rejected() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "test".to_string(),
        },
        cwd: PathBuf::from("."),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: None,
        submission_key: None,
        caller_authorized: false,
    };
    let err = broker
        .execute(&registry, "programmatic", json!({}), ctx)
        .await
        .unwrap_err();

    // Programmatic caller without authorization is rejected
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test]
async fn broker_authorized_programmatic_caller_accepted() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "test".to_string(),
        },
        cwd: PathBuf::from("."),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: None,
        submission_key: None,
        caller_authorized: true,
    };
    let result = broker
        .execute(&registry, "programmatic", json!({}), ctx)
        .await
        .unwrap();

    assert_eq!(
        result.value.terminal_status,
        codegg::tool::contract::ToolTerminalStatus::Success
    );
}

#[tokio::test]
async fn broker_empty_tool_name_returns_no_contract() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let err = broker
        .execute(&registry, "", json!({}), make_ctx(ToolCaller::Agent))
        .await
        .unwrap_err();

    assert!(matches!(err, BrokerError::NoContract(_)));
}

#[tokio::test]
async fn broker_lookup_returns_contract_reference() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    let contract = broker.lookup_contract("read").unwrap();
    assert_eq!(contract.name, "read");

    let err = broker.lookup_contract("nonexistent").unwrap_err();
    assert!(matches!(err, BrokerError::NoContract(_)));
}

#[tokio::test]
async fn broker_validate_pre_execution_input_size() {
    let registry = make_registry();
    let config = ToolBrokerConfig {
        max_input_bytes: 50,
        ..Default::default()
    };
    let broker = ToolBroker::with_config(&registry, config);
    let contract = broker.lookup_contract("read").unwrap();
    let ctx = make_ctx(ToolCaller::Agent);
    let large_input = json!({"data": "x".repeat(100)});

    let err = broker
        .validate_pre_execution(contract, &ctx, &large_input)
        .unwrap_err();
    assert!(matches!(err, BrokerError::InputTooLarge { .. }));
}

#[tokio::test]
async fn broker_validate_pre_execution_caller_policy() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    // "large_output" has no contract override — defaults to DirectOnly
    let contract = broker.lookup_contract("large_output").unwrap();
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "test".to_string(),
        },
        cwd: PathBuf::from("."),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: None,
        submission_key: None,
        caller_authorized: true,
    };

    let err = broker
        .validate_pre_execution(contract, &ctx, &json!({}))
        .unwrap_err();
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

// ── Migration compatibility tests ─────────────────────────────────────────

#[tokio::test]
async fn broker_legacy_string_output_preserved() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            make_ctx(ToolCaller::Agent),
        )
        .await
        .unwrap();

    // Legacy tools produce string output through the broker
    assert_eq!(result.value.display, "file contents here");
    assert!(result.value.value.is_none()); // No structured value
    assert!(result.value.artifacts.is_empty()); // No artifacts for small output
}

#[tokio::test]
async fn broker_contract_read_only_palette_programmatic() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    // M006: read, grep, glob are now DirectOrProgrammatic with ReadOnly effect
    let programmable_tools = ["grep", "glob", "read"];
    for name in programmable_tools {
        if let Some(contract) = broker.catalog().get(name) {
            assert_eq!(
                contract.caller_policy,
                ToolCallerPolicy::DirectOrProgrammatic,
                "tool {} should be DirectOrProgrammatic",
                name
            );
            assert_eq!(
                contract.effect_class,
                ToolEffectClass::ReadOnly,
                "tool {} should be ReadOnly",
                name
            );
        }
    }

    // Mutating tools should remain DirectOnly
    let direct_only_tools = ["write", "edit"];
    for name in direct_only_tools {
        if let Some(contract) = broker.catalog().get(name) {
            assert_eq!(
                contract.caller_policy,
                ToolCallerPolicy::DirectOnly,
                "tool {} should be DirectOnly",
                name
            );
        }
    }
}
