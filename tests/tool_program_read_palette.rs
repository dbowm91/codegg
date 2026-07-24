//! Integration tests for the read-only programmable tool palette (M006).
//!
//! Validates that read-only tools are correctly exposed through the
//! broker with DirectOrProgrammatic policy, that manifest resolution
//! works, and that the cache stores/retrieves results.

use std::path::PathBuf;

use async_trait::async_trait;
use codegg::error::ToolError;
use codegg::tool::broker::{BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCachePolicy, ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass,
    ToolTerminalStatus, ToolValue,
};
use codegg::tool::program_cache::{CacheKey, ProgramCallCache};
use codegg::tool::program_manifest::{self, RejectionReason};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use serde_json::json;

// ── Mock read-only tools for testing ──────────────────────────────────────

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
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("line 1: hello\nline 2: world".to_string())
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

struct MockGlobTool;

#[async_trait]
impl Tool for MockGlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files by pattern"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"pattern": {"type": "string"}}})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("[2 files]\n\nsrc/main.rs\nsrc/lib.rs".to_string())
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
                json!({"type": "object", "properties": {"files": {"type": "array"}}}),
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
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("src/main.rs:1:fn main() {\nsrc/lib.rs:1:pub fn helper() {}".to_string())
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

struct MockListTool;

#[async_trait]
impl Tool for MockListTool {
    fn name(&self) -> &str {
        "list"
    }
    fn description(&self) -> &str {
        "List directory contents"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("src/\ntests/\nCargo.toml".to_string())
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
                json!({"type": "object", "properties": {"entries": {"type": "array"}}}),
            ),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

struct MockWriteTool;

#[async_trait]
impl Tool for MockWriteTool {
    fn name(&self) -> &str {
        "write"
    }
    fn description(&self) -> &str {
        "Write file"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok("written".to_string())
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }
    // No contract override — defaults to DirectOnly, NonIdempotent
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::with_defaults();
    registry.register(MockReadTool);
    registry.register(MockGlobTool);
    registry.register(MockGrepTool);
    registry.register(MockListTool);
    registry.register(MockWriteTool);
    registry
}

fn make_broker() -> ToolBroker {
    ToolBroker::new(&make_registry())
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

fn agent_ctx() -> BrokerInvocationContext {
    BrokerInvocationContext {
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
    }
}

// ── Contract tests ────────────────────────────────────────────────────────

#[test]
fn read_only_tools_have_direct_or_programmatic_policy() {
    let broker = make_broker();
    for name in &["read", "glob", "grep", "list"] {
        let contract = broker.catalog().get(name).unwrap();
        assert_eq!(
            contract.caller_policy,
            ToolCallerPolicy::DirectOrProgrammatic,
            "tool {} should be DirectOrProgrammatic",
            name
        );
    }
}

#[test]
fn mutating_tool_stays_direct_only() {
    let broker = make_broker();
    let contract = broker.catalog().get("write").unwrap();
    assert_eq!(contract.caller_policy, ToolCallerPolicy::DirectOnly);
}

#[test]
fn read_only_tools_have_output_schemas() {
    let broker = make_broker();
    for name in &["read", "glob", "grep", "list"] {
        let contract = broker.catalog().get(name).unwrap();
        assert!(
            contract.output_schema.is_some(),
            "tool {} should have an output schema",
            name
        );
    }
}

#[test]
fn read_only_tools_are_cacheable() {
    let broker = make_broker();
    for name in &["read", "glob", "grep", "list"] {
        let contract = broker.catalog().get(name).unwrap();
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert!(
            contract.cache_policy.enabled,
            "tool {} should be cacheable",
            name
        );
    }
}

// ── Broker routing tests ──────────────────────────────────────────────────

#[tokio::test]
async fn programmatic_call_to_read_succeeds() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
}

#[tokio::test]
async fn programmatic_call_to_glob_succeeds() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "glob",
            json!({"pattern": "**/*.rs"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
}

#[tokio::test]
async fn programmatic_call_to_grep_succeeds() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "grep",
            json!({"pattern": "fn main"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
}

#[tokio::test]
async fn programmatic_call_to_list_succeeds() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "list",
            json!({"path": "."}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
}

#[tokio::test]
async fn programmatic_call_to_write_denied() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let err = broker
        .execute(
            &registry,
            "write",
            json!({"path": "/tmp/x", "content": "y"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        codegg::tool::broker::BrokerError::CallerDenied { .. }
    ));
}

#[tokio::test]
async fn agent_direct_call_to_read_still_works() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            agent_ctx(),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
}

// ── Manifest resolution tests ─────────────────────────────────────────────

#[test]
fn manifest_allows_read_only_palette() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(
        &broker,
        &["read".into(), "glob".into(), "grep".into(), "list".into()],
    );
    assert!(resolved.rejected.is_empty());
    assert_eq!(resolved.allowed_tools.len(), 4);
    assert!(program_manifest::manifest_is_valid(&resolved));
}

#[test]
fn manifest_rejects_write_tool() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(&broker, &["write".into()]);
    assert_eq!(resolved.rejected.len(), 1);
    assert_eq!(resolved.rejected[0].reason, RejectionReason::DirectOnly);
}

#[test]
fn manifest_rejects_unknown_tool() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(&broker, &["nonexistent".into()]);
    assert_eq!(resolved.rejected.len(), 1);
    assert_eq!(resolved.rejected[0].reason, RejectionReason::NotFound);
}

#[test]
fn manifest_mixed_palette() {
    let broker = make_broker();
    let resolved = program_manifest::resolve_manifest(
        &broker,
        &["read".into(), "write".into(), "grep".into()],
    );
    assert_eq!(resolved.allowed_tools.len(), 2); // read, grep
    assert_eq!(resolved.rejected.len(), 1); // write
}

// ── Cache tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn cache_stores_broker_results() {
    let cache = ProgramCallCache::with_defaults();
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/a.txt"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();

    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    cache.insert(key.clone(), result.value.clone());

    let cached = cache.get(&key).unwrap();
    assert_eq!(cached.value.display, result.value.display);
}

#[test]
fn cache_miss_for_different_args() {
    let cache = ProgramCallCache::with_defaults();
    let key1 = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    let key2 = CacheKey::new("read", &json!({"path": "/tmp/b.txt"}), None);
    cache.insert(key1, ToolValue::success("a".to_string()));
    assert!(cache.get(&key2).is_none());
}

// ── Equivalence tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn direct_and_programmatic_read_produce_same_output() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    let agent_result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            agent_ctx(),
        )
        .await
        .unwrap();

    let program_result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();

    assert_eq!(agent_result.value.display, program_result.value.display);
    assert_eq!(
        agent_result.value.terminal_status,
        program_result.value.terminal_status
    );
}

#[tokio::test]
async fn direct_and_programmatic_glob_produce_same_output() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    let agent_result = broker
        .execute(
            &registry,
            "glob",
            json!({"pattern": "**/*.rs"}),
            agent_ctx(),
        )
        .await
        .unwrap();

    let program_result = broker
        .execute(
            &registry,
            "glob",
            json!({"pattern": "**/*.rs"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();

    assert_eq!(agent_result.value.display, program_result.value.display);
    assert_eq!(
        agent_result.value.terminal_status,
        program_result.value.terminal_status
    );
}

#[tokio::test]
async fn direct_and_programmatic_grep_produce_same_output() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    let agent_result = broker
        .execute(
            &registry,
            "grep",
            json!({"pattern": "fn main"}),
            agent_ctx(),
        )
        .await
        .unwrap();

    let program_result = broker
        .execute(
            &registry,
            "grep",
            json!({"pattern": "fn main"}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();

    assert_eq!(agent_result.value.display, program_result.value.display);
    assert_eq!(
        agent_result.value.terminal_status,
        program_result.value.terminal_status
    );
}

#[tokio::test]
async fn direct_and_programmatic_list_produce_same_output() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    let agent_result = broker
        .execute(&registry, "list", json!({"path": "."}), agent_ctx())
        .await
        .unwrap();

    let program_result = broker
        .execute(
            &registry,
            "list",
            json!({"path": "."}),
            program_ctx("prog-1"),
        )
        .await
        .unwrap();

    assert_eq!(agent_result.value.display, program_result.value.display);
    assert_eq!(
        agent_result.value.terminal_status,
        program_result.value.terminal_status
    );
}

#[tokio::test]
async fn direct_and_programmatic_routes_preserve_structured_value() {
    let registry = make_registry();
    let broker = ToolBroker::new(&registry);

    for name in &["read", "glob", "grep", "list"] {
        let input = match *name {
            "read" => json!({"path": "/tmp/test.txt"}),
            "glob" => json!({"pattern": "**/*.rs"}),
            "grep" => json!({"pattern": "fn"}),
            "list" => json!({"path": "."}),
            _ => unreachable!(),
        };

        let agent_result = broker
            .execute(&registry, name, input.clone(), agent_ctx())
            .await
            .unwrap();
        let program_result = broker
            .execute(&registry, name, input, program_ctx("prog-eq"))
            .await
            .unwrap();

        // Both routes produce the same display text
        assert_eq!(
            agent_result.value.display, program_result.value.display,
            "tool {} display mismatch between direct and programmatic routes",
            name
        );
        // Both routes produce the same terminal status
        assert_eq!(
            agent_result.value.terminal_status, program_result.value.terminal_status,
            "tool {} status mismatch between direct and programmatic routes",
            name
        );
    }
}
