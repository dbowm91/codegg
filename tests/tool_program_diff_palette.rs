//! M001 — Deterministic local read contract expansion.
//!
//! Eligibility matrix, `diff` promotion evidence, programmatic execution,
//! authority/path negatives, bounds, manifest/cache/replay truthfulness, and
//! the local-read candidate census (eligible vs deferred with reasons).

use std::path::{Path, PathBuf};

use codegg::error::ToolError;
use codegg::tool::broker::{BrokerAuthority, BrokerError, BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass, ToolTerminalStatus,
};
use codegg::tool::program_cache::{CacheKey, ProgramCallCache};
use codegg::tool::program_manifest::{self, RejectionReason};
use codegg::tool::{Tool, ToolRegistry, ToolRegistryOptions};
use codegg_core::jobs::ToolAuthorityGrant;
use codegg_core::tool_program::{CallRequest, CallResult, CompletedCall, ProgramValue};
use serde_json::json;

// ── Helpers ───────────────────────────────────────────────────────────────

fn workspace_registry() -> (tempfile::TempDir, ToolRegistry) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(dir.path().to_path_buf()),
        ..Default::default()
    });
    (dir, registry)
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
        grant_id: "m001-test-grant".into(),
        principal_ref: "m001-test-principal".into(),
        workspace_id: "m001-test-ws".into(),
        workspace_path_policy_id: "workspace:m001-test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "m001-test-policy-v1".into(),
        allowed_caller_class: "program".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest: "m001-test-manifest".into(),
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
        grant_id: "m001-agent-grant".into(),
        principal_ref: "m001-test-principal".into(),
        workspace_id: "m001-test-ws".into(),
        workspace_path_policy_id: "workspace:m001-test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "m001-test-policy-v1".into(),
        allowed_caller_class: "agent".into(),
        allowed_effect_class: "any".into(),
        manifest_digest: "m001-agent-manifest".into(),
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

fn write_workspace_file(workspace: &Path, name: &str, content: &str) -> PathBuf {
    let path = workspace.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// M001 eligibility rule: programmatic admission requires an explicit
/// `DirectOrProgrammatic` caller policy, a read-side effect class, and a
/// declared output schema. `ToolCategory::ReadOnly` alone is NOT sufficient.
fn is_eligible(contract: &ToolContract) -> bool {
    contract.caller_policy == ToolCallerPolicy::DirectOrProgrammatic
        && matches!(
            contract.effect_class,
            ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate
        )
        && contract.output_schema.is_some()
}

// ── A. Eligibility matrix ─────────────────────────────────────────────────

#[test]
fn admitted_local_read_palette_satisfies_eligibility_matrix() {
    let (_dir, registry) = workspace_registry();
    for name in ["read", "glob", "grep", "list", "diff"] {
        let tool = registry.get(name).unwrap();
        let contract = tool.contract(name, tool.parameters());
        assert!(is_eligible(&contract), "'{name}' must satisfy the matrix");
        assert!(contract.cache_policy.enabled, "'{name}' must be cacheable");
        // Conservative retry: no broker-side retries on local reads.
        assert_eq!(
            contract.retry_policy.max_retries, 0,
            "'{name}' must not enable broker retries"
        );
        assert!(contract.validate().is_ok(), "'{name}' contract invalid");
    }
}

#[test]
fn diff_contract_declares_bounded_structured_output() {
    let (_dir, registry) = workspace_registry();
    let tool = registry.get("diff").unwrap();
    let contract = tool.contract("diff", tool.parameters());
    assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
    assert_eq!(
        contract.idempotency,
        codegg::tool::IdempotencyClass::Idempotent
    );
    let schema = contract.output_schema.expect("diff output schema");
    for key in ["path", "has_changes", "diff", "truncated"] {
        assert!(schema["properties"].get(key).is_some(), "missing {key}");
    }
    assert!(schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("path")));
}

#[test]
fn broker_catalog_exposes_diff_with_programmatic_contract() {
    let (_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);
    let contract = broker.lookup_contract("diff").unwrap();
    assert_eq!(
        contract.caller_policy,
        ToolCallerPolicy::DirectOrProgrammatic
    );
    assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
}

// ── E. Candidate census: eligible vs deferred ─────────────────────────────
//
// Disposition for every other nearby read-looking tool. Only the five local
// deterministic reads are admitted; everything else records why it stays
// direct-only without being promoted by this milestone.

#[test]
fn candidate_census_only_deterministic_local_reads_are_eligible() {
    let (_dir, registry) = workspace_registry();
    // (tool, disposition reason — documents why M001 does not promote it)
    let deferred: &[(&str, &str)] = &[
        ("write", "mutation: NonIdempotent file write"),
        ("edit", "mutation: file edit"),
        ("apply_patch", "mutation: batch patch application"),
        ("replace", "mutation: file replace"),
        ("bash", "process execution with side effects"),
        ("terminal", "process execution with side effects"),
        ("test", "scheduler-owned heavy execution, not a pure read"),
        ("task", "delegation/spawn authority, not a pure read"),
        (
            "git",
            "multiplexed: mixes reads with mutations (M003 scope)",
        ),
        ("commit", "mutation: git commit"),
        (
            "lsp",
            "multiplexed: mixes reads with mutation-adjacent ops (M003 scope)",
        ),
        (
            "repo_search",
            "external network read with nondeterministic results (M002 scope)",
        ),
        ("repo_fetch", "external network fetch (out of scope)"),
        ("repo_map", "external-backed mapping (out of scope)"),
        ("codesearch", "external-backed search (out of scope)"),
        ("batch_fetch", "external network fetch (out of scope)"),
        ("evidence_bundle", "external-backed bundle (out of scope)"),
        (
            "research",
            "long-running pipeline, not a bounded local read",
        ),
        ("research_search", "external-backed search (out of scope)"),
        ("security_search", "external-backed search (out of scope)"),
        ("websearch", "external network tool (out of scope)"),
        ("webfetch", "external network tool (out of scope)"),
        (
            "tool_program",
            "DirectOnly submission tool: programs cannot submit programs",
        ),
        (
            "security",
            "scan verdicts over workspace policy, not admitted local read",
        ),
        ("skill", "asset activation surface, not a file-content read"),
        ("question", "interactive control flow, not a read"),
        (
            "image",
            "media attachment helper, no bounded structured read contract",
        ),
        (
            "review",
            "model-synthesized verdicts, nondeterministic output",
        ),
    ];
    for (name, reason) in deferred {
        let Some(tool) = registry.get(name) else {
            continue;
        };
        let contract = tool.contract(name, tool.parameters());
        assert!(
            !is_eligible(&contract),
            "'{name}' must remain ineligible ({reason}), got {contract:?}"
        );
    }
}

#[test]
fn mutation_and_network_tools_rejected_from_program_manifest() {
    let (_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);
    for name in [
        "write",
        "edit",
        "apply_patch",
        "bash",
        "git",
        "lsp",
        "repo_search",
        "websearch",
        "tool_program",
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

// ── B/C. Direct compatibility ─────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn direct_diff_compatibility_preserved() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "note.txt", "hello\nworld\n");
    let tool = registry.get("diff").unwrap();

    // Changed content produces a unified diff.
    let out = tool
        .execute(json!({"path": "note.txt", "original": "hello\nrust\n"}))
        .await
        .unwrap();
    assert!(out.contains("-rust") || out.contains("+world"), "out={out}");

    // Identical content reports no changes.
    let out = tool
        .execute(json!({"path": "note.txt", "original": "hello\nworld\n"}))
        .await
        .unwrap();
    assert_eq!(out, "(no changes)");

    // Missing `original` fails deterministically as before.
    let err = tool.execute(json!({"path": "note.txt"})).await.unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(_)),
        "unexpected error: {err:?}"
    );
}

// ── D. Programmatic execution ─────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_diff_success_with_structured_schema_and_provenance() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "prog.txt", "alpha\nbeta\n");
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "prog.txt", "original": "alpha\ngamma\n"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(!result.value.truncated);

    let value = result.value.value.expect("structured diff value");
    assert_eq!(value["has_changes"], json!(true));
    assert_eq!(value["truncated"], json!(false));
    assert!(value["diff"].as_str().unwrap().contains("+beta"));
    assert_eq!(value["original_bytes"], json!(12));
    assert_eq!(value["current_bytes"], json!(11));

    // Output validates against the registered contract schema.
    let schema = result.contract.output_schema.clone().unwrap();
    for key in ["path", "has_changes", "diff", "truncated"] {
        assert!(value.get(key).is_some(), "structured value missing {key}");
        assert!(schema["properties"].get(key).is_some());
    }

    // Deterministic local provenance, not external-untrusted.
    let provenance = result.value.provenance.expect("provenance");
    assert_eq!(provenance.implementation, "codegg/diff");
    assert_eq!(provenance.trust, codegg::tool::ToolTrust::LocalTrusted);
    assert!(!provenance.truncated);
}

#[tokio::test(flavor = "current_thread")]
async fn program_diff_no_changes_reports_structured_flag() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "same.txt", "one\ntwo\n");
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "same.txt", "original": "one\ntwo\n"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-2"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let value = result.value.value.unwrap();
    assert_eq!(value["has_changes"], json!(false));
    assert_eq!(value["diff"], json!("(no changes)"));
}

#[tokio::test(flavor = "current_thread")]
async fn direct_and_programmatic_diff_produce_same_output() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "eq.txt", "a\nb\nc\n");
    let broker = ToolBroker::new(&registry);
    let input = json!({"path": "eq.txt", "original": "a\nB\nc\n"});

    let agent_result = broker
        .execute(&registry, "diff", input.clone(), agent_ctx(dir.path()))
        .await
        .unwrap();
    let program_result = broker
        .execute(
            &registry,
            "diff",
            input,
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-eq"),
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
async fn program_relative_path_resolves_against_execution_context_root() {
    // The tool instance is rooted elsewhere, but the broker-supplied
    // execution context (workspace root) is authoritative for program calls:
    // no process-CWD fallback is used as program authority.
    let workspace = tempfile::tempdir().unwrap();
    write_workspace_file(workspace.path(), "ctx.txt", "x\ny\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let tool =
        codegg::tool::diff::DiffTool::default().with_allowed_root(elsewhere.path().to_path_buf());

    let mut exec =
        codegg::tool::ToolExecutionContext::with_backend(codegg::tool::ToolBackendKind::Native);
    exec.cwd = workspace.path().to_path_buf();
    let structured = tool
        .execute_structured(json!({"path": "ctx.txt", "original": "x\nz\n"}), Some(exec))
        .await
        .unwrap();
    assert!(structured.success);
    assert!(
        structured.output.contains("+y"),
        "out={}",
        structured.output
    );
    let value = structured.value.expect("structured value");
    assert_eq!(value["has_changes"], json!(true));
}

// ── Path escape / symlink / root policy negatives ─────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_diff_absolute_escape_is_denied() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "inside.txt", "a\n");
    let broker = ToolBroker::new(&registry);
    let outside = tempfile::tempdir().unwrap();
    let outside_file = write_workspace_file(outside.path(), "secret.txt", "top\nsecret\n");

    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": outside_file.to_string_lossy(), "original": "other\n"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-escape"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Denied);
    assert!(result.into_programmatic_outcome().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn program_diff_dotdot_traversal_is_denied() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "ok.txt", "a\n");
    // A traversal that resolves to a real file outside the workspace root
    // must be denied by the path policy (not merely "not found").
    let secret_name = format!("m001-dotdot-secret-{}.txt", std::process::id());
    let secret_path = dir.path().parent().unwrap().join(&secret_name);
    std::fs::write(&secret_path, "secret\n").unwrap();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": format!("../{secret_name}"), "original": "x\n"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-dotdot"),
        )
        .await
        .unwrap();
    std::fs::remove_file(&secret_path).ok();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Denied);
}

#[tokio::test(flavor = "current_thread")]
#[cfg(unix)]
async fn program_diff_symlink_escape_is_denied() {
    let (dir, registry) = workspace_registry();
    let outside = tempfile::tempdir().unwrap();
    write_workspace_file(outside.path(), "real.txt", "real\ncontent\n");
    std::os::unix::fs::symlink(outside.path().join("real.txt"), dir.path().join("link.txt"))
        .unwrap();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "link.txt", "original": "other\n"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-symlink"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Denied);
}

#[tokio::test(flavor = "current_thread")]
async fn program_diff_missing_path_fails_deterministically() {
    let (dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "missing.txt", "original": "x\n"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-missing"),
        )
        .await
        .unwrap();
    assert_ne!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(result.into_programmatic_outcome().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn program_diff_missing_original_fails_deterministically() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "f.txt", "a\n");
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "f.txt"}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-noorig"),
        )
        .await
        .unwrap();
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::InfrastructureError
    );
    assert!(result.value.display.contains("original content required"));
}

// ── Bounds ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_diff_oversized_original_is_rejected() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "small.txt", "a\n");
    let broker = ToolBroker::new(&registry);
    let huge = "y".repeat(10 * 1024 * 1024 + 1);
    // Defense in depth, outermost first: the broker input-size bound rejects
    // the call before dispatch.
    let err = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "small.txt", "original": huge}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-huge"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, BrokerError::InputTooLarge { .. }),
        "unexpected error: {err:?}"
    );

    // The tool-level bound is the deterministic backstop for direct callers
    // that bypass the broker.
    let tool = registry.get("diff").unwrap();
    let err = tool
        .execute(json!({"path": "small.txt", "original": huge}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(_)),
        "unexpected error: {err:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn program_diff_large_output_is_truncated_with_metadata() {
    let (dir, registry) = workspace_registry();
    let lines: Vec<String> = (0..20_000).map(|i| format!("current line {i}")).collect();
    write_workspace_file(dir.path(), "big.txt", &lines.join("\n"));
    let original_lines: Vec<String> = (0..20_000).map(|i| format!("original line {i}")).collect();
    let broker = ToolBroker::new(&registry);
    let result = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "big.txt", "original": original_lines.join("\n")}),
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-big"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(result.value.truncated);
    assert!(result.value.display.contains("diff truncated"));
    let value = result.value.value.expect("structured value");
    assert_eq!(value["truncated"], json!(true));
    assert_eq!(value["has_changes"], json!(true));
    assert!(result.value.provenance.unwrap().truncated);
}

// ── Broker caller policy ──────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_caller_denied_for_direct_only_tools() {
    let (dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);
    let err = broker
        .execute(
            &registry,
            "write",
            json!({"path": "x.txt", "content": "y"}),
            // Any valid program grant reaches the caller-policy gate first:
            // `write` is DirectOnly, so the broker denies before consulting
            // the tool itself.
            program_ctx(&registry, dir.path(), &["diff"], "m001-prog-w"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn unverified_program_authority_is_rejected() {
    let (dir, registry) = workspace_registry();
    write_workspace_file(dir.path(), "u.txt", "a\n");
    let broker = ToolBroker::new(&registry);
    let mut ctx = program_ctx(&registry, dir.path(), &["diff"], "m001-prog-unv");
    ctx.authority = BrokerAuthority::Unverified;
    let err = broker
        .execute(
            &registry,
            "diff",
            json!({"path": "u.txt", "original": "b\n"}),
            ctx,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, BrokerError::CallerDenied { .. }));
}

// ── Manifest / contract hash / cache / ledger / replay ────────────────────

#[test]
fn manifest_allows_diff_and_hash_covers_contract() {
    let (_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);
    let resolved = program_manifest::resolve_manifest(
        &broker,
        &[
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "list".to_string(),
            "diff".to_string(),
        ],
    );
    assert!(program_manifest::manifest_is_valid(&resolved));
    assert_eq!(resolved.allowed_tools.len(), 5);

    let entries = codegg::tool::tool_program_context::resolve_contract_snapshot(
        &broker,
        &[
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "list".to_string(),
            "diff".to_string(),
        ],
    )
    .unwrap();
    let digest = codegg::tool::tool_program_context::canonical_contract_digest(&entries).unwrap();
    assert!(digest.starts_with("sha256:"));

    // Any contract weakening (e.g. back to DirectOnly) changes the digest,
    // so stale manifests and caches invalidate instead of silently widening.
    let mut weakened = entries.clone();
    weakened[4].caller_policy = "direct_only".to_string();
    let weakened_digest =
        codegg::tool::tool_program_context::canonical_contract_digest(&weakened).unwrap();
    assert_ne!(digest, weakened_digest);
}

#[test]
fn diff_cache_roundtrip_is_workspace_scoped() {
    let cache = ProgramCallCache::with_defaults();
    let input = json!({"path": "a.txt", "original": "x\n"});
    let key = CacheKey::new("diff", &input, Some("ws-1"));
    let stored = codegg::tool::contract::ToolValue {
        display: "--- a/a.txt".to_string(),
        value: Some(json!({"has_changes": true})),
        artifacts: vec![],
        provenance: None,
        terminal_status: ToolTerminalStatus::Success,
        truncated: false,
    };
    cache.insert(key.clone(), stored);
    assert_eq!(cache.get(&key).unwrap().value.display, "--- a/a.txt");
    // Same input in another workspace must not hit.
    let other = CacheKey::new("diff", &input, Some("ws-2"));
    assert!(cache.get(&other).is_none());
    // Different input must not hit.
    let changed = CacheKey::new(
        "diff",
        &json!({"path": "b.txt", "original": "x\n"}),
        Some("ws-1"),
    );
    assert!(cache.get(&changed).is_none());
}

#[test]
fn diff_call_ledger_reserve_complete_replay_roundtrip() {
    use codegg::tool::tool_program_ledger::ToolProgramLedger;
    let workspace = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(workspace.path());
    let program_id = "m001-diff-ledger-1";
    let request = CallRequest {
        tool_name: "diff".to_string(),
        input: json!({"path": "a.txt", "original": "x\n"}),
        call_id: None,
    };
    ledger.reserve_call(program_id, 0, &request).unwrap();
    let completed = CompletedCall {
        sequence: 0,
        request: request.clone(),
        result: CallResult {
            output: ProgramValue::ToolResult(json!({"has_changes": true})),
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
    assert_eq!(loaded[&0].request.tool_name, "diff");
    assert!(loaded[&0].result.success);
    // Replay replays the recorded result: reserving the same call again is a
    // no-op, while a divergent identity fails closed.
    ledger.reserve_call(program_id, 0, &request).unwrap();
    let divergent = CallRequest {
        tool_name: "diff".to_string(),
        input: json!({"path": "other.txt", "original": "x\n"}),
        call_id: None,
    };
    assert!(ledger.reserve_call(program_id, 0, &divergent).is_err());
}

#[test]
fn programmatic_outcome_mapping_is_truthful_for_diff() {
    use codegg::tool::contract::ToolValue;
    let ok = codegg::tool::BrokerResult {
        value: ToolValue::success("--- a/f".to_string()),
        contract: ToolContract::legacy("diff", json!({})),
        invocation_id: "m001".to_string(),
        elapsed_ms: 1,
    };
    assert!(ok.into_programmatic_outcome().is_ok());

    let denied = codegg::tool::BrokerResult {
        value: ToolValue::denied("outside allowed directory".to_string()),
        contract: ToolContract::legacy("diff", json!({})),
        invocation_id: "m001".to_string(),
        elapsed_ms: 1,
    };
    assert!(matches!(
        denied.into_programmatic_outcome(),
        Err(codegg::tool::broker::ProgrammaticOutcome::Denied)
    ));
}
