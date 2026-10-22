//! Regression: `python_script` tool calls must not stall for their wait bound.
//!
//! Two independent defects made a `python_script` call hang until its
//! `timeout + 30s` bound elapsed and then report a bogus
//! `timed out waiting for job ...`:
//!
//! 1. **Missing executor registration (the trigger).** `PythonJobExecutor` was
//!    only registered by an uncalled free function, so `ExecutorKind::Python`
//!    resolved to nothing. `try_dispatch_next` marked the job unschedulable, but
//!    `mark_unschedulable` neither published a terminal completion nor woke
//!    anyone — so the submitter parked until its bound with the answer already
//!    known. This is why the stall was deterministic rather than flaky.
//! 2. **Lost-wakeup on the shared `Notify` (the amplifier).** All completion
//!    waiters and the run loop parked on one `Notify` with a single stored
//!    permit, so a publish could wake an arbitrary waiter rather than the one
//!    whose job finished. Fixed by per-`JobId` broadcast signals; see
//!    `completion_signal_broadcast.rs`.
//!
//! This test drives the real production path: real scheduler, real run loop,
//! real `JobSubmissionService`, real `PythonScriptTool`, concurrent calls.

use std::sync::Arc;
use std::time::{Duration, Instant};

use codegg::python_script::tool::PythonScriptTool;
use codegg::scheduler::submission::JobSubmissionService;
use codegg::scheduler::{JobScheduler, ResolvedSchedulerConfig};
use codegg::tool::Tool;
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

/// Concurrent python_script calls, each with a generous model-supplied timeout.
const CONCURRENCY: usize = 3;
const MODEL_TIMEOUT_SECS: u64 = 20;
/// The tool's wait bound is `timeout + 30s` = 50s here, so any ceiling well
/// under that discriminates a stall from a slow call. A completed call normally
/// returns in ~1s; the extra headroom absorbs load from other test binaries
/// launching real subprocesses concurrently, which does not weaken the guard
/// because a genuinely stalled call still has to sit out the full 50s.
const EXPECTED_CEILING: Duration = Duration::from_secs(20);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_python_script_calls_do_not_stall_for_their_wait_bound() {
    let root = tempfile::tempdir().expect("temp workspace");

    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );

    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = Arc::new(JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("gen-wakeup-repro"),
    ));
    scheduler
        .register_default_executors_sync(None)
        .expect("register default executors");

    // Production starts the run loop; the existing python_scheduler_execution
    // harness never does, which is why this stall was never covered.
    let _run_handle = scheduler.spawn_run();

    let submission = JobSubmissionService::new(
        store,
        Arc::clone(&scheduler),
        services,
        DaemonGeneration::new_unchecked("gen-wakeup-repro"),
    );

    let workdir = root.path().to_string_lossy().to_string();
    let started = Instant::now();

    // Concurrent agent-style tool calls. Distinct sources so each one becomes
    // its own durable job (identical source would collapse onto one job via
    // the source-hash submission key).
    let mut calls = Vec::new();
    for index in 0..CONCURRENCY {
        let submission = Arc::clone(&submission);
        let workdir = workdir.clone();
        calls.push(tokio::spawn(async move {
            let tool = PythonScriptTool::with_scheduler(None, Some(submission));
            let call_started = Instant::now();
            let result = tool
                .execute(serde_json::json!({
                    "code": format!("print('repro-{index}')"),
                    "mode": "analyze",
                    "workdir": workdir,
                    "timeout": MODEL_TIMEOUT_SECS,
                }))
                .await;
            (index, call_started.elapsed(), result)
        }));
    }

    let mut elapsed = Vec::new();
    for call in calls {
        let (index, took, result) = call.await.expect("tool call task must not panic");
        // The sandbox may legitimately refuse to launch on this host; the point
        // is that the call RETURNS. Accept either outcome, reject a stall.
        match result {
            Ok(_) => {}
            Err(error) => eprintln!("call {index} returned an error (acceptable): {error}"),
        }
        eprintln!("call {index} returned after {took:?}");
        elapsed.push(took);
    }

    let slowest = elapsed.iter().copied().max().unwrap();
    eprintln!(
        "all {CONCURRENCY} calls returned in {:?} (slowest: {slowest:?})",
        started.elapsed()
    );

    assert!(
        slowest < EXPECTED_CEILING,
        "python_script stalled: slowest concurrent call took {slowest:?}, \
         which means it slept out its wait bound \
         (timeout {MODEL_TIMEOUT_SECS}s + 30s) instead of being woken on completion"
    );
}
