//! Regression: `wait_for_completion` must deliver on a per-`JobId` broadcast.
//!
//! `JobScheduler::wait_for_completion` used to park on the scheduler's single
//! shared `Notify` — the same one the run loop's `select!` and every other
//! concurrent waiter were queued on. `Notify::notify_one` releases a single
//! stored permit to one *arbitrary* waiter, so once more than one waiter was
//! parked the waiter whose job had actually finished could be the one left
//! asleep for its entire bound.
//!
//! Completion now goes through `completion_signals`: a bounded per-`JobId`
//! `watch` generation counter. Publishing a terminal outcome advances only that
//! job's counter, so it wakes every waiter for that job and leaves unrelated
//! waiters parked.
//!
//! # Why the latency thresholds are tight
//!
//! `wait_for_completion` also parks on a `COMPLETION_RECHECK_INTERVAL`
//! (250 ms) durable-state backstop, so a loose "did it return before the
//! 30 s bound?" assertion passes even with the shared `Notify` restored — the
//! tick quietly rescues every waiter. These tests therefore assert on a wake
//! **well below the backstop interval**, which is the only way to distinguish
//! "woken by its own job's signal" from "woken by the tick".
//!
//! These tests drive the public API only and deliberately never start the run
//! loop, so a submitted job stays `Queued` and `request_cancel` terminalizes it
//! without any executor running. That isolates the signal path from dispatch.

use std::sync::Arc;
use std::time::{Duration, Instant};

use codegg::scheduler::{ExecutorStatus, JobScheduler, ResolvedSchedulerConfig};
use codegg_core::jobs::{
    DaemonGeneration, ExecutionTarget, InMemoryJobStore, JobKind, JobPayload, JobPriority,
    JobSource, JobState, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

/// A real `python_script`/bash wait bound is `timeout + 30s`. A correctly woken
/// waiter returns in milliseconds; the bound only bounds the failure mode.
const WAIT_BOUND: Duration = Duration::from_secs(30);

/// Prompt-wake ceiling. Must sit far below the 250 ms backstop tick (which is
/// how a lost wakeup is masked) and far above the sub-millisecond signal path.
/// See the module docs.
const PROMPT_WAKE: Duration = Duration::from_millis(100);

/// How long a bystander waiter is given to wrongly finish after an unrelated
/// job's terminal transition.
const BYSTANDER_GRACE: Duration = Duration::from_millis(500);

async fn scheduler_with_workspace() -> (Arc<JobScheduler>, WorkspaceId) {
    let root = tempfile::tempdir().expect("temp workspace");
    let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let workspace_id = registry
        .get_or_register(root.path())
        .await
        .expect("register workspace")
        .id
        .clone();
    let services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );

    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store,
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("gen-completion-signal"),
    );
    (scheduler, workspace_id)
}

fn queued_test_job(workspace_id: WorkspaceId) -> NewJob {
    NewJob {
        workspace_id,
        session_id: Some("sess-1".to_string()),
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::AgentDelegated,
        priority: JobPriority::Normal,
        payload: JobPayload::Test {
            command: "echo ok".to_owned(),
            argv: vec!["echo".to_owned(), "ok".to_owned()],
            cwd: None,
            scope: None,
            parent_run_id: None,
        },
        resource_request: ResourceRequest::for_kind(JobKind::Test),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: ExecutionTarget::default(),
    }
}

/// Every waiter on the SAME job must be woken by that job's terminal outcome.
///
/// This is the case the shared `Notify` cannot serve: `notify_one` releases a
/// single permit, so N waiters parked on one job need N wakeups while a publish
/// issues a fixed number. Under the shared `Notify` the extra waiters can only
/// escape via the 250 ms backstop tick, which this test's `PROMPT_WAKE`
/// threshold rejects.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_waiters_on_one_job_are_woken_promptly_by_its_terminal_transition() {
    const WAITERS: usize = 4;

    let (scheduler, workspace_id) = scheduler_with_workspace().await;
    let job = scheduler
        .submit(queued_test_job(workspace_id))
        .await
        .expect("submit queued job");
    assert!(
        matches!(job.state, JobState::Queued),
        "no run loop is started, so the job must stay Queued for this test to isolate the signal"
    );

    let mut waiters = Vec::new();
    for _ in 0..WAITERS {
        let scheduler = Arc::clone(&scheduler);
        let job_id = job.job_id.clone();
        waiters.push(tokio::spawn(async move {
            let result = scheduler.wait_for_completion(&job_id, WAIT_BOUND).await;
            (Instant::now(), result)
        }));
    }

    // Let every waiter park on its per-job signal, so the terminal transition
    // below lands while they are all genuinely asleep.
    //
    // This settle window is deliberately far SHORTER than the 250 ms backstop
    // tick: each waiter's tick fires 250 ms after it parks, so a long settle
    // would put the tick almost exactly on top of the terminal transition and
    // the tick would rescue every lost wakeup, proving nothing. At 50 ms the
    // signal path wakes in ~0.2 ms while a lost wakeup must wait ~200 ms.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Latency is measured from the terminal transition, not from waiter start,
    // so the settling sleep above is not counted as wake-up delay.
    let cancelled_at = Instant::now();
    scheduler
        .request_cancel(&job.job_id, "test broadcast")
        .await
        .expect("request cancel");

    for (index, waiter) in waiters.into_iter().enumerate() {
        let (finished_at, result) = waiter.await.expect("waiter task must not panic");
        let woke_after = finished_at.saturating_duration_since(cancelled_at);
        let completion = result.expect("waiter must observe the terminal state");
        assert_eq!(
            completion.status,
            ExecutorStatus::Cancelled,
            "waiter {index} observed the wrong terminal status"
        );
        assert!(
            woke_after < PROMPT_WAKE,
            "waiter {index} took {woke_after:?} to wake after its own job's terminal \
             transition, which is the backstop tick (~250ms), not the per-job signal. A \
             shared-Notify build reaches this state whenever the single permit is \
             consumed elsewhere."
        );
        eprintln!("waiter {index} woke {woke_after:?} after terminal transition");
    }
    let cancel_to_wake = cancelled_at.elapsed();
    eprintln!("all {WAITERS} same-job waiters woken {cancel_to_wake:?} after cancel");
}

/// A terminal transition for job A must wake A's waiters *promptly* without
/// waiting on an unrelated job's waiter having been served first.
///
/// Under the shared `Notify`, one publish issues one `notify_one` against an
/// arbitrary waiter: if that permit went to B, A waits for the backstop tick.
/// This asserts the prompt path, which is what a per-job broadcast guarantees.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_transition_for_one_job_is_not_delayed_by_an_unrelated_waiter() {
    let (scheduler, workspace_id) = scheduler_with_workspace().await;
    let target = scheduler
        .submit(queued_test_job(workspace_id.clone()))
        .await
        .expect("submit target job");
    let bystander = scheduler
        .submit(queued_test_job(workspace_id))
        .await
        .expect("submit bystander job");
    assert_ne!(
        target.job_id, bystander.job_id,
        "the two jobs must be distinct for this test to mean anything"
    );

    let target_wait = {
        let scheduler = Arc::clone(&scheduler);
        let job_id = target.job_id.clone();
        tokio::spawn(async move {
            let result = scheduler.wait_for_completion(&job_id, WAIT_BOUND).await;
            (Instant::now(), result)
        })
    };
    let bystander_wait = {
        let scheduler = Arc::clone(&scheduler);
        let job_id = bystander.job_id.clone();
        tokio::spawn(async move { scheduler.wait_for_completion(&job_id, WAIT_BOUND).await })
    };

    // Keep this settle window short relative to the 250 ms backstop tick, so a
    // lost wakeup cannot be masked by a tick landing on the transition.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cancelled_at = Instant::now();
    scheduler
        .request_cancel(&target.job_id, "test isolation")
        .await
        .expect("cancel target job");

    let (finished_at, result) = target_wait.await.expect("target waiter must not panic");
    let woke_after = finished_at.saturating_duration_since(cancelled_at);
    let completion = result.expect("target waiter must observe its terminal state");
    assert_eq!(completion.status, ExecutorStatus::Cancelled);
    assert!(
        woke_after < PROMPT_WAKE,
        "the target job's own waiter took {woke_after:?} to wake while an unrelated waiter \
         was parked, so the wakeup was routed through the shared signal instead of the \
         per-job broadcast"
    );
    eprintln!("target waiter woke {woke_after:?} after its own cancel");

    // The bystander has no terminal outcome of its own, so its waiter must
    // still be waiting. (A spurious wake is benign — it re-reads state and
    // re-parks — so this asserts isolation of the *result*, not of wakeups.)
    tokio::time::sleep(BYSTANDER_GRACE).await;
    assert!(
        !bystander_wait.is_finished(),
        "a waiter on the bystander job completed after an unrelated job's terminal \
         transition, so completion state leaked across jobs"
    );

    // Releasing the bystander proves it was genuinely waiting, not just slow.
    scheduler
        .request_cancel(&bystander.job_id, "test isolation release")
        .await
        .expect("cancel bystander job");
    let result = bystander_wait
        .await
        .expect("bystander waiter must not panic");
    assert_eq!(
        result
            .expect("bystander must observe its own terminal state")
            .status,
        ExecutorStatus::Cancelled
    );
}

/// A waiter that arrives *after* the terminal transition must still return
/// immediately.
///
/// Guards the ordering discipline in `wait_for_completion`: the per-job
/// receiver is created before the first completion-cache read precisely so a
/// publish landing in the gap between that read and the park is still
/// delivered. `mark_unschedulable` legitimately completes a job before its
/// submitter ever waits, so the late-waiter path is a real production order.
#[tokio::test(flavor = "current_thread")]
async fn waiter_arriving_after_the_transition_returns_immediately() {
    let (scheduler, workspace_id) = scheduler_with_workspace().await;
    let job = scheduler
        .submit(queued_test_job(workspace_id))
        .await
        .expect("submit queued job");

    scheduler
        .request_cancel(&job.job_id, "pre-wait cancel")
        .await
        .expect("cancel before any waiter subscribes");

    let started = Instant::now();
    let completion = scheduler
        .wait_for_completion(&job.job_id, WAIT_BOUND)
        .await
        .expect("late waiter must observe the terminal state");
    let took = started.elapsed();

    assert_eq!(completion.status, ExecutorStatus::Cancelled);
    assert!(
        took < PROMPT_WAKE,
        "a waiter subscribing after the transition still took {took:?}, so it missed \
         both the per-job signal and the durable-state re-read"
    );
    eprintln!("late waiter returned in {took:?}");
}
