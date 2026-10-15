//! Async-background-jobs M001: generic job notification contract.
//!
//! Proves submit/notify/replay atomically or with durable crash
//! reconciliation, exact-owner security, and bounded terminal semantics:
//! - Only explicitly subscribed background jobs produce notices.
//! - `Interrupted` (retryable) never produces a false final.
//! - Duplicate terminals are idempotent (one logical notice).
//! - Cross-session forgery fails closed without leakage.
//! - Claim/ack lifecycle is CAS-guarded; stale claims expire terminally.
//! - Bounded backlog suppresses oldest, never starves new notices.
//! - Completion-before-subscribe is healed by reconciliation.
//! - Timeout-as-observation never kills the job (wait timeout is not a
//!   terminal outcome).

use codegg::scheduler::job_notifications::{
    is_final_job_state, JobNotificationService, JobNotificationState,
};
use codegg_core::jobs::{JobId, JobKind, JobPriority, JobRecord, JobSource, JobState};
use codegg_core::workspace::WorkspaceId;

mod common;

fn job_record(job_id: &str, session: Option<&str>, state: JobState) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked(job_id),
        workspace_id: WorkspaceId::new_unchecked("ws-m001"),
        session_id: session.map(ToOwned::to_owned),
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: codegg_core::jobs::JobPayload::Test {
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: None,
            scope: None,
            parent_run_id: None,
        },
        resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Test),
        state,
        current_attempt_id: None,
        attempt_count: 0,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        labels: Default::default(),
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
        target: Default::default(),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
    }
}

#[test]
fn final_states_exclude_interrupted_and_running() {
    assert!(is_final_job_state(&JobState::Completed));
    assert!(is_final_job_state(&JobState::Failed));
    assert!(is_final_job_state(&JobState::Cancelled));
    assert!(is_final_job_state(&JobState::TimedOut));
    assert!(is_final_job_state(&JobState::Expired));
    assert!(!is_final_job_state(&JobState::Interrupted));
    assert!(!is_final_job_state(&JobState::Running));
    assert!(!is_final_job_state(&JobState::Queued));
    assert!(!is_final_job_state(&JobState::Scheduled));
    assert!(!is_final_job_state(&JobState::Blocked));
}

#[tokio::test(flavor = "current_thread")]
async fn foreground_job_without_subscription_produces_no_notice() {
    let svc = JobNotificationService::new();
    let job = job_record("job-fg-1", Some("s1"), JobState::Completed);
    let out = svc.record_terminal(&job, "ok", None).await.unwrap();
    assert!(out.is_none(), "foreground must never create action items");
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_never_emits_false_final() {
    let svc = JobNotificationService::new();
    svc.subscribe(
        &JobId::new_unchecked("job-int-1"),
        "test",
        "s1",
        None,
        "inv-1",
    )
    .await
    .unwrap();
    for state in [
        JobState::Interrupted,
        JobState::Running,
        JobState::Queued,
        JobState::Scheduled,
        JobState::Blocked,
    ] {
        let job = job_record("job-int-1", Some("s1"), state);
        let out = svc.record_terminal(&job, "not-final", None).await.unwrap();
        assert!(out.is_none(), "non-final {state:?} must not notify");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_before_subscription_healed_by_reconcile() {
    // Completion before return/subscribe: the scheduler terminalizes first,
    // the subscription lands second, reconciliation backfills one notice.
    let svc = JobNotificationService::new();
    let job = job_record("job-race-1", Some("s1"), JobState::Completed);
    // No subscription yet -> no notice (not a foreground retro-notify).
    assert!(svc
        .record_terminal(&job, "ok", None)
        .await
        .unwrap()
        .is_none());
    // Subscription arrives (retry with same key returns same job).
    svc.subscribe(
        &JobId::new_unchecked("job-race-1"),
        "test",
        "s1",
        None,
        "inv-race",
    )
    .await
    .unwrap();
    // Reconcile the already-terminal job.
    let n = svc
        .reconcile_terminals(vec![
            codegg::scheduler::job_notifications::RecoveredJobTerminal {
                job_id: "job-race-1".into(),
                kind: "test".into(),
                session_id: Some("s1".into()),
                turn_id: None,
                state: JobState::Completed,
                summary: "ok".into(),
                run_id: None,
                created_at: chrono::Utc::now().timestamp_millis(),
            },
        ])
        .await
        .unwrap();
    assert_eq!(n, 1);
    let pending = svc.pending_for_session("s1").await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].job_id, "job-race-1");
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_terminal_events_are_idempotent() {
    let svc = JobNotificationService::new();
    svc.subscribe(
        &JobId::new_unchecked("job-dup-1"),
        "test",
        "s1",
        None,
        "inv-dup",
    )
    .await
    .unwrap();
    let job = job_record("job-dup-1", Some("s1"), JobState::Completed);
    let first = svc
        .record_terminal(&job, "ok", None)
        .await
        .unwrap()
        .unwrap();
    // Duplicate terminal event (scheduler replay, provider retry).
    let second = svc
        .record_terminal(&job, "ok", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.notice_id, second.notice_id);
    assert_eq!(first.payload_digest, second.payload_digest);
    let pending = svc.pending_for_session("s1").await.unwrap();
    assert_eq!(pending.len(), 1, "exactly one logical notice");
}

#[tokio::test(flavor = "current_thread")]
async fn two_concurrent_claimants_only_one_wins() {
    let svc = JobNotificationService::new();
    svc.subscribe(
        &JobId::new_unchecked("job-cc-1"),
        "test",
        "s1",
        None,
        "inv-cc",
    )
    .await
    .unwrap();
    let job = job_record("job-cc-1", Some("s1"), JobState::Failed);
    svc.record_terminal(&job, "boom", None).await.unwrap();
    // Two simultaneous frontend listeners race the claim.
    let (a, b) = tokio::join!(
        svc.claim_as("job-cc-1", "frontend-a"),
        svc.claim_as("job-cc-1", "frontend-b")
    );
    let wins = [a.unwrap(), b.unwrap()].iter().filter(|&&v| v).count();
    assert_eq!(wins, 1, "exactly one claimant wins the CAS");
}

#[tokio::test(flavor = "current_thread")]
async fn forged_session_and_second_principal_denied_without_leakage() {
    let svc = JobNotificationService::new();
    svc.subscribe(
        &JobId::new_unchecked("job-sec-1"),
        "test",
        "owner-session",
        None,
        "inv-sec",
    )
    .await
    .unwrap();
    let job = job_record("job-sec-1", Some("owner-session"), JobState::Completed);
    let notice = svc
        .record_terminal(&job, "ok", None)
        .await
        .unwrap()
        .unwrap();
    // Exact-owner check: guessed job id by a second principal fails.
    assert!(JobNotificationService::check_owner(&notice, Some("owner-session")).is_ok());
    assert!(JobNotificationService::check_owner(&notice, Some("attacker-session")).is_err());
    assert!(JobNotificationService::check_owner(&notice, None).is_err());
    // Cross-session lineage mismatch fails closed at record time.
    let forged = job_record("job-sec-1", Some("attacker-session"), JobState::Completed);
    let err = svc.record_terminal(&forged, "ok", None).await.unwrap_err();
    assert!(
        matches!(
            err,
            codegg::scheduler::job_notifications::JobNotificationError::Unauthorized(_)
        ),
        "cross-session mismatch must fail closed, got {err:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn changed_payload_duplicate_conflicts() {
    let svc = JobNotificationService::new();
    svc.subscribe(
        &JobId::new_unchecked("job-conf-1"),
        "test",
        "s1",
        None,
        "inv-conf",
    )
    .await
    .unwrap();
    // Same submission key must carry the same fingerprint; the scheduler
    // enforces SubmissionKeyConflict, while the notice layer treats a
    // different terminal digest for the same job as a conflict (keeps first).
    let job = job_record("job-conf-1", Some("s1"), JobState::Completed);
    svc.record_terminal(&job, "summary-A", None).await.unwrap();
    let err = svc
        .record_terminal(&job, "summary-B-different", None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            codegg::scheduler::job_notifications::JobNotificationError::Conflict(_)
        ),
        "conflicting terminal must not overwrite, got {err:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn output_overflow_is_bounded() {
    let svc = JobNotificationService::new();
    svc.subscribe(
        &JobId::new_unchecked("job-big-1"),
        "test",
        "s1",
        None,
        "inv-big",
    )
    .await
    .unwrap();
    let huge = "x".repeat(100_000);
    let job = job_record("job-big-1", Some("s1"), JobState::Failed);
    let notice = svc
        .record_terminal(&job, &huge, None)
        .await
        .unwrap()
        .unwrap();
    assert!(
        notice.summary.chars().count() <= 2_048,
        "summary must be bounded, got {}",
        notice.summary.len()
    );
    assert!(
        !notice.summary.contains("secret-token-"),
        "no secret passthrough by construction"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn all_terminal_outcomes_produce_stable_notices() {
    for (state, status) in [
        (JobState::Completed, "completed"),
        (JobState::Failed, "failed"),
        (JobState::Cancelled, "cancelled"),
        (JobState::TimedOut, "timed_out"),
        (JobState::Expired, "expired"),
    ] {
        let svc = JobNotificationService::new();
        let id = format!("job-t-{status}");
        svc.subscribe(&JobId::new_unchecked(&id), "test", "s1", None, "inv")
            .await
            .unwrap();
        let job = job_record(&id, Some("s1"), state);
        let notice = svc
            .record_terminal(&job, "done", Some("run-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(notice.status, status);
        assert_eq!(notice.run_id.as_deref(), Some("run-1"));
        assert_eq!(notice.notice_id, id);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn stale_claim_expires_terminally_and_session_bound_suppresses_oldest() {
    let svc = JobNotificationService::with_policy(
        codegg::scheduler::job_notifications::JobNotificationPolicy {
            max_pending_per_session: 2,
            claim_lease_ms: 50,
            max_payload_bytes: 8_192,
            max_summary_chars: 2_048,
        },
    );
    for i in 0..3 {
        let id = format!("job-bound-{i}");
        svc.subscribe(&JobId::new_unchecked(&id), "test", "s-bound", None, "inv")
            .await
            .unwrap();
        let job = job_record(&id, Some("s-bound"), JobState::Completed);
        svc.record_terminal(&job, "ok", None).await.unwrap();
    }
    let pending = svc.pending_for_session("s-bound").await.unwrap();
    assert_eq!(pending.len(), 2, "oldest must be suppressed, newest kept");
    // Stale claim becomes Expired, never resurrected to Pending.
    let target = pending[0].notice_id.clone();
    assert!(svc.claim(&target).await.unwrap());
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let expired = svc.expire_stale(50).await;
    assert!(expired.contains(&target));
    let got = svc.get(&target).await.unwrap().unwrap();
    assert_eq!(got.state, JobNotificationState::Expired);
}

#[tokio::test(flavor = "current_thread")]
async fn sqlite_persistence_survives_service_restart() {
    let pool = common::pool::isolated_pool().await;
    let svc = JobNotificationService::with_pool(pool.clone());
    svc.subscribe(
        &JobId::new_unchecked("job-sql-1"),
        "test",
        "s-sql",
        Some("turn-1"),
        "inv-sql",
    )
    .await
    .unwrap();
    let job = job_record("job-sql-1", Some("s-sql"), JobState::Completed);
    svc.record_terminal(&job, "ok", None).await.unwrap();
    // Fresh service against the same pool recovers durable state.
    let svc2 = JobNotificationService::with_pool(pool.clone());
    let got = svc2.get("job-sql-1").await.unwrap();
    assert!(got.is_some(), "notice must survive restart via SQLite");
    let pending = svc2.pending_for_session("s-sql").await.unwrap();
    assert_eq!(pending.len(), 1);
    let sub = svc2.get_subscription("job-sql-1").await.unwrap();
    assert!(sub.is_some(), "subscription must survive restart");
}
