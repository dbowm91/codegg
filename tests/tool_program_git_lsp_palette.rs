//! M003 — Operation-scoped Git/LSP read adapters for Tool Programs.
//!
//! Allow/deny effect table, bounded read-subset selection with canonical
//! delegation, hidden `ProgrammaticOnly` contracts, brokered program
//! execution, exhaustive mutation-name/input negatives, workspace
//! isolation, cache/replay/provenance truthfulness, and ordinary
//! model-disclosure invariance.
//!
//! Git positives execute against throwaway `git` repositories (the same
//! `git` CLI the canonical service uses). LSP positives prove
//! delegation equivalence with the canonical `lsp` tool instance (same
//! service, same root): adapter and canonical tool observe identical
//! results whether or not a language server is installed, so the
//! adapter adds no independent backend logic. Unavailable-server reads
//! fail closed as typed programmatic errors, never `Success`.

use std::path::Path;
use std::process::Command as StdCommand;

use codegg::tool::broker::{BrokerAuthority, BrokerError, BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass, ToolTerminalStatus,
};
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
        grant_id: "m003-test-grant".into(),
        principal_ref: "m003-test-principal".into(),
        workspace_id: "m003-test-ws".into(),
        workspace_path_policy_id: "workspace:m003-test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "m003-test-policy-v1".into(),
        allowed_caller_class: "program".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest: "m003-test-manifest".into(),
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
        timeout_ms: Some(10_000),
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
        grant_id: "m003-agent-grant".into(),
        principal_ref: "m003-test-principal".into(),
        workspace_id: "m003-test-ws".into(),
        workspace_path_policy_id: "workspace:m003-test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "m003-test-policy-v1".into(),
        allowed_caller_class: "agent".into(),
        allowed_effect_class: "any".into(),
        manifest_digest: "m003-agent-manifest".into(),
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
        timeout_ms: Some(10_000),
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

fn init_repo(dir: &Path) {
    StdCommand::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.email", "m003@example.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.name", "M003"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn commit_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
    StdCommand::new("git")
        .args(["add", name])
        .current_dir(dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", &format!("add {name}")])
        .current_dir(dir)
        .output()
        .unwrap();
}

/// M003 eligibility rule: programmatic admission requires an explicit
/// program-capable caller policy (`DirectOrProgrammatic` for ordinary
/// reads, `ProgrammaticOnly` for the hidden operation-scoped
/// adapters), a read-side effect class, and a declared output schema.
/// `ToolCategory::ReadOnly` alone is NOT sufficient, and the
/// multiplexed `DirectOnly` surfaces stay out.
fn is_program_eligible(contract: &ToolContract) -> bool {
    matches!(
        contract.caller_policy,
        ToolCallerPolicy::DirectOrProgrammatic | ToolCallerPolicy::ProgrammaticOnly
    ) && matches!(
        contract.effect_class,
        ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate
    ) && contract.output_schema.is_some()
}

// ── A. Operation allow/deny table + eligibility ───────────────────────────

#[test]
fn adapter_contracts_are_hidden_programmatic_only_reads() {
    let (_dir, registry) = workspace_registry();
    for name in ["git_read", "lsp_read"] {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("'{name}' registered"));
        let contract = tool.contract(name, tool.parameters());
        assert!(
            is_program_eligible(&contract),
            "'{name}' must satisfy the matrix"
        );
        assert_eq!(
            contract.caller_policy,
            ToolCallerPolicy::ProgrammaticOnly,
            "'{name}' must be ProgrammaticOnly (never model-called)"
        );
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert_eq!(
            contract.idempotency,
            codegg::tool::IdempotencyClass::Idempotent
        );
        // Workspace-version-dependent reads: cache disabled so reruns
        // re-observe repository/server state instead of implying
        // determinism; ledger replay still serves recorded results.
        assert!(
            !contract.cache_policy.enabled,
            "'{name}' program-call cache must be disabled"
        );
        assert_eq!(contract.retry_policy.max_retries, 0);
        assert!(contract.validate().is_ok());
        assert!(!tool.expose_in_definitions(), "'{name}' must stay hidden");
    }
}

#[test]
fn multiplexed_git_and_lsp_stay_direct_only() {
    let (_dir, registry) = workspace_registry();
    for name in ["git", "lsp"] {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("'{name}' registered"));
        let contract = tool.contract(name, tool.parameters());
        assert_eq!(
            contract.caller_policy,
            ToolCallerPolicy::DirectOnly,
            "multiplexed '{name}' must stay DirectOnly, got {:?}",
            contract.caller_policy
        );
        assert!(
            !is_program_eligible(&contract),
            "multiplexed '{name}' must remain program-ineligible"
        );
    }
}

#[test]
fn git_read_schema_admits_only_the_read_subset() {
    let (_dir, registry) = workspace_registry();
    let tool = registry.get("git_read").unwrap();
    let schema = tool.parameters();
    let ops: Vec<&str> = schema["properties"]["operation"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(ops, vec!["status", "diff", "log", "branches"]);
    // No mutation-shaped field exists anywhere in the schema.
    let props = schema["properties"].as_object().unwrap();
    for banned in [
        "mutation",
        "recover",
        "operation_state",
        "subcommand",
        "args",
        "workdir",
        "paths",
        "message",
        "force",
        "remote",
        "url",
        "target",
    ] {
        assert!(
            !props.contains_key(banned),
            "git_read schema must not contain '{banned}'"
        );
    }
}

#[test]
fn lsp_read_schema_admits_only_the_read_subset() {
    let (_dir, registry) = workspace_registry();
    let tool = registry.get("lsp_read").unwrap();
    let schema = tool.parameters();
    let ops: Vec<&str> = schema["properties"]["operation"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        ops,
        vec![
            "diagnostics",
            "documentSymbol",
            "workspaceSymbol",
            "hover",
            "goToDefinition",
            "findReferences"
        ]
    );
    let props = schema["properties"].as_object().unwrap();
    // Only the operation plus its read fields exist; every
    // preview/mutation-adjacent field is absent.
    for allowed in ["operation", "file_path", "line", "column", "symbol"] {
        assert!(
            props.contains_key(allowed),
            "lsp_read schema missing '{allowed}'"
        );
    }
    for banned in [
        "new_name",
        "action",
        "content",
        "patch",
        "radius",
        "only",
        "trigger_kind",
    ] {
        assert!(
            !props.contains_key(banned),
            "lsp_read schema must not contain '{banned}'"
        );
    }
}

// ── B. Canonical delegation ───────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn git_read_delegates_the_canonical_execution_service() {
    // The adapter display for `status` must equal the canonical
    // service stdout for the same typed operation: no independent
    // backend logic, no shell fallback of its own.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let adapter = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-delegate"),
        )
        .await
        .unwrap();
    assert_eq!(adapter.value.terminal_status, ToolTerminalStatus::Success);

    let service = codegg::git_service::GitExecutionService::new();
    let canonical = service
        .execute(
            &codegg_git::GitOperation::Status { short: false },
            dir.path(),
        )
        .await
        .unwrap();
    assert_eq!(adapter.value.display, canonical.stdout);
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_read_matches_canonical_tool_for_every_allowed_read() {
    // Same service, same workspace root: adapter and canonical tool
    // must observe identical results (success or identical typed
    // failure) for every allowed read, proving the adapter adds no
    // backend logic of its own. The registry is rooted at the program
    // workspace, mirroring production (`BrokerAdapter::with_cwd`).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(dir.path().to_path_buf()),
        ..Default::default()
    });
    let broker = ToolBroker::new(&registry);

    let cases: &[(&str, serde_json::Value)] = &[
        (
            "diagnostics",
            json!({"operation": "diagnostics", "file_path": "main.rs"}),
        ),
        (
            "documentSymbol",
            json!({"operation": "documentSymbol", "file_path": "main.rs"}),
        ),
        (
            "workspaceSymbol",
            json!({"operation": "workspaceSymbol", "symbol": "main"}),
        ),
        (
            "hover",
            json!({"operation": "hover", "file_path": "main.rs", "line": 1, "column": 4}),
        ),
        (
            "goToDefinition",
            json!({"operation": "goToDefinition", "file_path": "main.rs", "line": 1, "column": 4}),
        ),
        (
            "findReferences",
            json!({"operation": "findReferences", "file_path": "main.rs", "line": 1, "column": 4}),
        ),
    ];
    for (label, input) in cases {
        let adapter = broker
            .execute(
                &registry,
                "lsp_read",
                input.clone(),
                program_ctx(&registry, dir.path(), &["lsp_read"], "m003-lsp-equiv"),
            )
            .await
            .unwrap();
        let canonical = broker
            .execute(&registry, "lsp", input.clone(), agent_ctx(dir.path()))
            .await
            .unwrap();
        assert_eq!(
            adapter.value.terminal_status, canonical.value.terminal_status,
            "lsp_read/{label} must match canonical terminal status"
        );
        assert_eq!(
            adapter.value.display, canonical.value.display,
            "lsp_read/{label} must match canonical display"
        );
    }
}

// ── C. Git allowed-operation matrix ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_git_status_reports_branch_and_clean_state() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-status"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(
        result.value.display.contains("main"),
        "display={}",
        result.value.display
    );
    let value = result.value.value.as_ref().expect("structured value");
    assert_eq!(value["operation"], json!("status"));
    assert_eq!(value["truncated"], json!(false));
    assert_eq!(value["results"]["branch"], json!("main"));
    assert_eq!(value["results"]["is_dirty"], json!(false));
    let provenance = result.value.provenance.as_ref().expect("provenance");
    assert_eq!(provenance.trust, codegg::tool::ToolTrust::LocalTrusted);
    assert!(result.into_programmatic_outcome().is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn program_git_diff_observes_worktree_changes() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "diff"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-diff"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let value = result.value.value.expect("structured value");
    assert_eq!(value["operation"], json!("diff"));
}

#[tokio::test(flavor = "current_thread")]
async fn program_git_log_lists_commits() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "first\n");
    commit_file(dir.path(), "b.txt", "second\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "log", "max_count": 10}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-log"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let value = result.value.value.expect("structured value");
    assert_eq!(value["operation"], json!("log"));
    let commits = value["results"].as_array().expect("log results array");
    assert_eq!(commits.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn program_git_branches_lists_local_branches() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "branches"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-branches"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let value = result.value.value.expect("structured value");
    assert_eq!(value["operation"], json!("branches"));
    let branches = value["results"].as_array().expect("branches array");
    assert!(branches
        .iter()
        .any(|b| b["name"] == json!("main") && b["current"] == json!(true)));
}

// ── D. Git bounds ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_git_log_max_count_is_clamped() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    for i in 0..60 {
        commit_file(dir.path(), &format!("f{i}.txt"), "x\n");
    }
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "log", "max_count": 1000}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-log-clamp"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    let value = result.value.value.unwrap();
    let commits = value["results"].as_array().unwrap();
    assert!(
        commits.len() <= 50,
        "log must clamp to 50, got {}",
        commits.len()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn program_git_diff_base_ref_validation_rejects_flags() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for bad in ["-p", "--upload-pack=x", "HEAD; id", "", &"a".repeat(200)] {
        let result = broker
            .execute(
                &registry,
                "git_read",
                json!({"operation": "diff", "base_ref": bad}),
                program_ctx(&registry, dir.path(), &["git_read"], "m003-git-baseref"),
            )
            .await
            .unwrap();
        assert_ne!(
            result.value.terminal_status,
            ToolTerminalStatus::Success,
            "base_ref '{bad}' must fail closed"
        );
        assert!(result.into_programmatic_outcome().is_err());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn program_git_large_diff_is_truncated_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "big.txt", "seed\n");
    let lines: Vec<String> = (0..20_000).map(|i| format!("changed line {i}")).collect();
    std::fs::write(dir.path().join("big.txt"), lines.join("\n")).unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "diff"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-big"),
        )
        .await
        .unwrap();
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(result.value.truncated, "large diff must set truncated");
    assert!(result.value.display.contains("git_read truncated"));
    let value = result.value.value.expect("structured value");
    assert_eq!(value["truncated"], json!(true));
    assert!(result.value.provenance.unwrap().truncated);
}

// ── E. Git concurrent state change / rerun ────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_git_rerun_observes_changed_repository_state() {
    // Cache is disabled: a rerun after a worktree change must
    // re-observe state instead of serving a stale memoized result.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let first = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-rerun"),
        )
        .await
        .unwrap();
    assert_eq!(
        first.value.value.as_ref().unwrap()["results"]["is_dirty"],
        json!(false)
    );

    std::fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();
    let second = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-rerun"),
        )
        .await
        .unwrap();
    assert_eq!(second.value.terminal_status, ToolTerminalStatus::Success);
    assert_eq!(
        second.value.value.as_ref().unwrap()["results"]["is_dirty"],
        json!(true),
        "rerun must observe the dirty worktree, not a cached clean result"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn program_git_outside_repository_fails_typed() {
    let dir = tempfile::tempdir().unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir.path(), &["git_read"], "m003-git-outside"),
        )
        .await
        .unwrap();
    assert_ne!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(result.into_programmatic_outcome().is_err());
}

// ── F. Mutation-name/input negatives ──────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_git_read_rejects_every_mutation_shaped_input() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    // Unsupported `operation` values: broker schema-enum rejection or
    // adapter allowlist denial — both fail closed, never Success.
    for op in [
        "show",
        "blame",
        "add",
        "commit",
        "checkout",
        "push",
        "pull",
        "fetch",
        "merge",
        "rebase",
        "reset",
        "clean",
        "stash",
        "tag",
        "remote",
        "worktree",
        "rev-parse",
        "stage_paths",
        "commit_amend",
        "reset_hard",
        "continue",
        "abort",
        "skip",
        "operation_state",
    ] {
        let outcome = broker
            .execute(
                &registry,
                "git_read",
                json!({"operation": op}),
                program_ctx(&registry, dir.path(), &["git_read"], "m003-git-neg"),
            )
            .await;
        match outcome {
            Err(BrokerError::Execution(_)) => {}
            Ok(result) => {
                assert_ne!(
                    result.value.terminal_status,
                    ToolTerminalStatus::Success,
                    "git_read operation '{op}' must fail closed"
                );
                assert!(result.into_programmatic_outcome().is_err());
            }
            Err(other) => panic!("git_read operation '{op}' unexpected broker error: {other:?}"),
        }
    }

    // Mutation-shaped fields: strict parsing rejects them before any
    // backend is touched.
    for extra in [
        "mutation",
        "recover",
        "operation_state",
        "subcommand",
        "args",
        "workdir",
    ] {
        let mut input = json!({"operation": "status"});
        input[extra] = json!("commit");
        let result = broker
            .execute(
                &registry,
                "git_read",
                input,
                program_ctx(&registry, dir.path(), &["git_read"], "m003-git-negfield"),
            )
            .await
            .unwrap();
        assert_ne!(
            result.value.terminal_status,
            ToolTerminalStatus::Success,
            "git_read field '{extra}' must fail closed"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn program_lsp_read_rejects_every_preview_shaped_input() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for op in [
        "renamePreview",
        "formatPreview",
        "sourceActionPreview",
        "codeActionPreview",
        "codeActionSummaries",
        "semanticCheckPreview",
        "semanticContext",
        "securityContext",
        "callHierarchy",
        "typeHierarchy",
        "capabilities",
        "hunkSourceContext",
        "declaration",
        "implementation",
        "documentHighlights",
        "signatureHelp",
        "completion",
        "semanticTokens",
        "workflow_repair_local",
        "workflow_review_diff",
        "executeCommand",
    ] {
        let outcome = broker
            .execute(
                &registry,
                "lsp_read",
                json!({"operation": op, "file_path": "main.rs", "line": 1, "column": 1}),
                program_ctx(&registry, dir.path(), &["lsp_read"], "m003-lsp-neg"),
            )
            .await;
        match outcome {
            Err(BrokerError::Execution(_)) => {}
            Ok(result) => {
                assert_ne!(
                    result.value.terminal_status,
                    ToolTerminalStatus::Success,
                    "lsp_read operation '{op}' must fail closed"
                );
                assert!(result.into_programmatic_outcome().is_err());
            }
            Err(other) => panic!("lsp_read operation '{op}' unexpected broker error: {other:?}"),
        }
    }

    for extra in ["new_name", "action", "content", "patch", "radius", "only"] {
        let mut input =
            json!({"operation": "hover", "file_path": "main.rs", "line": 1, "column": 1});
        input[extra] = json!("x");
        let result = broker
            .execute(
                &registry,
                "lsp_read",
                input,
                program_ctx(&registry, dir.path(), &["lsp_read"], "m003-lsp-negfield"),
            )
            .await
            .unwrap();
        assert_ne!(
            result.value.terminal_status,
            ToolTerminalStatus::Success,
            "lsp_read field '{extra}' must fail closed"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn program_lsp_read_requires_position_and_bounds_symbol() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for input in [
        json!({"operation": "hover", "file_path": "main.rs"}),
        json!({"operation": "goToDefinition", "file_path": "main.rs", "line": 0, "column": 1}),
        json!({"operation": "diagnostics"}),
        json!({"operation": "workspaceSymbol"}),
        json!({"operation": "workspaceSymbol", "symbol": "x".repeat(201)}),
    ] {
        let result = broker
            .execute(
                &registry,
                "lsp_read",
                input,
                program_ctx(&registry, dir.path(), &["lsp_read"], "m003-lsp-bounds"),
            )
            .await
            .unwrap();
        assert_ne!(result.value.terminal_status, ToolTerminalStatus::Success);
    }
}

// ── G. Workspace / path / project isolation ───────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn program_git_read_is_scoped_to_the_program_workspace() {
    let dir_a = tempfile::tempdir().unwrap();
    init_repo(dir_a.path());
    commit_file(dir_a.path(), "a.txt", "repo-a\n");
    StdCommand::new("git")
        .args(["checkout", "-b", "feature-a"])
        .current_dir(dir_a.path())
        .output()
        .unwrap();

    let dir_b = tempfile::tempdir().unwrap();
    init_repo(dir_b.path());
    commit_file(dir_b.path(), "b.txt", "repo-b\n");

    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let in_a = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir_a.path(), &["git_read"], "m003-git-ws"),
        )
        .await
        .unwrap();
    let in_b = broker
        .execute(
            &registry,
            "git_read",
            json!({"operation": "status"}),
            program_ctx(&registry, dir_b.path(), &["git_read"], "m003-git-ws"),
        )
        .await
        .unwrap();
    assert_eq!(in_a.value.terminal_status, ToolTerminalStatus::Success);
    assert_eq!(in_b.value.terminal_status, ToolTerminalStatus::Success);
    assert_eq!(
        in_a.value.value.as_ref().unwrap()["results"]["branch"],
        json!("feature-a")
    );
    assert_eq!(
        in_b.value.value.as_ref().unwrap()["results"]["branch"],
        json!("main")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn program_lsp_read_path_escape_is_denied() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("secret.rs");
    std::fs::write(&outside_file, "fn secret() {}\n").unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for file_path in [
        outside_file.to_string_lossy().to_string(),
        "../escape.rs".to_string(),
    ] {
        let result = broker
            .execute(
                &registry,
                "lsp_read",
                json!({"operation": "diagnostics", "file_path": file_path}),
                program_ctx(&registry, dir.path(), &["lsp_read"], "m003-lsp-escape"),
            )
            .await
            .unwrap();
        assert_ne!(
            result.value.terminal_status,
            ToolTerminalStatus::Success,
            "lsp_read escape '{file_path}' must fail closed"
        );
        assert!(result.into_programmatic_outcome().is_err());
    }
}

// ── H. Broker caller policy ───────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn agent_caller_is_denied_the_program_only_adapters() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for (tool, input) in [
        ("git_read", json!({"operation": "status"})),
        (
            "lsp_read",
            json!({"operation": "diagnostics", "file_path": "a.txt"}),
        ),
    ] {
        let err = broker
            .execute(&registry, tool, input, agent_ctx(dir.path()))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BrokerError::CallerDenied { .. }),
            "'{tool}' must deny Agent callers, got {err:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn program_caller_is_denied_the_multiplexed_tools() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for (tool, input) in [
        ("git", json!({"subcommand": "status", "args": []})),
        (
            "lsp",
            json!({"operation": "diagnostics", "file_path": "a.txt"}),
        ),
    ] {
        let err = broker
            .execute(
                &registry,
                tool,
                input,
                program_ctx(&registry, dir.path(), &["git_read"], "m003-prog-multi"),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, BrokerError::CallerDenied { .. }),
            "multiplexed '{tool}' must deny Program callers, got {err:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unverified_program_authority_is_rejected_for_adapters() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    commit_file(dir.path(), "a.txt", "hello\n");
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    for (tool, input) in [
        ("git_read", json!({"operation": "status"})),
        (
            "lsp_read",
            json!({"operation": "workspaceSymbol", "symbol": "main"}),
        ),
    ] {
        let mut ctx = program_ctx(&registry, dir.path(), &[tool], "m003-unverified");
        ctx.authority = BrokerAuthority::Unverified;
        let err = broker
            .execute(&registry, tool, input, ctx)
            .await
            .unwrap_err();
        assert!(
            matches!(err, BrokerError::CallerDenied { .. }),
            "'{tool}' must reject unverified authority, got {err:?}"
        );
    }
}

// ── I. Manifest / contract snapshot ───────────────────────────────────────

#[test]
fn manifest_admits_adapters_and_rejects_multiplexed_tools() {
    let (_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let resolved = program_manifest::resolve_manifest(
        &broker,
        &["git_read".to_string(), "lsp_read".to_string()],
    );
    assert!(program_manifest::manifest_is_valid(&resolved));
    assert_eq!(resolved.allowed_tools.len(), 2);

    for name in ["git", "lsp"] {
        let resolved = program_manifest::resolve_manifest(&broker, &[name.to_string()]);
        assert_eq!(
            resolved.allowed_tools.len(),
            0,
            "'{name}' must not enter a manifest"
        );
        assert_eq!(resolved.rejected.len(), 1);
        assert_eq!(resolved.rejected[0].reason, RejectionReason::DirectOnly);
    }
}

#[test]
fn contract_snapshot_admits_programmatic_only_and_hash_covers_policy() {
    let (_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    // M003 fix: `ProgrammaticOnly` adapters resolve alongside the
    // `DirectOrProgrammatic` palette.
    let entries = codegg::tool::tool_program_context::resolve_contract_snapshot(
        &broker,
        &[
            "read".to_string(),
            "git_read".to_string(),
            "lsp_read".to_string(),
        ],
    )
    .unwrap();
    let digest = codegg::tool::tool_program_context::canonical_contract_digest(&entries).unwrap();
    assert!(digest.starts_with("sha256:"));

    // Weakening an adapter back to DirectOnly changes the digest, so
    // stale manifests invalidate instead of silently narrowing.
    let mut weakened = entries.clone();
    let idx = weakened
        .iter()
        .position(|e| e.tool_name == "git_read")
        .unwrap();
    weakened[idx].caller_policy = "direct_only".to_string();
    let weakened_digest =
        codegg::tool::tool_program_context::canonical_contract_digest(&weakened).unwrap();
    assert_ne!(digest, weakened_digest);

    // Multiplexed tools still fail snapshot resolution for programs.
    for name in ["git", "lsp"] {
        assert!(
            codegg::tool::tool_program_context::resolve_contract_snapshot(
                &broker,
                &[name.to_string()]
            )
            .is_err(),
            "'{name}' must fail snapshot resolution"
        );
    }
}

// ── J. Disclosure invariance ──────────────────────────────────────────────

#[test]
fn adapters_are_hidden_and_ordinary_definitions_unchanged() {
    use codegg::tool::disclosure;
    let registry = ToolRegistry::with_defaults();
    assert!(registry.contains("git_read"));
    assert!(registry.contains("lsp_read"));

    for name in ["git_read", "lsp_read"] {
        assert_eq!(
            disclosure::disclosure_for(name),
            disclosure::ToolDisclosure::Hidden
        );
        assert!(disclosure::is_hidden(name));
        assert!(!disclosure::is_core(name));
        assert!(!disclosure::is_deferred_by_default(name));
    }

    // Ordinary model-facing definitions do not grow: the adapters are
    // registered but never exposed.
    let defs: Vec<String> = registry.definitions().into_iter().map(|d| d.name).collect();
    assert!(!defs.contains(&"git_read".to_string()));
    assert!(!defs.contains(&"lsp_read".to_string()));
    // The multiplexed tools they delegate remain model-visible.
    assert!(defs.contains(&"git".to_string()));
    assert!(defs.contains(&"lsp".to_string()));
    // Curated palettes are untouched.
    for palette in [
        disclosure::CORE_PALETTE,
        disclosure::CURATED_PALETTE,
        disclosure::MINIMAL_PALETTE,
    ] {
        assert!(!palette.contains(&"git_read"));
        assert!(!palette.contains(&"lsp_read"));
    }
    assert!(!disclosure::plan_allowed("git_read"));
    assert!(!disclosure::plan_allowed("lsp_read"));
}

#[test]
fn adapters_are_undiscoverable_via_tool_search() {
    use std::sync::Arc;
    let registry = ToolRegistry::with_defaults();
    // Hidden tools never surface, even without an allow-list.
    for name in ["git_read", "lsp_read"] {
        let search =
            codegg::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
        let out = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(search.execute(json!({"query": name})))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let names: Vec<String> = parsed["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .collect();
        assert!(
            !names.contains(&name.to_string()),
            "hidden {name} must remain undiscoverable, got: {names:?}"
        );
    }
}

// ── K. Disabled-cache declaration / ledger / outcome truthfulness ─────────

#[test]
fn adapter_cache_is_disabled_as_version_dependent_declaration() {
    let (_dir, registry) = workspace_registry();
    for name in ["git_read", "lsp_read"] {
        let tool = registry.get(name).unwrap();
        let contract = tool.contract(name, tool.parameters());
        assert!(
            !contract.cache_policy.enabled,
            "'{name}' cache must be disabled"
        );
        assert_eq!(contract.cache_policy.ttl_secs, 0);
        assert_eq!(contract.cache_policy.max_entries, 0);
    }
}

#[test]
fn adapter_ledger_reserve_complete_replay_roundtrip() {
    use codegg::tool::tool_program_ledger::ToolProgramLedger;
    let workspace = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(workspace.path());
    let program_id = "m003-adapter-ledger-1";
    let request = CallRequest {
        tool_name: "git_read".to_string(),
        input: json!({"operation": "status"}),
        call_id: None,
    };
    ledger.reserve_call(program_id, 0, &request).unwrap();
    let completed = CompletedCall {
        sequence: 0,
        request: request.clone(),
        result: CallResult {
            output: ProgramValue::ToolResult(json!({"operation": "status"})),
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
    assert_eq!(loaded[&0].request.tool_name, "git_read");
    assert!(loaded[&0].result.success);
    // Replay serves the recorded result; a divergent call fails closed.
    ledger.reserve_call(program_id, 0, &request).unwrap();
    let divergent = CallRequest {
        tool_name: "git_read".to_string(),
        input: json!({"operation": "log"}),
        call_id: None,
    };
    assert!(ledger.reserve_call(program_id, 0, &divergent).is_err());
}

#[test]
fn programmatic_outcome_mapping_is_truthful_for_adapters() {
    use codegg::tool::contract::ToolValue;
    let ok = codegg::tool::BrokerResult {
        value: ToolValue::success("Branch: main".to_string()),
        contract: ToolContract::legacy("git_read", json!({})),
        invocation_id: "m003".to_string(),
        elapsed_ms: 1,
    };
    assert!(ok.into_programmatic_outcome().is_ok());

    let infra = codegg::tool::BrokerResult {
        value: ToolValue::infrastructure_error("LSP server not available".to_string()),
        contract: ToolContract::legacy("lsp_read", json!({})),
        invocation_id: "m003".to_string(),
        elapsed_ms: 1,
    };
    assert!(infra.into_programmatic_outcome().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn program_lsp_unavailable_fails_closed_never_success() {
    // Without a language server for the workspace, reads fail as
    // typed programmatic errors — the adapter never synthesizes a
    // result and never claims success.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "plain text\n").unwrap();
    let (_reg_dir, registry) = workspace_registry();
    let broker = ToolBroker::new(&registry);

    let result = broker
        .execute(
            &registry,
            "lsp_read",
            json!({"operation": "diagnostics", "file_path": "note.txt"}),
            program_ctx(&registry, dir.path(), &["lsp_read"], "m003-lsp-unavail"),
        )
        .await
        .unwrap();
    assert_ne!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(result.into_programmatic_outcome().is_err());
}

// ── L. Census: no other promotion ─────────────────────────────────────────

#[test]
fn candidate_census_no_other_tool_promoted_by_m003() {
    let (_dir, registry) = workspace_registry();
    // Admitted after M003: the M001/M002 palette plus the two hidden
    // operation-scoped adapters.
    let admitted = [
        "read",
        "glob",
        "grep",
        "list",
        "diff",
        "repo_search",
        "git_read",
        "lsp_read",
    ];
    for name in admitted {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("'{name}' registered"));
        let contract = tool.contract(name, tool.parameters());
        assert!(is_program_eligible(&contract), "'{name}' must be eligible");
    }
    // Everything else nearby stays out, with the multiplexed surfaces
    // explicitly pinned DirectOnly.
    for name in [
        "write",
        "edit",
        "apply_patch",
        "bash",
        "git",
        "lsp",
        "commit",
        "websearch",
        "webfetch",
        "tool_program",
        "task",
        "test",
        "terminal",
    ] {
        let Some(tool) = registry.get(name) else {
            continue;
        };
        let contract = tool.contract(name, tool.parameters());
        assert!(
            !is_program_eligible(&contract),
            "'{name}' must remain ineligible, got {contract:?}"
        );
    }
}
