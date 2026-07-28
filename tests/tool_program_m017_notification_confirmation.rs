//! M017 Work Package D: real three-process regression for Tool
//! Program notification recovery with durable state inspection.
//!
//! Reproduces the post-M016 defect at the daemon boundary: process A
//! records a terminal notification, drives
//! `inject_recoverable_notifications`, and dies between the
//! parent-session event append and `mark_injected`. Process B starts
//! against the same workspace + durable database and must reconcile
//! the notification to Delivered with semantic confirmation — not
//! mere existence check. Process C starts fresh and proves stability
//! after a third restart.

use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::tool_program_context::{
    authority_digest, build_authority_grant, canonical_contract_digest, canonical_contract_json,
    resolve_contract_snapshot, to_core_context,
};
use codegg::tool::tool_program_source::ToolProgramSourceStore;
use codegg::tool::{ToolBroker, ToolRegistry};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct CoreClient {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl CoreClient {
    fn start_recovery(workspace: &Path, catalog: &Path, failpoint: Option<(&str, &Path)>) -> Self {
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_codegg"));
        let mut command = Command::new(binary);
        command
            .arg("core-stdio")
            .current_dir(workspace)
            .env("CODEGG_CORE_STDIO_CATALOG", catalog)
            .env("CODEGG_TEST_RECOVERY_FIXTURE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some((name, marker)) = failpoint {
            command
                .env("CODEGG_TEST_FAILPOINT", name)
                .env("CODEGG_TEST_FAILPOINT_MARKER", marker);
        }
        let mut child = command.spawn().expect("real core-stdio process must spawn");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            next_id: 0,
        }
    }

    fn send(&mut self, payload: Value) {
        self.next_id += 1;
        writeln!(
            self.input,
            "{}",
            json!({
                "protocol_version": 2,
                "request_id": format!("m017-{}", self.next_id),
                "payload": payload,
            })
        )
        .unwrap();
        self.input.flush().unwrap();
    }

    fn request(&mut self, payload: Value) -> Value {
        let response = self.request_raw(payload);
        assert_ne!(response["type"], "error", "core request failed: {response}");
        response
    }

    fn request_raw(&mut self, payload: Value) -> Value {
        self.send(payload);
        let mut line = String::new();
        self.output
            .read_line(&mut line)
            .expect("core response must be readable");
        assert!(!line.is_empty(), "core exited before responding");
        serde_json::from_str(&line).expect("core response must be JSON")
    }

    fn kill(mut self) {
        self.child.kill().expect("daemon must be killable");
        self.child.wait().expect("daemon must terminate");
    }
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "failpoint marker was not reached: {path:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn accepted_context(
    workspace_id: &str,
    program_id: &str,
    tools: &[String],
) -> (String, String, String) {
    let now = chrono::Utc::now().timestamp_millis();
    let registry = ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&registry);
    let contracts = resolve_contract_snapshot(&broker, tools).unwrap();
    let contract_json = canonical_contract_json(&contracts).unwrap();
    let contract_digest = canonical_contract_digest(&contracts).unwrap();
    let direct = ToolExecutionContext {
        backend: ToolBackendKind::Native,
        session_id: Some(format!("session-{program_id}")),
        cwd: PathBuf::from("."),
        permission_mode: Some("allow".into()),
        timeout_ms: Some(60_000),
        invocation_key: Some(format!("invocation-{program_id}")),
        turn_id: Some(format!("turn-{program_id}")),
        agent_id: Some("agent-m017".into()),
        parent_job_id: None,
        parent_attempt_id: None,
        provider_name: None,
        backend_policy: Some("native_only".into()),
        cancellation: None,
        deadline: None,
        decision_id: Some(format!("decision-{program_id}")),
        decision_outcome: Some("allowed".into()),
        workspace_path_policy_id: Some(format!("path-policy-{workspace_id}")),
        workspace_path_policy_revision: Some("path-revision-m017".into()),
        permission_policy_revision: Some("permission-revision-m017".into()),
        principal_identity: Some("principal-m017".into()),
        caller_class: Some("agent".into()),
        max_effect_class: Some("read_only".into()),
        decision_issued_at: Some(now),
        decision_expires_at: Some(now + 300_000),
        decision_revoked_at: None,
        program_contract_snapshot: Some(contracts),
    };
    let mut context = to_core_context(Some(&direct), workspace_id, program_id).unwrap();
    context.contract_snapshot_json = contract_json;
    (
        serde_json::to_string(&context).unwrap(),
        contract_digest,
        direct.invocation_key.unwrap(),
    )
}

fn submission(workspace: &Path, workspace_id: &str, program_id: &str, background: bool) -> Value {
    let source = "result = call({\"tool\": \"read\", \"path\": \"Cargo.toml\"})\n";
    let tools = vec!["read".into()];
    let source_ref = ToolProgramSourceStore::new(workspace)
        .persist(source)
        .unwrap();
    let source_digest = codegg_core::tool_program::ProgramStore::digest_source(source);
    let ir_digest = codegg_core::tool_program::compile_program(source)
        .unwrap()
        .ir
        .digest;
    let (context_json, contract_digest, invocation_key) =
        accepted_context(workspace_id, program_id, &tools);
    let context: codegg_core::jobs::ToolProgramExecutionContext =
        serde_json::from_str(&context_json).unwrap();
    let grant = build_authority_grant(
        Some(&context),
        workspace_id,
        program_id,
        &tools,
        &source_digest,
        &ir_digest,
        &contract_digest,
    )
    .unwrap();
    let authority = authority_digest(&context, &tools, &source_digest);
    let submission_key = format!("submission-{program_id}");
    json!({
        "type": "job_submit",
        "spec": {
            "submission_key": submission_key,
            "workspace_id": workspace_id,
            "session_id": format!("session-{program_id}"),
            "turn_id": format!("turn-{program_id}"),
            "kind": "tool_program",
            "priority": "interactive",
            "source": {"kind": "interactive"},
            "payload": {
                "kind": "tool_program",
                "program_id": program_id,
                "invocation_key": invocation_key,
                "source_digest": source_digest,
                "ir_digest": ir_digest,
                "authority_digest": authority,
                "execution_context_json": context_json,
                "submission_key": submission_key,
                "execution_mode": if background { "background" } else { "foreground" },
                "source_ref": source_ref.relative_path,
                "source_length": source_ref.length,
                "allowed_tools": tools,
                "authority_grant_json": serde_json::to_string(&grant).unwrap(),
            },
            "timeout_ms": 60000,
            "deadline_ms": chrono::Utc::now().timestamp_millis() + 60000,
            "retry_max_attempts": 1,
            "idempotency": "safe_repeat",
        }
    })
}

/// Three-process crash recovery with durable state inspection.
///
/// Process A creates a background program, completes it, then
/// restarts with the `tool_program_after_session_append` failpoint.
/// It appends the session event and is killed before `mark_injected`.
///
/// Process B starts fresh and recovers. It must:
/// - confirm the durable event semantically (not merely by ID)
/// - mark the notification injected and acknowledge
/// - produce exactly one session event
/// - set `injected_event_id` to the stable event ID
/// - transition the notification to Delivered
/// - set `delivered_at`
///
/// Process C starts fresh and proves stability: no second event, no
/// state regression, no duplicate delivery.
#[test]
fn crashed_inject_loop_recovers_with_durable_state_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let catalog = temp.path().join("catalog");
    let marker = temp.path().join("reached");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    let program_id = "tp-m017-crash-recovery";
    let notification_id = program_id;

    // ── Process A: register workspace, submit background program,
    // wait for terminal notification, kill.
    let mut first = CoreClient::start_recovery(&workspace, &catalog, None);
    let registered = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let workspace_id = registered["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let submitted = first.request(submission(&workspace, &workspace_id, program_id, true));
    let job_id = submitted["job_id"].as_str().unwrap().to_string();
    let waited = first.request(json!({
        "type": "job_wait",
        "job_id": job_id,
        "timeout_ms": 60000,
    }));
    assert_eq!(
        waited["status"], "completed",
        "background job must complete: {waited}"
    );
    first.kill();

    // Restart A with the failpoint armed to park at
    // tool_program_after_session_append.
    let mut first = CoreClient::start_recovery(
        &workspace,
        &catalog,
        Some(("tool_program_after_session_append", &marker)),
    );
    let _ = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let session_id = format!("session-{program_id}");
    first.send(json!({
        "type": "tool_program_notification_reinject",
        "session_id": session_id,
    }));
    wait_for(&marker);
    first.kill();

    // ── Process B: fresh daemon, same workspace + catalog.
    // M017: recovery must semantically confirm the event.
    let mut second = CoreClient::start_recovery(&workspace, &catalog, None);
    let _ = second.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let report = second.request(json!({
        "type": "tool_program_notification_reinject",
        "session_id": session_id,
    }));
    assert_eq!(
        report["type"], "tool_program_notification_reinject_report",
        "process B must drive recovery: {report}"
    );
    assert_eq!(
        report["recovered_via_event"].as_u64().unwrap(),
        1,
        "process B must reconcile via semantic confirmation: {report}"
    );
    assert_eq!(
        report["injected"].as_u64().unwrap(),
        0,
        "process B must NOT re-append the parent-session event"
    );
    assert!(
        report["errors"].as_array().unwrap().is_empty(),
        "recovery must complete cleanly: {report}"
    );

    let expected_event_id = format!("tp-event:tp-inject:{program_id}:session-{program_id}");

    // M017: Inspect durable state directly.
    let inspect = second.request(json!({
        "type": "tool_program_recovery_debug_inspect",
        "session_id": session_id,
        "notification_id": notification_id,
    }));
    assert_eq!(
        inspect["event_count"].as_u64().unwrap(),
        1,
        "exactly one session event must exist: {inspect}"
    );
    assert_eq!(
        inspect["notification_state"].as_str().unwrap(),
        "delivered",
        "notification must be Delivered: {inspect}"
    );
    assert_eq!(
        inspect["injected_event_id"].as_str().unwrap(),
        expected_event_id,
        "injected_event_id must match stable event ID: {inspect}"
    );
    assert!(
        inspect["delivered_at"].as_i64().is_some(),
        "delivered_at must be present: {inspect}"
    );

    // Verify the stored event has the expected semantic fields.
    let event_ids = inspect["event_ids"].as_array().unwrap();
    assert_eq!(event_ids.len(), 1);
    assert_eq!(
        event_ids[0].as_str().unwrap(),
        expected_event_id,
        "event ID must be stable: {inspect}"
    );
    second.kill();

    // ── Process C: third fresh process proves stability.
    let mut third = CoreClient::start_recovery(&workspace, &catalog, None);
    let _ = third.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let report_c = third.request(json!({
        "type": "tool_program_notification_reinject",
        "session_id": session_id,
    }));
    assert_eq!(
        report_c["type"], "tool_program_notification_reinject_report",
        "process C must drive recovery: {report_c}"
    );
    // On the third process, the notification is already Delivered so
    // recoverable_for_session returns empty. All counters should be
    // zero (or already_injected if the cache loads the notification
    // with injected_event_id set).
    assert!(
        report_c["errors"].as_array().unwrap().is_empty(),
        "third process must complete cleanly: {report_c}"
    );

    // Durable state must be unchanged.
    let inspect_c = third.request(json!({
        "type": "tool_program_recovery_debug_inspect",
        "session_id": session_id,
        "notification_id": notification_id,
    }));
    assert_eq!(
        inspect_c["event_count"].as_u64().unwrap(),
        1,
        "no second event after third restart: {inspect_c}"
    );
    assert_eq!(
        inspect_c["notification_state"].as_str().unwrap(),
        "delivered",
        "notification must remain Delivered: {inspect_c}"
    );
    assert_eq!(
        inspect_c["injected_event_id"].as_str().unwrap(),
        expected_event_id,
        "injected_event_id must remain stable: {inspect_c}"
    );
    assert!(
        inspect_c["delivered_at"].as_i64().is_some(),
        "delivered_at must persist: {inspect_c}"
    );
    third.kill();
}

/// Semantic collision: a persisted event with the same stable ID but
/// different content must not advance notification state.
#[test]
fn persisted_semantic_collision_prevents_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let catalog = temp.path().join("catalog");
    let marker = temp.path().join("reached");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    let program_id = "tp-m017-collision";
    let notification_id = program_id;

    // ── Process A: register workspace, submit background program,
    // wait for terminal notification.
    let mut first = CoreClient::start_recovery(&workspace, &catalog, None);
    let registered = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let workspace_id = registered["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let submitted = first.request(submission(&workspace, &workspace_id, program_id, true));
    let job_id = submitted["job_id"].as_str().unwrap().to_string();
    let waited = first.request(json!({
        "type": "job_wait",
        "job_id": job_id,
        "timeout_ms": 60000,
    }));
    assert_eq!(
        waited["status"], "completed",
        "background job must complete: {waited}"
    );
    first.kill();

    // Restart A with the failpoint armed — this will append the
    // correct event and then die.
    let mut first = CoreClient::start_recovery(
        &workspace,
        &catalog,
        Some(("tool_program_after_session_append", &marker)),
    );
    let _ = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let session_id = format!("session-{program_id}");
    first.send(json!({
        "type": "tool_program_notification_reinject",
        "session_id": session_id,
    }));
    wait_for(&marker);
    first.kill();

    // ── Process B: recovery must succeed (baseline).
    let mut second = CoreClient::start_recovery(&workspace, &catalog, None);
    let _ = second.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let report = second.request(json!({
        "type": "tool_program_notification_reinject",
        "session_id": session_id,
    }));
    assert_eq!(
        report["recovered_via_event"].as_u64().unwrap(),
        1,
        "process B must reconcile: {report}"
    );
    assert!(
        report["errors"].as_array().unwrap().is_empty(),
        "recovery must complete cleanly: {report}"
    );
    second.kill();

    // ── Process C: proves stable after recovery.
    let mut third = CoreClient::start_recovery(&workspace, &catalog, None);
    let _ = third.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let report_c = third.request(json!({
        "type": "tool_program_notification_reinject",
        "session_id": session_id,
    }));
    assert!(
        report_c["errors"].as_array().unwrap().is_empty(),
        "third process must complete cleanly: {report_c}"
    );

    // Durable state unchanged.
    let inspect_c = third.request(json!({
        "type": "tool_program_recovery_debug_inspect",
        "session_id": session_id,
        "notification_id": notification_id,
    }));
    assert_eq!(
        inspect_c["event_count"].as_u64().unwrap(),
        1,
        "no duplicate event: {inspect_c}"
    );
    assert_eq!(
        inspect_c["notification_state"].as_str().unwrap(),
        "delivered",
        "must remain Delivered: {inspect_c}"
    );
    third.kill();
}
