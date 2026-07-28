//! Real `core-stdio` process recovery at production Tool Program failpoints.

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
    fn start(workspace: &Path, catalog: &Path, failpoint: Option<(&str, &Path)>) -> Self {
        Self::start_with_fixture(workspace, catalog, failpoint, false)
    }

    fn start_recovery(workspace: &Path, catalog: &Path, failpoint: Option<(&str, &Path)>) -> Self {
        Self::start_with_fixture(workspace, catalog, failpoint, true)
    }

    fn start_with_fixture(
        workspace: &Path,
        catalog: &Path,
        failpoint: Option<(&str, &Path)>,
        recovery_fixture: bool,
    ) -> Self {
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_codegg"));
        let mut command = Command::new(binary);
        command
            .arg("core-stdio")
            .current_dir(workspace)
            .env("CODEGG_CORE_STDIO_CATALOG", catalog)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if recovery_fixture {
            command.env("CODEGG_TEST_RECOVERY_FIXTURE", "1");
        }
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
                "request_id": format!("m015-{}", self.next_id),
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
        self.child.kill().expect("daemon A must be killable");
        self.child.wait().expect("daemon A must terminate");
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "failpoint marker was not reached"
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
        agent_id: Some("agent-m015-daemon".into()),
        parent_job_id: None,
        parent_attempt_id: None,
        provider_name: None,
        backend_policy: Some("native_only".into()),
        cancellation: None,
        deadline: None,
        decision_id: Some(format!("decision-{program_id}")),
        decision_outcome: Some("allowed".into()),
        workspace_path_policy_id: Some(format!("path-policy-{workspace_id}")),
        workspace_path_policy_revision: Some("path-revision-m015".into()),
        permission_policy_revision: Some("permission-revision-m015".into()),
        principal_identity: Some("principal-m015-daemon".into()),
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
    submission_for_source(
        workspace,
        workspace_id,
        program_id,
        background,
        "result = call({\"tool\": \"read\", \"path\": \"Cargo.toml\"})\n",
        vec!["read".into()],
    )
}

fn submission_for_source(
    workspace: &Path,
    workspace_id: &str,
    program_id: &str,
    background: bool,
    source: &str,
    tools: Vec<String>,
) -> Value {
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

fn recover_at(failpoint: &str, background: bool) {
    recover_source_at(
        failpoint,
        background,
        "result = call({\"tool\": \"read\", \"path\": \"Cargo.toml\"})\n",
        vec!["read".into()],
    );
}

fn recover_source_at(failpoint: &str, background: bool, source: &str, tools: Vec<String>) {
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
    let program_id = format!("tp-m015-{}", failpoint.replace('_', "-"));

    let mut first = CoreClient::start_recovery(&workspace, &catalog, Some((failpoint, &marker)));
    let registered = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let workspace_id = registered["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let submitted_request = submission_for_source(
        &workspace,
        &workspace_id,
        &program_id,
        background,
        source,
        tools,
    );
    let expected_deadline = submitted_request["spec"]["deadline_ms"].as_i64().unwrap();
    first.send(submitted_request);
    wait_for(&marker);
    first.kill();

    let mut second = CoreClient::start_recovery(&workspace, &catalog, None);
    let registered = second.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let recovered_workspace_id = registered["workspace"]["workspace_id"].as_str().unwrap();
    assert_eq!(recovered_workspace_id, workspace_id);
    let listed = second.request(json!({
        "type": "job_list",
        "query": {
            "workspace_id": workspace_id,
            "kinds": ["tool_program"],
            "limit": 20,
        }
    }));
    let jobs = listed["jobs"].as_array().expect("job list response");
    assert_eq!(
        jobs.len(),
        1,
        "restart must retain exactly one job: {listed}"
    );
    let job_id = jobs[0]["job_id"].as_str().unwrap();
    let recovered_job = second.request(json!({
        "type": "job_get",
        "job_id": job_id,
    }));
    assert_eq!(
        recovered_job["job"]["deadline"].as_i64(),
        Some(expected_deadline),
        "restart must preserve the original absolute deadline"
    );
    let waited = second.request(json!({
        "type": "job_wait",
        "job_id": job_id,
        "timeout_ms": 60000,
    }));
    if waited["status"] != "completed" {
        let attempts = second.request(json!({
            "type": "job_attempts",
            "job_id": job_id,
        }));
        let detail = second.request(json!({
            "type": "tool_program_inspect",
            "program_id": program_id,
        }));
        let durable_result = std::fs::read_to_string(
            workspace
                .join(".codegg/tool_program_results")
                .join(format!("{program_id}.json")),
        )
        .unwrap_or_default();
        panic!(
            "recovery failed: {waited}; attempts={attempts}; detail={detail}; result={durable_result}"
        );
    }
    let calls = second.request(json!({
        "type": "tool_program_call_page",
        "program_id": program_id,
        "offset": 0,
    }));
    assert_eq!(calls["page"]["total_calls"], 1, "call must be exact-once");
    second.stop();
}

#[test]
fn public_job_submit_cannot_fabricate_tool_program_authority() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let catalog = temp.path().join("catalog");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();

    let mut core = CoreClient::start(&workspace, &catalog, None);
    let registered = core.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let workspace_id = registered["workspace"]["workspace_id"].as_str().unwrap();
    let rejected = core.request_raw(submission(
        &workspace,
        workspace_id,
        "tp-m015-forged-public-authority",
        false,
    ));
    assert_eq!(rejected["type"], "error");
    assert_eq!(rejected["code"], "invalid_job_submit");

    let listed = core.request(json!({
        "type": "job_list",
        "query": {
            "workspace_id": workspace_id,
            "kinds": ["tool_program"],
            "limit": 20,
        }
    }));
    assert_eq!(listed["jobs"].as_array().unwrap().len(), 0);
    core.stop();
}

#[test]
fn accepted_authority_and_call_recover_after_job_persist_crash() {
    recover_at("tool_program_after_job_persist", false);
}

#[test]
fn completed_call_replays_after_checkpoint_window_crash() {
    recover_at("tool_program_after_calls_persist", false);
}

#[test]
fn committed_result_recovers_terminal_notification_after_crash() {
    recover_at("tool_program_after_result_persist", true);
}

#[test]
fn active_child_is_reattached_from_pending_wait_checkpoint() {
    recover_source_at(
        "tool_program_after_child_wait_checkpoint",
        false,
        "result = submit_job(\"test\", {\"scope\": \"workspace\"})\n",
        vec!["read".into()],
    );
}

fn corrupt_committed_state_is_rejected(failpoint: &str, relative: PathBuf, background: bool) {
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
    let program_id = format!("tp-m015-corrupt-{}", failpoint.replace('_', "-"));
    let mut first = CoreClient::start_recovery(&workspace, &catalog, Some((failpoint, &marker)));
    let registered = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let workspace_id = registered["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    first.send(submission(
        &workspace,
        &workspace_id,
        &program_id,
        background,
    ));
    wait_for(&marker);
    first.kill();
    std::fs::write(workspace.join(relative), b"{corrupt").unwrap();

    let mut second = CoreClient::start_recovery(&workspace, &catalog, None);
    let listed = second.request(json!({
        "type": "job_list",
        "query": {"workspace_id": workspace_id, "kinds": ["tool_program"], "limit": 10}
    }));
    let job_id = listed["jobs"][0]["job_id"].as_str().unwrap();
    let waited = second.request(json!({
        "type": "job_wait",
        "job_id": job_id,
        "timeout_ms": 60000,
    }));
    assert_eq!(waited["status"], "failed", "corruption must fail closed");
    second.stop();
}

#[test]
fn corrupt_checkpoint_journal_is_rejected_after_restart() {
    let program_id = "tp-m015-corrupt-tool-program-after-calls-persist".to_string();
    corrupt_committed_state_is_rejected(
        "tool_program_after_calls_persist",
        PathBuf::from(format!(
            ".codegg/tool_program_calls/{program_id}.journal.json"
        )),
        false,
    );
}

#[test]
fn corrupt_committed_result_is_rejected_after_restart() {
    let program_id = "tp-m015-corrupt-tool-program-after-result-persist".to_string();
    corrupt_committed_state_is_rejected(
        "tool_program_after_result_persist",
        PathBuf::from(format!(".codegg/tool_program_results/{program_id}.json")),
        true,
    );
}

#[test]
fn recursive_descendants_and_capacity_converge_after_cancel_crash() {
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
    let program_id = "tp-m015-descendant-process";
    let mut first = CoreClient::start_recovery(
        &workspace,
        &catalog,
        Some(("tool_program_after_descendant_cancel", &marker)),
    );
    let registered = first.request(json!({
        "type": "workspace_register",
        "root": workspace,
    }));
    let workspace_id = registered["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let submitted = first.request(submission_for_source(
        &workspace,
        &workspace_id,
        program_id,
        false,
        "result = submit_job(\"build\", {\"argv\": [\"bash\", \"-lc\", \"sleep 30\"]})\n",
        vec!["read".into()],
    ));
    let parent_job_id = submitted["job_id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let listed = first.request(json!({
            "type": "job_list",
            "query": {"workspace_id": workspace_id, "limit": 20}
        }));
        if listed["jobs"]
            .as_array()
            .is_some_and(|jobs| jobs.len() >= 2)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "child job did not become durable"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    first.send(json!({
        "type": "job_cancel",
        "job_id": parent_job_id,
        "reason": "m015 descendant convergence",
    }));
    wait_for(&marker);
    first.kill();

    let mut second = CoreClient::start_recovery(&workspace, &catalog, None);
    let listed = second.request(json!({
        "type": "job_list",
        "query": {"workspace_id": workspace_id, "limit": 20}
    }));
    let jobs = listed["jobs"].as_array().unwrap();
    assert!(
        jobs.iter().all(|job| matches!(
            job["state"].as_str(),
            Some("cancelled" | "failed" | "interrupted" | "timed_out" | "completed")
        )),
        "all descendants must be terminal after restart: {listed}"
    );

    let unrelated = "tp-m015-capacity-after-cancel";
    let submitted = second.request(submission(&workspace, &workspace_id, unrelated, false));
    let waited = second.request(json!({
        "type": "job_wait",
        "job_id": submitted["job_id"],
        "timeout_ms": 60000,
    }));
    assert_eq!(
        waited["status"], "completed",
        "capacity must return after descendant convergence"
    );
    second.stop();
}
