//! Integration tests for tool contract guard invariants (M006).
//!
//! Validates the security and correctness invariants for the
//! read-only programmable tool palette.

use std::path::PathBuf;

use async_trait::async_trait;
use codegg::error::ToolError;
use codegg::tool::broker::{BrokerError, BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass, ToolTerminalStatus,
};
use codegg::tool::program_manifest::{self, RejectionReason};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use serde_json::json;

// ── Mock tools ────────────────────────────────────────────────────────────

struct ValidReadOnlyTool;

#[async_trait]
impl Tool for ValidReadOnlyTool {
    fn name(&self) -> &str {
        "valid_read"
    }
    fn description(&self) -> &str {
        "Valid read-only tool"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("data".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            output_schema: Some(json!({"type": "object"})),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

struct DirectOnlyTool;

#[async_trait]
impl Tool for DirectOnlyTool {
    fn name(&self) -> &str {
        "direct_only"
    }
    fn description(&self) -> &str {
        "Direct-only tool"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("direct".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    // No contract override — defaults to DirectOnly
}

struct NoSchemaTool;

#[async_trait]
impl Tool for NoSchemaTool {
    fn name(&self) -> &str {
        "no_schema"
    }
    fn description(&self) -> &str {
        "Tool without output schema"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("no schema".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            output_schema: None, // No schema!
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_broker() -> ToolBroker {
    let mut registry = ToolRegistry::with_defaults();
    registry.register(ValidReadOnlyTool);
    registry.register(DirectOnlyTool);
    registry.register(NoSchemaTool);
    ToolBroker::new(&registry)
}

fn program_ctx(program_id: &str) -> BrokerInvocationContext {
    BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: program_id.to_string(),
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

// ── Guard tests ───────────────────────────────────────────────────────────

#[test]
fn only_schema_tools_pass_manifest() {
    let broker = make_broker();
    let resolved =
        program_manifest::resolve_manifest(&broker, &["valid_read".into(), "no_schema".into()]);
    // valid_read has schema, no_schema doesn't
    assert_eq!(resolved.allowed_tools.len(), 1);
    assert_eq!(resolved.allowed_tools[0].name, "valid_read");
    assert_eq!(resolved.rejected.len(), 1);
    assert_eq!(resolved.rejected[0].reason, RejectionReason::NoOutputSchema);
}

#[test]
fn direct_only_rejected_by_manifest() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(&broker, &["direct_only".into()]);
    assert_eq!(resolved.rejected.len(), 1);
    assert_eq!(resolved.rejected[0].reason, RejectionReason::DirectOnly);
}

#[test]
fn direct_only_rejected_by_broker() {
    let broker = make_broker();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let err = rt.block_on(async {
        broker
            .execute(
                &ToolRegistry::with_defaults(),
                "direct_only",
                json!({}),
                program_ctx("prog-1"),
            )
            .await
            .unwrap_err()
    });
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[test]
fn unknown_tool_rejected() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(&broker, &["nonexistent".into()]);
    assert_eq!(resolved.rejected.len(), 1);
    assert_eq!(resolved.rejected[0].reason, RejectionReason::NotFound);
}

#[test]
fn manifest_is_valid_only_when_no_rejections() {
    let broker = make_broker();
    let valid = program_manifest::resolve_manifest(&broker, &["valid_read".into()]);
    let invalid =
        program_manifest::resolve_manifest(&broker, &["valid_read".into(), "direct_only".into()]);
    assert!(program_manifest::manifest_is_valid(&valid));
    assert!(!program_manifest::manifest_is_valid(&invalid));
}

#[test]
fn contract_validation_catches_empty_name() {
    let mut contract = ToolContract::legacy("test", json!({}));
    contract.name = String::new();
    assert!(contract.validate().is_err());
}

#[test]
fn contract_validation_catches_retry_on_non_retryable() {
    let mut contract = ToolContract::legacy("test", json!({}));
    contract.retry_policy.max_retries = 3;
    contract.effect_class = ToolEffectClass::NonIdempotent;
    assert!(contract.validate().is_err());
}

#[tokio::test]
async fn programmatic_unauthorized_caller_rejected() {
    let broker = make_broker();
    let mut registry = ToolRegistry::with_defaults();
    registry.register(ValidReadOnlyTool);
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "prog-1".to_string(),
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
        caller_authorized: false, // NOT authorized
    };
    let err = broker
        .execute(&registry, "valid_read", json!({}), ctx)
        .await
        .unwrap_err();
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[test]
fn tool_program_is_direct_only() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let contract = tool.contract("tool_program", tool.parameters());
    assert_eq!(
        contract.caller_policy,
        ToolCallerPolicy::DirectOnly,
        "tool_program must be DirectOnly — programs cannot submit other programs"
    );
    assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
    assert!(
        contract.output_schema.is_some(),
        "tool_program must have output schema"
    );
}

#[test]
fn palette_tools_are_direct_or_programmatic() {
    let registry = ToolRegistry::with_defaults();
    let palette = ["read", "glob", "grep", "list"];
    for name in &palette {
        if let Some(tool) = registry.get(name) {
            let contract = tool.contract(name, tool.parameters());
            assert_eq!(
                contract.caller_policy,
                ToolCallerPolicy::DirectOrProgrammatic,
                "palette tool '{}' must be DirectOrProgrammatic, got {:?}",
                name,
                contract.caller_policy
            );
            assert!(
                contract.output_schema.is_some(),
                "palette tool '{}' must have output schema",
                name
            );
        }
    }
}

#[test]
fn tool_program_rejects_mutation_tools() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(
        &broker,
        &[
            "valid_read".into(),
            "write_file".into(),
            "apply_patch".into(),
        ],
    );
    // Only valid_read should pass; write/apply_patch don't exist in mock broker
    // but the manifest correctly rejects unknown tools
    assert!(resolved.allowed_tools.len() <= 1);
    for rejection in &resolved.rejected {
        assert!(
            matches!(
                rejection.reason,
                RejectionReason::NotFound
                    | RejectionReason::DirectOnly
                    | RejectionReason::NoOutputSchema
            ),
            "unexpected rejection reason for '{}': {:?}",
            rejection.tool_name,
            rejection.reason
        );
    }
}
