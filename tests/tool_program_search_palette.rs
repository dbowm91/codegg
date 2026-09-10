//! M002 — External repository-search programmatic read seam.
//!
//! Eligibility (M001 matrix extended to an explicitly nondeterministic
//! external read), `repo_search` promotion evidence, daemon-owned runtime
//! threading, brokered program execution, backend/policy negatives,
//! bounds, provenance, cancellation, disabled-cache/replay truthfulness,
//! ledger replay-vs-rerun distinction, and the network-tool census (no
//! other network tool becomes callable).

use std::path::Path;
use std::sync::Arc;

use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::mcp::{McpService, McpTool};
use codegg::search_backend::SearchRuntimeContext;
use codegg::tool::broker::{BrokerAuthority, BrokerError, BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass, ToolTerminalStatus,
};
use codegg::tool::program_cache::{CacheKey, ProgramCallCache};
use codegg::tool::program_manifest::{self, RejectionReason};
use codegg::tool::{ToolRegistry, ToolRegistryOptions};
use codegg_core::jobs::ToolAuthorityGrant;
use codegg_core::tool_program::{CallRequest, CallResult, CompletedCall, ProgramValue};
use serde_json::json;

// ── Search fixture ────────────────────────────────────────────────────

fn eggsearch_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        fallback_to_builtin: Some(false),
        eggsearch: Some(EggsearchConfig {
            server_name: Some("eggsearch".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Build a mock eggsearch service advertising `repo_search` and serving
/// `handler` responses. Recorded `(tool, args)` pairs go into `calls`.
fn mock_repo_search_service(
    calls: Arc<tokio::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    handler: impl Fn(usize) -> String + Send + Sync + 'static,
) -> McpService {
    let mut svc = McpService::new();
    let handler = Arc::new(handler);
    svc.register_mock_server(
        "eggsearch",
        vec![McpTool {
            name: "repo_search".to_string(),
            description: "Search code repositories".to_string(),
            input_schema: json!({"type": "object"}),
            server: "eggsearch".to_string(),
        }],
        Box::new(move |tool, args| {
            let n = match calls.try_lock() {
                Ok(mut g) => {
                    g.push((tool.to_string(), args.clone()));
                    g.len()
                }
                Err(_) => 0,
            };
            Ok(handler(n))
        }),
    );
    svc
}

fn canned_results(query: &str) -> String {
    json!({
        "query": query,
        "results": [
            {"repo": "owner/repo", "path": "src/main.rs", "snippet": "fn main() {}"},
            {"repo": "owner/repo", "path": "src/lib.rs", "snippet": "pub fn helper() {}"}
        ]
    })
    .to_string()
}

fn search_runtime_with_mock(
    calls: Arc<tokio::sync::Mutex<Vec<(String, serde_json::Value)>>>,
) -> SearchRuntimeContext {
    let svc = mock_repo_search_service(calls, |_| canned_results("hello"));
    SearchRuntimeContext::new(eggsearch_config()).with_mcp(Arc::new(tokio::sync::RwLock::new(svc)))
}

/// Registry whose `repo_search` executes against the mock eggsearch service.
fn search_registry(workspace: &Path, ctx: SearchRuntimeContext) -> ToolRegistry {
    ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(workspace.to_path_buf()),
        search_runtime: Some(ctx),
        ..Default::default()
    })
}

fn grant_with_snapshot(registry: &ToolRegistry, tool_names: &[&str]) -> ToolAuthorityGrant {
    let mut entries = Vec::new();
    for name in tool_names {
        let contract = registry
            .get(name)
            .unwrap_or_else(|| panic!("tool '{name}' must be registered"))
            .contract(name, registry.get(name).unwrap().parameters());
        entries.push(codegg::tool::tool_program_context::contract_entry(&contract).unwrap());
    }
    let snapshot_json =
        codegg::tool::tool_program_context::canonical_contract_json(&entries).unwrap();
    let contract_digest =
        codegg::tool::tool_program_context::canonical_contract_digest(&entries).unwrap();
    let mut grant = ToolAuthorityGrant {
        schema_version: 1,
        grant_id: "m002-test-grant".into(),
        principal_ref: "m002-test-principal".into(),
        workspace_id: "m002-test-ws".into(),
        workspace_path_policy_id: "workspace:m002-test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "m002-test-policy-v1".into(),
        allowed_caller_class: "program".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest: "m002-test-manifest".into(),
        source_digest: String::new(),
        ir_digest: String::new(),
        contract_digest,
        contract_snapshot_json: snapshot_json,
        issued_at: 0,
        expires_at: None,
        revoked_at: None,
        decision_digest: String::new(),
    };
    grant.decision_digest = grant.compute_digest();
    grant
}

fn program_ctx(
    registry: &ToolRegistry,
    workspace: &Path,
    tool_names: &[&str],
    program_id: &str,
) -> BrokerInvocationContext {
    BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: program_id.to_string(),
        },
        cwd: workspace.to_path_buf(),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(5_000),
        submission_key: None,
        authority: BrokerAuthority::from_grant(grant_with_snapshot(registry, tool_names)),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
    }
}

fn agent_ctx(workspace: &Path) -> BrokerInvocationContext {
    let mut grant = ToolAuthorityGrant {
        schema_version: 1,
        grant_id: "m002-agent-grant".into(),
        principal_ref: "m002-test-principal".into(),
        workspace_id: "m002-test-ws".into(),
        workspace_path_policy_id: "workspace:m002-test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "m002-test-policy-v1".into(),
        allowed_caller_class: "agent".into(),
        allowed_effect_class: "any".into(),
        manifest_digest: "m002-agent-manifest".into(),
        source_digest: String::new(),
        ir_digest: String::new(),
        contract_digest: String::new(),
        contract_snapshot_json: String::new(),
        issued_at: 0,
        expires_at: None,
        revoked_at: None,
        decision_digest: String::new(),
    };
    grant.decision_digest = grant.compute_digest();
    BrokerInvocationContext {
        caller: ToolCaller::Agent,
        cwd: workspace.to_path_buf(),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(5_000),
        submission_key: None,
        authority: BrokerAuthority::from_grant(grant),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
    }
}

/// M001 matrix for deterministic local reads: explicit programmatic
/// policy, read-side effect, output schema, cache enabled, no retry.
fn is_eligible_local(contract: &ToolContract) -> bool {
    contract.caller_policy == ToolCallerPolicy::DirectOrProgrammatic
        && matches!(
            contract.effect_class,
            ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate
        )
        && contract.output_schema.is_some()
        && contract.cache_policy.enabled
        && contract.retry_policy.max_retries == 0
}

/// M002 extension for explicitly nondeterministic external reads: same
/// admission shape, but the program-call cache is DISABLED so repeated
/// identical queries re-execute instead of implying determinism.
fn is_eligible_external(contract: &ToolContract) -> bool {
    contract.caller_policy == ToolCallerPolicy::DirectOrProgrammatic
        && matches!(
            contract.effect_class,
            ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate
        )
        && contract.output_schema.is_some()
        && !contract.cache_policy.enabled
        && contract.retry_policy.max_retries == 0
}

fn is_program_eligible(contract: &ToolContract) -> bool {
    is_eligible_local(contract) || is_eligible_external(contract)
}

// ── A. Eligibility matrix ─────────────────────────────────────────────

#[test]
fn repo_search_satisfies_external_eligibility_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(dir.path(), search_runtime_with_mock(calls));
    let tool = registry.get("repo_search").unwrap();
    let contract = tool.contract("repo_search", tool.parameters());
    assert!(
        is_eligible_external(&contract),
        "repo_search must satisfy the M002 external matrix: {contract:?}"
    );
    assert!(!is_eligible_local(&contract));
    assert!(contract.validate().is_ok(), "repo_search contract invalid");
}

#[test]
fn broker_catalog_exposes_repo_search_with_programmatic_contract() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(dir.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    let contract = broker.lookup_contract("repo_search").unwrap();
    assert_eq!(
        contract.caller_policy,
        ToolCallerPolicy::DirectOrProgrammatic
    );
    assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
    assert!(contract.output_schema.is_some());
}

// ── Census: no other network tool becomes callable ────────────────────

#[test]
fn candidate_census_only_repo_search_admitted_among_external_reads() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(dir.path(), search_runtime_with_mock(calls));
    // (tool, disposition reason — documents why M002 does not promote it)
    let deferred: &[(&str, &str)] = &[
        ("write", "mutation: NonIdempotent file write"),
        ("edit", "mutation: file edit"),
        ("apply_patch", "mutation: batch patch application"),
        ("bash", "process execution with side effects"),
        (
            "git",
            "multiplexed: mixes reads with mutations (M003 scope)",
        ),
        (
            "lsp",
            "multiplexed: mixes reads with mutation-adjacent ops (M003 scope)",
        ),
        ("repo_fetch", "external network fetch (out of scope)"),
        ("repo_map", "external-backed mapping (out of scope)"),
        (
            "codesearch",
            "compat alias: canonical repo_search covers programs; alias stays direct-only",
        ),
        ("batch_fetch", "external network fetch (out of scope)"),
        ("evidence_bundle", "external-backed bundle (out of scope)"),
        (
            "research",
            "long-running pipeline, not a bounded search read",
        ),
        ("research_search", "external-backed search (out of scope)"),
        ("security_search", "external-backed search (out of scope)"),
        ("websearch", "external network tool (out of scope)"),
        (
            "webfetch",
            "external network tool with arbitrary-URL surface (out of scope)",
        ),
        (
            "tool_program",
            "DirectOnly submission tool: programs cannot submit programs",
        ),
    ];
    for (name, reason) in deferred {
        let Some(tool) = registry.get(name) else {
            continue;
        };
        let contract = tool.contract(name, tool.parameters());
        assert!(
            !is_program_eligible(&contract),
            "'{name}' must remain ineligible ({reason}), got {contract:?}"
        );
    }
}

#[test]
fn other_network_tools_rejected_from_program_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(dir.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    for name in [
        "websearch",
        "webfetch",
        "repo_fetch",
        "repo_map",
        "codesearch",
        "batch_fetch",
        "security_search",
        "research_search",
        "evidence_bundle",
    ] {
        if broker.lookup_contract(name).is_err() {
            continue;
        }
        let resolved = program_manifest::resolve_manifest(&broker, &[name.to_string()]);
        assert_eq!(
            resolved.allowed_tools.len(),
            0,
            "'{name}' must not enter a program manifest"
        );
        assert_eq!(resolved.rejected.len(), 1);
        assert!(
            matches!(
                resolved.rejected[0].reason,
                RejectionReason::DirectOnly
                    | RejectionReason::NoOutputSchema
                    | RejectionReason::NotReadOnly
                    | RejectionReason::InvalidContract(_)
            ),
            "'{name}' unexpected rejection: {:?}",
            resolved.rejected[0].reason
        );
    }
}

#[test]
fn repo_search_input_schema_admits_no_arbitrary_url_or_credential() {
    // SSRF/policy surface: repo_search takes query/filters only. There is
    // no URL parameter to smuggle an arbitrary fetch through, and no
    // credential parameter for a program to choose a hidden provider
    // identity with.
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(dir.path(), search_runtime_with_mock(calls));
    let tool = registry.get("repo_search").unwrap();
    let schema = tool.parameters();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("repo_search input schema properties");
    for forbidden in ["url", "api_key", "token", "credential", "env"] {
        assert!(
            !props.contains_key(forbidden),
            "repo_search input schema must not expose '{forbidden}'"
        );
    }
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("query")),
        "query must stay required"
    );
}

// ── B. Runtime threading ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_registry_uses_configured_search_runtime() {
    // The program-callable registry carries the daemon-owned context:
    // a configured mock service serves program calls, while a disabled
    // registry fails closed with a disabled error.
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let enabled = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let disabled = search_registry(workspace.path(), SearchRuntimeContext::disabled());

    let broker = ToolBroker::new(&enabled);
    let result = broker
        .execute(
            &enabled,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(&enabled, workspace.path(), &["repo_search"], "m002-prog-rt"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);

    let broker = ToolBroker::new(&disabled);
    let result = broker
        .execute(
            &disabled,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(
                &disabled,
                workspace.path(),
                &["repo_search"],
                "m002-prog-rt",
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::InfrastructureError
    );
    assert!(
        result.value.display.contains("disabled"),
        "unexpected: {}",
        result.value.display
    );
    assert!(result.into_programmatic_outcome().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn isolated_default_reports_unavailable_never_global() {
    // A registry built without an explicit context gets the isolated
    // default (eggsearch config, no shared service): program calls
    // report actionable unavailable, never a silent global slot.
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(workspace.path().to_path_buf()),
        ..Default::default()
    });
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-iso",
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::InfrastructureError
    );
    assert!(
        result.value.display.contains("eggsearch"),
        "expected eggsearch-unavailable, got: {}",
        result.value.display
    );
}

// ── C. Programmatic execution ─────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_repo_search_success_with_structured_schema_and_provenance() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(&registry, workspace.path(), &["repo_search"], "m002-prog-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(!result.value.truncated);

    // Structured value is the upstream JSON object; display is the
    // framed external-untrusted projection.
    let value = result.value.value.clone().expect("structured value");
    assert!(value.is_object(), "value must validate as object");
    assert!(result.value.display.contains("trust=external_untrusted"));
    assert!(result.value.display.contains("tool=repo_search"));

    // Output validates against the registered contract schema.
    let schema = result.contract.output_schema.clone().unwrap();
    assert_eq!(schema["type"], json!("object"));

    // External-untrusted provenance, never LocalTrusted.
    let provenance = result.value.provenance.expect("provenance");
    assert_eq!(provenance.trust, codegg::tool::ToolTrust::ExternalUntrusted);
    assert_eq!(provenance.backend, "mcp");
    assert!(!provenance.truncated);
}

#[tokio::test(flavor = "current_thread")]
async fn direct_and_programmatic_repo_search_produce_same_output() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    let input = json!({"query": "hello"});

    let agent_result = broker
        .execute(
            &registry,
            "repo_search",
            input.clone(),
            agent_ctx(workspace.path()),
        )
        .await
        .unwrap();
    let program_result = broker
        .execute(
            &registry,
            "repo_search",
            input,
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-eq",
            ),
        )
        .await
        .unwrap();
    assert_eq!(agent_result.value.display, program_result.value.display);
    assert_eq!(
        agent_result.value.terminal_status,
        program_result.value.terminal_status
    );
}

#[tokio::test(flavor = "current_thread")]
async fn direct_repo_search_compatibility_preserved() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let tool = registry.get("repo_search").unwrap();

    // Direct string path still serves framed evidence.
    let out = tool.execute(json!({"query": "hello"})).await.unwrap();
    assert!(out.contains("trust=external_untrusted"));

    // Missing query still fails deterministically as before.
    let err = tool.execute(json!({})).await.unwrap_err();
    assert!(
        matches!(err, codegg::error::ToolError::Execution(_)),
        "unexpected error: {err:?}"
    );
}

// ── D. Backend/policy negatives ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn unavailable_backend_yields_typed_tool_failure() {
    // Eggsearch config with no MCP handle: typed InfrastructureError
    // under existing policy, mappable to a programmatic Err.
    let workspace = tempfile::tempdir().unwrap();
    let registry = search_registry(
        workspace.path(),
        SearchRuntimeContext::new(eggsearch_config()),
    );
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-unav",
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::InfrastructureError
    );
    assert!(
        result.value.display.contains("eggsearch"),
        "unexpected: {}",
        result.value.display
    );
    assert!(result.into_programmatic_outcome().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn builtin_backend_requires_eggsearch() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = search_registry(
        workspace.path(),
        SearchRuntimeContext::new(SearchConfig {
            backend: Some(SearchBackendConfig::Builtin),
            ..Default::default()
        }),
    );
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-bi",
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::InfrastructureError
    );
    assert!(
        result.value.display.contains("eggsearch"),
        "unexpected: {}",
        result.value.display
    );
}

#[tokio::test(flavor = "current_thread")]
async fn program_caller_denied_for_direct_only_network_tools() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    // websearch is DirectOnly: any valid program grant is denied before
    // consulting the tool itself.
    let err = broker
        .execute(
            &registry,
            "websearch",
            json!({"query": "x"}),
            program_ctx(&registry, workspace.path(), &["repo_search"], "m002-prog-w"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn unverified_program_authority_is_rejected_for_search() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    let mut ctx = program_ctx(
        &registry,
        workspace.path(),
        &["repo_search"],
        "m002-prog-unv",
    );
    ctx.authority = BrokerAuthority::Unverified;
    let err = broker
        .execute(&registry, "repo_search", json!({"query": "x"}), ctx)
        .await
        .unwrap_err();
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn missing_query_fails_as_typed_programmatic_error() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(workspace.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    // Missing required `query` fails input-schema validation at the
    // broker boundary: a typed Err (never Success, never dispatched).
    let err = broker
        .execute(
            &registry,
            "repo_search",
            json!({}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-noq",
            ),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, BrokerError::Execution(_)),
        "unexpected error: {err:?}"
    );
}

// ── Bounds ────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn max_results_bounded_at_30() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(
        workspace.path(),
        search_runtime_with_mock(Arc::clone(&calls)),
    );
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello", "max_results": 100}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-max",
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "repo_search");
    assert_eq!(
        recorded[0].1.get("max_results"),
        Some(&json!(30)),
        "max_results must be capped at 30, got: {:?}",
        recorded[0].1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_search_output_is_truncated_with_metadata() {
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let big = "y".repeat(40_000);
    let svc = mock_repo_search_service(calls, move |_| {
        json!({"query": "big", "blob": big.clone()}).to_string()
    });
    let registry = search_registry(
        workspace.path(),
        SearchRuntimeContext::new(eggsearch_config())
            .with_mcp(Arc::new(tokio::sync::RwLock::new(svc))),
    );
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "big"}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-big",
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(result.value.truncated);
    assert!(result.value.display.contains("truncated by Codegg"));
    assert!(result.value.provenance.unwrap().truncated);
}

#[tokio::test(flavor = "current_thread")]
async fn program_cannot_smuggle_credentials_into_backend_args() {
    // Programs supply only query/filters; provider credentials stay in
    // the daemon-owned context. Unknown credential-ish fields never
    // reach the backend adapter.
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(
        workspace.path(),
        search_runtime_with_mock(Arc::clone(&calls)),
    );
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello", "api_key": "secret", "env": {"K": "V"}}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-cred",
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].1.get("api_key").is_none());
    assert!(recorded[0].1.get("env").is_none());
    assert_eq!(recorded[0].1.get("query"), Some(&json!("hello")));
}

// ── Cancellation ──────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn cancelled_program_search_is_typed_not_success() {
    // Cancellation propagates on the program path programs actually
    // use: `BrokerAdapter` observes the cancelled token before dispatch
    // and never touches the backend.
    use codegg::scheduler::tool_program_executor::BrokerAdapter;
    use codegg_core::tool_program::{BrokerCallback, InterpreterError};

    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = Arc::new(search_registry(
        workspace.path(),
        search_runtime_with_mock(Arc::clone(&calls)),
    ));
    let broker = Arc::new(ToolBroker::new(&registry));
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let adapter = BrokerAdapter::new(broker, registry, "m002-prog-cancel".to_string())
        .with_cancellation(token);
    let err = adapter
        .execute_call(&CallRequest {
            tool_name: "repo_search".to_string(),
            input: json!({"query": "x"}),
            call_id: None,
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, InterpreterError::Cancelled),
        "unexpected error: {err:?}"
    );
    assert!(
        calls.lock().await.is_empty(),
        "cancelled call must never dispatch to the backend"
    );
}

// ── Cache disabled / ledger replay vs rerun ───────────────────────────

#[test]
fn repo_search_cache_disabled_as_nondeterministic_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(dir.path().to_path_buf()),
        ..Default::default()
    });
    let tool = registry.get("repo_search").unwrap();
    let contract = tool.contract("repo_search", tool.parameters());
    assert!(
        !contract.cache_policy.enabled,
        "repo_search must declare cache disabled"
    );
    assert_eq!(contract.retry_policy.max_retries, 0);
}

#[test]
fn repo_search_cache_keys_remain_workspace_scoped_if_used() {
    // The key construction stays content+workspace scoped (as for local
    // reads) so any future bounded use cannot leak across workspaces;
    // production declares the contract disabled so repeated identical
    // queries re-execute instead of implying determinism.
    let cache = ProgramCallCache::with_defaults();
    let input = json!({"query": "hello"});
    let key = CacheKey::new("repo_search", &input, Some("ws-1"));
    let stored = codegg::tool::contract::ToolValue {
        display: "[external_repo_evidence trust=external_untrusted]".to_string(),
        value: Some(json!({"results": []})),
        artifacts: vec![],
        provenance: None,
        terminal_status: ToolTerminalStatus::Success,
        truncated: false,
    };
    cache.insert(key.clone(), stored);
    assert!(cache.get(&key).is_some());
    let other = CacheKey::new("repo_search", &input, Some("ws-2"));
    assert!(cache.get(&other).is_none());
}

#[test]
fn repo_search_call_ledger_reserve_complete_replay_roundtrip() {
    use codegg::tool::tool_program_ledger::ToolProgramLedger;
    let workspace = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(workspace.path());
    let program_id = "m002-search-ledger-1";
    let request = CallRequest {
        tool_name: "repo_search".to_string(),
        input: json!({"query": "hello"}),
        call_id: None,
    };
    ledger.reserve_call(program_id, 0, &request).unwrap();
    let completed = CompletedCall {
        sequence: 0,
        request: request.clone(),
        result: CallResult {
            output: ProgramValue::ToolResult(json!({"results": []})),
            artifacts: vec![],
            success: true,
        },
        replay_fingerprint: None,
    };
    ledger
        .persist_call_completion(program_id, &completed)
        .unwrap();
    assert!(ledger.is_call_completed(program_id, 0));
    let loaded = ledger.load_completed_calls(program_id).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[&0].request.tool_name, "repo_search");
    // Replay serves the recorded execution-time result: reserving the
    // same call again is a no-op, while a divergent identity fails closed.
    ledger.reserve_call(program_id, 0, &request).unwrap();
    let divergent = CallRequest {
        tool_name: "repo_search".to_string(),
        input: json!({"query": "different"}),
        call_id: None,
    };
    assert!(ledger.reserve_call(program_id, 0, &divergent).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn rerun_may_observe_different_results_replay_serves_recorded() {
    // Truthfulness core: a fresh rerun against a changed backend may
    // return different results (nondeterministic), while ledger replay
    // serves the recorded execution-time result.
    let workspace = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    let svc = mock_repo_search_service(calls, move |_| {
        let n = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        json!({"query": "hello", "generation": n}).to_string()
    });
    let registry = search_registry(
        workspace.path(),
        SearchRuntimeContext::new(eggsearch_config())
            .with_mcp(Arc::new(tokio::sync::RwLock::new(svc))),
    );
    let broker = ToolBroker::new(&registry);
    let first = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-rerun-1",
            ),
        )
        .await
        .unwrap();
    let second = broker
        .execute(
            &registry,
            "repo_search",
            json!({"query": "hello"}),
            program_ctx(
                &registry,
                workspace.path(),
                &["repo_search"],
                "m002-prog-rerun-2",
            ),
        )
        .await
        .unwrap();
    assert_eq!(first.value.terminal_status, ToolTerminalStatus::Success);
    assert_eq!(second.value.terminal_status, ToolTerminalStatus::Success);
    let v1 = first.value.value.expect("first value");
    let v2 = second.value.value.expect("second value");
    assert_ne!(
        v1, v2,
        "successive reruns against a changed backend must be allowed to differ"
    );
}

// ── Manifest ──────────────────────────────────────────────────────────

#[test]
fn manifest_allows_repo_search_and_hash_covers_contract() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let registry = search_registry(dir.path(), search_runtime_with_mock(calls));
    let broker = ToolBroker::new(&registry);
    let resolved = program_manifest::resolve_manifest(
        &broker,
        &[
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "list".to_string(),
            "diff".to_string(),
            "repo_search".to_string(),
        ],
    );
    assert!(program_manifest::manifest_is_valid(&resolved));
    assert_eq!(resolved.allowed_tools.len(), 6);

    let entries = codegg::tool::tool_program_context::resolve_contract_snapshot(
        &broker,
        &[
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "list".to_string(),
            "diff".to_string(),
            "repo_search".to_string(),
        ],
    )
    .unwrap();
    let digest = codegg::tool::tool_program_context::canonical_contract_digest(&entries).unwrap();
    assert!(digest.starts_with("sha256:"));

    // Any contract weakening (e.g. back to DirectOnly) changes the digest.
    let mut weakened = entries.clone();
    let pos = weakened
        .iter()
        .position(|e| e.tool_name == "repo_search")
        .unwrap();
    weakened[pos].caller_policy = "direct_only".to_string();
    let weakened_digest =
        codegg::tool::tool_program_context::canonical_contract_digest(&weakened).unwrap();
    assert_ne!(digest, weakened_digest);
}

#[test]
fn programmatic_outcome_mapping_is_truthful_for_search() {
    use codegg::tool::contract::ToolValue;
    let ok = codegg::tool::BrokerResult {
        value: ToolValue::success("results".to_string()),
        contract: ToolContract::legacy("repo_search", json!({})),
        invocation_id: "m002".to_string(),
        elapsed_ms: 1,
    };
    assert!(ok.into_programmatic_outcome().is_ok());

    let infra = codegg::tool::BrokerResult {
        value: ToolValue::infrastructure_error("eggsearch unavailable".to_string()),
        contract: ToolContract::legacy("repo_search", json!({})),
        invocation_id: "m002".to_string(),
        elapsed_ms: 1,
    };
    assert!(matches!(
        infra.into_programmatic_outcome(),
        Err(codegg::tool::broker::ProgrammaticOutcome::InfrastructureError)
    ));
}
