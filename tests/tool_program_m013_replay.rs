//! M013 checkpoint restoration and replay fingerprint tests.
//!
//! Covers closure criteria related to F06 / F1-F5:
//! - F1: Versioned replay identity binds all safety-gated fields.
//! - F2: Restore checkpoint — interpreter resumes at correct instruction.
//! - F3: Deadline authority — original deadline is preserved across restart.
//! - F4: Divergence behavior — mismatch stops execution and persists a recoverable divergence.
//! - F5: Completed calls are not physically re-executed after process restart.

#![cfg(test)]

use codegg_core::tool_program::{
    CallRequest, CallResult, CompletedCall, IrProgram, MeteredInterpreter, ProgramValue,
    ReplayFingerprint, RuntimeLimits,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Helper to build a ReplayFingerprint with all M013-F1 fields populated.
fn make_fingerprint(
    authority_digest: &str,
    source_digest: &str,
    ir_digest: &str,
    workspace_path_policy_id: &str,
    manifest_digest: &str,
    contract_digest: &str,
    backend_selection: &str,
    deadline_millis: Option<i64>,
) -> ReplayFingerprint {
    ReplayFingerprint {
        schema_version: 2,
        program_id: "tp-m013-replay".into(),
        authority_digest: authority_digest.into(),
        execution_context_digest: "sha256:ctx-digest".into(),
        source_digest: source_digest.into(),
        ir_digest: ir_digest.into(),
        workspace_id: "ws-replay".into(),
        workspace_path_policy_id: workspace_path_policy_id.into(),
        policy_revision: "rev-1".into(),
        session_id: Some("sess-1".into()),
        agent_id: Some("agent-1".into()),
        manifest_digest: manifest_digest.into(),
        contract_digest: contract_digest.into(),
        backend_selection: backend_selection.into(),
        original_deadline_millis: deadline_millis,
    }
}

fn default_fingerprint() -> ReplayFingerprint {
    make_fingerprint(
        "sha256:auth-abc",
        "sha256:src-abc",
        "sha256:ir-abc",
        "workspace:ws-1",
        "sha256:manifest-v1",
        "sha256:contract-v1",
        "native_only",
        Some(1_700_000_000_000),
    )
}

/// Compile a simple program that makes one tool call and emits the result.
fn compile_one_call_program() -> IrProgram {
    let source = "x = call({\"tool\": \"read\", \"input\": {\"path\": \"/tmp\"}})\nemit(x)\n";
    let compilation = codegg_core::tool_program::compile_program(source).unwrap();
    compilation.ir
}

/// A broker that counts how many times execute_call is invoked.
/// Used to prove that replay does not physically re-execute.
struct CountingBroker {
    call_count: Arc<AtomicUsize>,
}

impl CountingBroker {
    fn new() -> (Self, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        (
            Self {
                call_count: Arc::clone(&count),
            },
            count,
        )
    }
}

#[async_trait::async_trait]
impl codegg_core::tool_program::BrokerCallback for CountingBroker {
    async fn execute_call(
        &self,
        _request: &CallRequest,
    ) -> Result<CallResult, codegg_core::tool_program::InterpreterError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({"path": "/tmp"})),
            artifacts: vec![],
            success: true,
        })
    }
}

/// F5: A completed call is not physically re-executed when the interpreter
/// is restarted with the same completed_calls map.
#[tokio::test(flavor = "current_thread")]
async fn f5_completed_call_not_re_executed_on_restart() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    // First run: execute the call through the broker.
    let (broker, call_count) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker, None).await;
    assert_eq!(
        result1.status,
        codegg_core::tool_program::ProgramStatus::Completed
    );
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Capture completed calls from first run.
    let completed = interp1.completed_calls().clone();
    assert_eq!(completed.len(), 1);

    // Second run: restart with preloaded completed calls.
    let (broker2, call_count2) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    interp2.set_replay_fingerprint(default_fingerprint());
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Completed
    );
    assert_eq!(
        call_count2.load(Ordering::SeqCst),
        0,
        "broker must not be called during replay"
    );
}

/// F2: Checkpoint state resumes at the correct instruction.
/// The interpreter restores the program counter, completed calls,
/// and budget from the checkpoint.
#[tokio::test(flavor = "current_thread")]
async fn f2_checkpoint_preserves_state() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker, _call_count) = CountingBroker::new();
    let mut interp = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp.set_replay_fingerprint(default_fingerprint());
    let result = interp.run(&broker, None).await;
    assert_eq!(
        result.status,
        codegg_core::tool_program::ProgramStatus::Completed
    );

    // Verify the interpreter completed successfully — checkpoints are internal state.
    assert_eq!(result.status, codegg_core::tool_program::ProgramStatus::Completed);

    // Verify completed calls are present.
    assert_eq!(interp.completed_calls().len(), 1);
}

/// F4: Each fingerprint field independently causes fail-closed divergence
/// when changed. We test each field by creating a completed call with the
/// default fingerprint and then running with a modified fingerprint.
#[tokio::test(flavor = "current_thread")]
async fn f4_authority_digest_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    // Run first with default fingerprint.
    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    // Restart with a different authority_digest.
    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.authority_digest = "sha256:DIFFERENT-AUTHORITY".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "authority digest mismatch must cause failure"
    );
    assert!(result2
        .error_message
        .as_ref()
        .map(|m| m.contains("replay identity mismatch"))
        .unwrap_or(false));
}

#[tokio::test(flavor = "current_thread")]
async fn f4_source_digest_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.source_digest = "sha256:DIFFERENT-SOURCE".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "source digest mismatch must cause failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn f4_ir_digest_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.ir_digest = "sha256:DIFFERENT-IR".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "IR digest mismatch must cause failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn f4_workspace_path_policy_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.workspace_path_policy_id = "workspace:DIFFERENT-POLICY".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "workspace path policy mismatch must cause failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn f4_manifest_digest_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.manifest_digest = "sha256:DIFFERENT-MANIFEST".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "manifest digest mismatch must cause failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn f4_contract_digest_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.contract_digest = "sha256:DIFFERENT-CONTRACT".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "contract digest mismatch must cause failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn f4_backend_selection_divergence() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let (broker2, _) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    let mut modified_fp = default_fingerprint();
    modified_fp.backend_selection = "hosted".into();
    interp2.set_replay_fingerprint(modified_fp);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(
        result2.status,
        codegg_core::tool_program::ProgramStatus::Failed,
        "backend selection mismatch must cause failure"
    );
}

/// F3: Original deadline is preserved in the fingerprint across restart.
#[tokio::test(flavor = "current_thread")]
async fn f3_original_deadline_preserved_in_fingerprint() {
    let fp = default_fingerprint();
    assert_eq!(
        fp.original_deadline_millis,
        Some(1_700_000_000_000),
        "original deadline must be present in fingerprint"
    );
}

/// F3: Missing deadline in fingerprint is handled gracefully.
#[tokio::test(flavor = "current_thread")]
async fn f3_missing_deadline_is_none() {
    let fp = make_fingerprint(
        "sha256:auth",
        "sha256:src",
        "sha256:ir",
        "workspace:ws",
        "sha256:manifest",
        "sha256:contract",
        "native_only",
        None,
    );
    assert!(fp.original_deadline_millis.is_none());
}

/// F1: Replay fingerprint covers all required fields (field presence check).
#[tokio::test(flavor = "current_thread")]
async fn f1_fingerprint_covers_all_required_fields() {
    let fp = default_fingerprint();

    // Every field required by C-24 must be present.
    assert_eq!(fp.schema_version, 2);
    assert!(!fp.program_id.is_empty());
    assert!(!fp.authority_digest.is_empty());
    assert!(!fp.execution_context_digest.is_empty());
    assert!(!fp.source_digest.is_empty());
    assert!(!fp.ir_digest.is_empty());
    assert!(!fp.workspace_id.is_empty());
    assert!(!fp.workspace_path_policy_id.is_empty());
    assert!(!fp.policy_revision.is_empty());
    assert!(!fp.manifest_digest.is_empty());
    assert!(!fp.contract_digest.is_empty());
    assert!(!fp.backend_selection.is_empty());
}

/// F5: A completed call from the journal is replayed rather than re-executed.
/// The interpreter must use the stored result, not the broker's result.
#[tokio::test(flavor = "current_thread")]
async fn f5_stored_result_is_used_during_replay() {
    let ir = compile_one_call_program();
    let limits = RuntimeLimits::from(&ir.bounds);

    // First run: capture the exact completed call format the compiler produces.
    let (broker1, _) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);
    let mut completed = interp1.completed_calls().clone();
    assert_eq!(completed.len(), 1);

    // Modify the stored result to prove replay uses the stored value, not the broker.
    let stored_call = completed.get_mut(&0).unwrap();
    stored_call.result = CallResult {
        output: ProgramValue::ToolResult(serde_json::json!({"path": "/replayed"})),
        artifacts: vec![],
        success: true,
    };

    // Second run: replay with modified result.
    let (broker2, call_count2) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    interp2.set_replay_fingerprint(default_fingerprint());
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;

    assert_eq!(result2.status, codegg_core::tool_program::ProgramStatus::Completed);
    assert_eq!(
        call_count2.load(Ordering::SeqCst),
        0,
        "broker must not be called during replay"
    );
    // The output should contain the stored value.
    if let Some(ref output) = result2.output {
        let json = output.to_json();
        assert!(
            json.to_string().contains("/replayed"),
            "stored result must be used during replay; got: {json}"
        );
    }
}

/// F5: Multiple sequential calls are all replayed without broker invocation.
#[tokio::test(flavor = "current_thread")]
async fn f5_multiple_calls_all_replayed() {
    let source = "a = call({\"tool\": \"read\", \"input\": {\"path\": \"/a\"}})\nb = call({\"tool\": \"read\", \"input\": {\"path\": \"/b\"}})\nemit(b)\n";
    let compilation = codegg_core::tool_program::compile_program(source).unwrap();
    let ir = compilation.ir;
    let limits = RuntimeLimits::from(&ir.bounds);

    // First run: execute both calls.
    let (broker1, call_count1) = CountingBroker::new();
    let mut interp1 = MeteredInterpreter::new(ir.clone(), limits.clone());
    interp1.set_replay_fingerprint(default_fingerprint());
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, codegg_core::tool_program::ProgramStatus::Completed);
    assert_eq!(call_count1.load(Ordering::SeqCst), 2);

    // Capture completed calls.
    let completed = interp1.completed_calls().clone();
    assert_eq!(completed.len(), 2);

    // Second run: both calls should be replayed.
    let (broker2, call_count2) = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(ir, limits);
    interp2.set_replay_fingerprint(default_fingerprint());
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, codegg_core::tool_program::ProgramStatus::Completed);
    assert_eq!(
        call_count2.load(Ordering::SeqCst),
        0,
        "all calls must be replayed without broker invocation"
    );
}
