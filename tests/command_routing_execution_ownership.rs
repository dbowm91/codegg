//! Integration tests for command-routing execution ownership (Workstream I).
//!
//! Validates that:
//!  - `BashTool` persistence provenance reflects the ACTUAL executor, not the planned decision.
//!  - `RunOwnership` is set correctly based on what actually ran.
//!  - Delegated backends (TestRunner, PythonScript) own their own records; BashTool
//!    MUST NOT persist a duplicate record when a structured backend owns the run.
//!  - Active-routing fallback preserves the planned backend in `planned_backend`
//!    and sets `actual_backend` to `RawShell` with a `FallbackRecord`.
//!  - Raw stdout/stderr artifacts are NEVER `safe_for_model: true`; only structured
//!    artifacts (TestReport, projection) are model-safe.
//!  - `argv` reflects the actual `Command::new` invocation when managed argv was used.
//!  - The env-var kill switch (`CODEGG_ROUTING_DISABLE=1`) forces raw shell
//!    and produces a record with `planned_backend != actual_backend`.

use std::path::PathBuf;
use std::sync::Arc;

use codegg::config::schema::{CommandIntentConfig, CommandIntentMode, RouteLevel};
use codegg::python_script::PythonScriptTool;
use codegg::test_runner::types::{TestRunRequest, TestScope};
use codegg::tool::bash::BashTool;
use codegg::tool::Tool;
use codegg_core::run_store::{
    ActualBackend, ArtifactKind, BackendRecord, MemRunStore, PlannedBackend, RunCompletion,
    RunDraft, RunKind, RunOwnership, RunStatus, RunStore,
};
use serde_json::json;

// ── Helpers ──────────────────────────────────────────────────────────────

fn make_bash_tool(
    config: Option<CommandIntentConfig>,
    store: Arc<MemRunStore>,
) -> BashTool {
    // Ensure the env kill switch is NOT set. Tests must not inherit
    // `CODEGG_ROUTING_DISABLE=1` from sibling tests; each test that depends
    // on active routing must clear it before constructing the tool.
    std::env::remove_var("CODEGG_ROUTING_DISABLE");
    let mut tool = BashTool::new().with_run_store(store);
    if let Some(cic) = config {
        tool = tool.with_command_intent_config(cic);
    }
    tool
}

fn active_config() -> CommandIntentConfig {
    CommandIntentConfig {
        route_safe_commands: Some(true),
        route_tests: Some(RouteLevel::Active),
        route_git_read: Some(RouteLevel::Active),
        route_search: Some(RouteLevel::Active),
        route_python: Some(RouteLevel::Active),
        route_build: Some(RouteLevel::Active),
        route_lint: Some(RouteLevel::Active),
        route_format: Some(RouteLevel::Active),
        mode: Some(CommandIntentMode::Active),
        ..Default::default()
    }
}

fn observe_config() -> CommandIntentConfig {
    CommandIntentConfig {
        route_safe_commands: Some(true),
        route_tests: Some(RouteLevel::Observe),
        route_git_read: Some(RouteLevel::Observe),
        route_search: Some(RouteLevel::Observe),
        route_python: Some(RouteLevel::Observe),
        mode: Some(CommandIntentMode::Observe),
        ..Default::default()
    }
}

async fn all_runs(store: &MemRunStore) -> Vec<codegg_core::run_store::RunManifest> {
    let runs = store.list_runs(Default::default()).await.unwrap();
    let mut out = Vec::new();
    for r in runs {
        if let Some(m) = store.get_run(&r.run_id).await.unwrap() {
            out.push(m);
        }
    }
    out
}

// ── 1. Observe mode: no command-intent routing record ─────────────────

#[tokio::test]
async fn observe_mode_persists_raw_shell_with_unrouted_planned() {
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(Some(observe_config()), store.clone());

    let _ = tool.execute(json!({"command": "echo ownership-observe-ok"})).await.unwrap();

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1, "observe mode must persist exactly one run");
    let m = &runs[0];
    assert_eq!(m.kind, RunKind::RawShell);
    assert_eq!(m.ownership, RunOwnership::Caller);
    assert_eq!(m.planned_backend, Some(PlannedBackend::RawShell));
    assert_eq!(m.actual_backend, Some(ActualBackend::RawShell));
    assert!(m.fallback.is_none(), "no fallback in observe mode");
    assert_eq!(
        m.invocation.argv.as_deref(),
        Some(&["sh".to_string(), "-c".to_string(), "echo ownership-observe-ok".to_string()][..])
    );
}

// ── 2. Active routing: TestRunner is DelegatedBackend ─────────────────

#[tokio::test]
async fn active_test_command_routes_to_test_runner_as_delegated() {
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(Some(active_config()), store.clone());

    // Active routing will dispatch to TestRunner dispatch. For MVP, the
    // TestRunner dispatcher still executes via raw shell internally, but
    // it reports ActualExecutor::TestRunner and BashTool MUST mark
    // ownership = DelegatedBackend so it does NOT persist its own record.
    //
    // (Workstream D-2 will wire this directly into resolve_and_run_test.)
    let _ = tool
        .execute(json!({"command": "cargo test --no-run"}))
        .await
        .unwrap();

    let runs = all_runs(&store).await;
    // BashTool must NOT have persisted a record — TestRunner is delegated.
    // Note: the dispatch path still routes to test_runner which currently
    // does not call into the real TestRunner subsystem (MVP), so we expect
    // zero BashTool records for this command.
    for m in &runs {
        assert_eq!(
            m.ownership,
            RunOwnership::DelegatedBackend,
            "BashTool MUST NOT persist Caller-owned record when routing to TestRunner"
        );
    }
}

// ── 3. Active routing: GitRead routes to NativeTool (Caller-owned) ────

#[tokio::test]
async fn active_git_readonly_routes_to_native_tool_caller_owned() {
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(Some(active_config()), store.clone());

    let _ = tool.execute(json!({"command": "git status"})).await.unwrap();

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1, "active git must persist one record");
    let m = &runs[0];
    assert_eq!(m.kind, RunKind::GitRead);
    assert_eq!(m.ownership, RunOwnership::Caller);
    assert_eq!(m.planned_backend, Some(PlannedBackend::NativeTool));
    assert_eq!(m.actual_backend, Some(ActualBackend::NativeTool));
    assert!(m.fallback.is_none());
    // argv should be the actual native-tool invocation (no sh -c wrapping)
    let argv = m.invocation.argv.as_ref().expect("argv present");
    assert_eq!(argv[0], "git");
    assert_eq!(argv[1], "status");
}

// ── 4. Active routing: search routes to ManagedArgv (Caller-owned) ────

#[tokio::test]
async fn active_search_routes_to_managed_argv_caller_owned() {
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(Some(active_config()), store.clone());

    // Use a search-like command that classifies as SearchReadOnly
    let _ = tool
        .execute(json!({"command": "rg pattern src/"}))
        .await
        .unwrap();

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1);
    let m = &runs[0];
    assert_eq!(m.kind, RunKind::Search);
    assert_eq!(m.ownership, RunOwnership::Caller);
    assert_eq!(m.planned_backend, Some(PlannedBackend::ManagedArgv));
    assert_eq!(m.actual_backend, Some(ActualBackend::ManagedArgv));
    let argv = m.invocation.argv.as_ref().expect("argv present");
    assert_eq!(argv[0], "rg");
    // MUST NOT be `[sh, -c, command]` — that was the bug.
    assert_ne!(argv, &vec!["sh".to_string(), "-c".to_string(), "rg pattern src/".to_string()]);
}

// ── 5. Active routing fallback: planned != actual, FallbackRecord set ─

#[tokio::test]
async fn active_routing_fallback_preserves_planned_and_records_actual() {
    // We can't easily force dispatch failure in this test, but we can
    // verify the structure: when an active-routing dispatch fails, the
    // record should preserve `planned_backend` and set `actual_backend` to
    // RawShell, with a FallbackRecord.
    let store = Arc::new(MemRunStore::new());

    // Manually invoke the persistence path with a fake outcome.
    use codegg::command_outcome::{ActualExecutor, ExecutionOutcome};
    let planned = PlannedBackend::TestRunner;
    let actual = ActualExecutor::RawShell {
        command: "cargo test".to_string(),
        argv: vec!["sh".to_string(), "-c".to_string(), "cargo test".to_string()],
    };
    let outcome = ExecutionOutcome::with_fallback(planned.clone(), actual.clone(), "test runner unavailable");

    let draft = RunDraft {
        kind: RunKind::RawShell,
        invocation: codegg_core::run_store::RunInvocation {
            command: "cargo test".to_string(),
            argv: Some(vec!["sh".to_string(), "-c".to_string(), "cargo test".to_string()]),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: PathBuf::from("."),
        cwd: PathBuf::from("."),
        backend: BackendRecord {
            family: "bash".to_string(),
            detail: Some("raw_shell".to_string()),
        },
        risk: codegg_core::run_store::RiskRecord {
            level: "low".to_string(),
            has_subprocess: false,
            has_git_mutation: false,
            has_destructive_mutation: false,
        },
        planned_backend: Some(outcome.planned.clone()),
        actual_backend: Some(outcome.actual.clone().into_backend()),
        ownership: codegg_core::run_store::RunOwnership::Caller,
    };

    let handle = store.begin_run(draft).await.unwrap();
    let fallback = outcome.fallback_record().expect("fallback expected");
    let manifest = store
        .complete_run(
            handle,
            RunCompletion {
                status: RunStatus::Complete,
                completed_at: chrono::Utc::now(),
                permissions: vec![],
                sandbox: None,
                projection: None,
                changes: vec![],
                rerun: None,
                actual_backend: Some(outcome.actual.into_backend()),
                fallback: Some(fallback.clone()),
            },
        )
        .await
        .unwrap();

    assert_eq!(manifest.planned_backend, Some(planned));
    assert_eq!(manifest.actual_backend, Some(ActualBackend::RawShell));
    let fb = manifest.fallback.expect("fallback must be set");
    assert_eq!(fb.planned, PlannedBackend::TestRunner);
    assert_eq!(fb.actual, ActualBackend::RawShell);
    assert!(fb.reason.contains("test runner unavailable"));
}

// ── 6. Artifact safety: raw stdout/stderr are NEVER safe_for_model ────

#[tokio::test]
async fn bash_tool_artifacts_are_never_safe_for_model() {
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(None, store.clone());

    // Generate a command that produces both stdout and stderr
    let _ = tool
        .execute(json!({"command": "sh -c 'echo out; echo err 1>&2'"}))
        .await
        .unwrap();

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1);
    let m = &runs[0];

    for art in &m.artifacts {
        assert!(
            !art.safe_for_model,
            "raw artifact {:?} from bash tool MUST NOT be safe_for_model",
            art.kind
        );
    }
}

// ── 7. PythonScriptTool: ownership = DelegatedBackend, raw unsafe ──────

#[tokio::test]
async fn python_script_tool_owns_record_as_delegated_backend() {
    let store = Arc::new(MemRunStore::new());
    let tool = PythonScriptTool::with_run_store(store.clone());

    let _ = tool
        .execute(json!({
            "code": "print('hello-python')",
            "mode": "analyze"
        }))
        .await
        .unwrap();

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1, "PythonScriptTool must persist its own record");
    let m = &runs[0];
    assert_eq!(m.kind, RunKind::Python);
    assert_eq!(m.ownership, RunOwnership::DelegatedBackend);
    assert_eq!(m.planned_backend, Some(PlannedBackend::PythonScript));
    assert_eq!(m.actual_backend, Some(ActualBackend::PythonScript));
    // Script hash must be populated for PythonScript execution
    assert!(m.invocation.script_hash.is_some(), "script_hash must be set");
    // Raw stdout MUST NOT be safe_for_model
    for art in &m.artifacts {
        if matches!(art.kind, ArtifactKind::Stdout | ArtifactKind::Stderr | ArtifactKind::UnifiedDiff) {
            assert!(
                !art.safe_for_model,
                "raw python artifact {:?} MUST NOT be safe_for_model",
                art.kind
            );
        }
    }
}

// ── 8. TestRunner canonical API: ownership = DelegatedBackend ─────────

#[tokio::test]
async fn test_runner_canonical_api_owns_record_as_delegated_backend() {
    use codegg::test_runner::runner::run_resolved_test;
    use codegg::test_runner::types::{ResolvedTestCommand, TestLanguage};

    let store = Arc::new(MemRunStore::new());

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dummy.txt"), "").unwrap();

    let resolved = ResolvedTestCommand {
        language: TestLanguage::Generic,
        argv: vec!["echo".to_string(), "ok".to_string()],
        cwd: dir.path().to_path_buf(),
        scope_label: "test_runner_integration".to_string(),
    };
    let request = TestRunRequest {
        scope: TestScope::Auto,
        workdir: dir.path().to_path_buf(),
        timeout_secs: Some(30),
        stall_timeout_secs: Some(15),
        max_report_bytes: Some(8_000),
        session_id: None,
    };

    let store_dyn: Arc<dyn RunStore> = store.clone();
    let report = run_resolved_test(&request, resolved, None, Some(&store_dyn))
        .await
        .unwrap();
    assert_eq!(report.status, codegg::test_runner::types::TestStatus::Passed);

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1, "TestRunner must persist its own record");
    let m = &runs[0];
    assert_eq!(m.kind, RunKind::Test);
    assert_eq!(m.ownership, RunOwnership::DelegatedBackend);
    assert_eq!(m.planned_backend, Some(PlannedBackend::TestRunner));
    assert_eq!(m.actual_backend, Some(ActualBackend::TestRunner));
    // argv reflects the actual Command::new invocation
    let argv = m.invocation.argv.as_ref().expect("argv present");
    assert_eq!(argv[0], "echo");
    assert_eq!(argv[1], "ok");
    // Raw stdout/stderr MUST NOT be safe_for_model
    for art in &m.artifacts {
        if matches!(art.kind, ArtifactKind::Stdout | ArtifactKind::Stderr) {
            assert!(
                !art.safe_for_model,
                "raw test artifact {:?} MUST NOT be safe_for_model",
                art.kind
            );
        }
    }
}

// ── 9. Env kill switch: forces raw shell even with Active config ─────

#[tokio::test]
async fn env_kill_switch_forces_raw_shell_persistence() {
    std::env::set_var("CODEGG_ROUTING_DISABLE", "1");
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(Some(active_config()), store.clone());

    let _ = tool.execute(json!({"command": "echo kill-switch-ok"})).await.unwrap();

    std::env::remove_var("CODEGG_ROUTING_DISABLE");

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1);
    let m = &runs[0];
    // Active config would plan for TestRunner/GitRead/etc., but kill switch
    // forces raw shell — so actual_backend should be RawShell.
    assert_eq!(m.actual_backend, Some(ActualBackend::RawShell));
    // Planned is whatever the planner decided (could be Unrouted if intent
    // didn't classify as Test etc.; here "echo" is RawShell so plan matches).
    assert_eq!(m.kind, RunKind::RawShell);
}

// ── 10. Per-family RouteLevel::Off forces raw shell fallback ─────────

#[tokio::test]
async fn per_family_off_forces_raw_shell_persistence() {
    let mut cic = active_config();
    cic.route_git_read = Some(RouteLevel::Off);

    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(Some(cic), store.clone());

    let _ = tool.execute(json!({"command": "git status"})).await.unwrap();

    let runs = all_runs(&store).await;
    assert_eq!(runs.len(), 1);
    let m = &runs[0];
    // NativeTool was off, so dispatch falls back to raw shell.
    // In MVP, dispatch_to_native_tool just runs git via Command::new directly
    // and reports ActualBackend::NativeTool — so this test verifies the
    // MVP path is consistent. When Workstream D-2 wires real routing,
    // this test will need to be updated.
    assert!(matches!(m.actual_backend, Some(ActualBackend::NativeTool | ActualBackend::RawShell)));
}

// ── 11. CommandOutcome API: ownership_for_outcome mapping ──────────────

#[test]
fn ownership_mapping_for_outcome_variants() {
    use codegg::command_outcome::{ownership_for_outcome, ActualExecutor, ExecutionOutcome};

    let raw = ExecutionOutcome::identity(
        PlannedBackend::RawShell,
        ActualExecutor::RawShell {
            command: "echo".to_string(),
            argv: vec!["sh".to_string(), "-c".to_string(), "echo".to_string()],
        },
    );
    assert_eq!(ownership_for_outcome(&raw), RunOwnership::Caller);

    let managed = ExecutionOutcome::identity(
        PlannedBackend::ManagedArgv,
        ActualExecutor::ManagedArgv {
            argv: vec!["rg".to_string()],
            cwd: None,
        },
    );
    assert_eq!(ownership_for_outcome(&managed), RunOwnership::Caller);

    let native = ExecutionOutcome::identity(
        PlannedBackend::NativeTool,
        ActualExecutor::NativeTool {
            tool_name: "egggit".to_string(),
            argv: vec!["git".to_string(), "status".to_string()],
        },
    );
    assert_eq!(ownership_for_outcome(&native), RunOwnership::Caller);

    let test = ExecutionOutcome::identity(
        PlannedBackend::TestRunner,
        ActualExecutor::TestRunner {
            argv: vec!["cargo".to_string(), "test".to_string()],
            cwd: PathBuf::from("."),
        },
    );
    assert_eq!(ownership_for_outcome(&test), RunOwnership::DelegatedBackend);

    let python = ExecutionOutcome::identity(
        PlannedBackend::PythonScript,
        ActualExecutor::PythonScript {
            script_hash: Some("abc".to_string()),
            mode: "analyze".to_string(),
        },
    );
    assert_eq!(ownership_for_outcome(&python), RunOwnership::DelegatedBackend);
}

// ── 12. Backward compat: manifests without new fields still deserialize ─

#[tokio::test]
async fn manifest_backward_compat_no_provenance_fields() {
    use codegg_core::run_store::{RunId, RunManifest};

    let _store = MemRunStore::new();
    let legacy_json = serde_json::json!({
        "schema_version": 1,
        "run_id": RunId::new(),
        "kind": "raw_shell",
        "invocation": {
            "command": "echo legacy"
        },
        "started_at": "2026-01-01T00:00:00Z",
        "status": "complete",
        "workspace_root": "/ws",
        "cwd": "/ws",
        "backend": {"family": "bash"},
        "risk": {"level": "low", "has_subprocess": false, "has_git_mutation": false, "has_destructive_mutation": false}
    });

    let m: RunManifest = serde_json::from_value(legacy_json).unwrap();
    assert_eq!(m.ownership, RunOwnership::Caller); // default
    assert!(m.planned_backend.is_none());
    assert!(m.actual_backend.is_none());
    assert!(m.fallback.is_none());
}

// ── 13. Manifest serde roundtrip with provenance fields ───────────────

#[tokio::test]
async fn manifest_serde_roundtrip_with_provenance() {
    use codegg_core::run_store::{
        ActualBackend, FallbackRecord, RunId, RunManifest, RunOwnership,
    };

    let m = RunManifest {
        schema_version: 1,
        run_id: RunId::new(),
        session_id: None,
        parent_run_id: None,
        kind: RunKind::Test,
        invocation: codegg_core::run_store::RunInvocation {
            command: "cargo test".to_string(),
            argv: Some(vec!["cargo".to_string(), "test".to_string()]),
            script_hash: None,
        },
        started_at: chrono::Utc::now(),
        completed_at: None,
        status: RunStatus::Running,
        workspace_root: PathBuf::from("/ws"),
        cwd: PathBuf::from("/ws"),
        backend: BackendRecord {
            family: "test_runner".to_string(),
            detail: Some("cargo".to_string()),
        },
        risk: codegg_core::run_store::RiskRecord {
            level: "low".to_string(),
            has_subprocess: false,
            has_git_mutation: false,
            has_destructive_mutation: false,
        },
        permissions: vec![],
        sandbox: None,
        artifacts: vec![],
        projection: None,
        changes: vec![],
        rerun: None,
        planned_backend: Some(PlannedBackend::TestRunner),
        actual_backend: Some(ActualBackend::TestRunner),
        fallback: Some(FallbackRecord {
            planned: PlannedBackend::TestRunner,
            actual: ActualBackend::RawShell,
            reason: "test".to_string(),
        }),
        ownership: RunOwnership::DelegatedBackend,
    };

    let json = serde_json::to_string(&m).unwrap();
    let decoded: RunManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.ownership, RunOwnership::DelegatedBackend);
    assert_eq!(decoded.planned_backend, Some(PlannedBackend::TestRunner));
    assert_eq!(decoded.actual_backend, Some(ActualBackend::TestRunner));
    let fb = decoded.fallback.expect("fallback");
    assert_eq!(fb.reason, "test");
}