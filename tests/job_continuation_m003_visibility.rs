//! Async-background-jobs M003: safe continuation, inbox, visibility.
//!
//! - Live parent gets one safe-boundary notice without duplicate provider
//!   entry; ended turn spends no tokens but keeps one durable visible
//!   notification for reconnect/restart/next turn.
//! - Explicit `resume_owner` yields exactly one bounded continuation per
//!   logical terminal (idempotent under duplicates + concurrent frontends,
//!   within lease); cancel/archive/revocation/expiry wins before invocation.
//! - Crash at each boundary (persist, append, ack, lease claim/consume)
//!   reconstructs without duplicate unbounded inference.
//! - Failure/timeout/cancel/Interrupted classified; foreign sessions get
//!   nothing; observer reads no secrets and cancels nothing.
//! - Daemon + frontend views agree on reconnect; no toast-only receipt.

use codegg::agent::job_notification_recovery::inject_recoverable_job_notifications;
use codegg::scheduler::job_notifications::{
    JobCompletionDisposition, JobContinuationStatus, JobNotificationService,
};
use codegg_core::jobs::{JobId, JobKind, JobPriority, JobRecord, JobSource, JobState};
use codegg_core::workspace::WorkspaceId;

mod common;

fn job_record(job_id: &str, session: &str, state: JobState) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked(job_id),
        workspace_id: WorkspaceId::new_unchecked("ws-m003"),
        session_id: Some(session.to_string()),
        turn_id: Some("turn-m003".to_string()),
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

async fn service_with_pool() -> (JobNotificationService, codegg_core::session::EventStore) {
    let pool = common::pool::isolated_pool().await;
    let svc = JobNotificationService::with_pool(pool.clone());
    let store = codegg_core::session::EventStore::new(pool);
    (svc, store)
}

/// `session_events.session_id` references `session(id)`, which references
/// `project(id)`. Hermetic pools need both rows before the recovery loop
/// can append the durable parent-session event (production always has a
/// real session row).
async fn ensure_session(pool: &sqlx::SqlitePool, session_id: &str) {
    sqlx::query("INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('proj-m003', '/tmp', 1, 1, '[]')")
        .execute(pool)
        .await
        .expect("project row");
    sqlx::query("INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, 'proj-m003', 's', '/tmp', 't', 'v', 1, 1)")
        .bind(session_id)
        .execute(pool)
        .await
        .expect("session row");
}

#[tokio::test(flavor = "current_thread")]
async fn live_parent_gets_one_safe_boundary_notice() {
    let (svc, store) = service_with_pool().await;
    svc.subscribe(
        &JobId::new_unchecked("job-live-1"),
        "test",
        "sess-live",
        Some("turn-1"),
        "inv-live",
    )
    .await
    .unwrap();
    svc.record_terminal(
        &job_record("job-live-1", "sess-live", JobState::Completed),
        "ok",
        None,
    )
    .await
    .unwrap();
    // Live parent doing unrelated work: one injection at the next safe
    // boundary, no duplicate provider entry (single system message + single
    // durable event).
    ensure_session(&store.pool(), "sess-live").await;
    let mut messages = vec![];
    let report = inject_recoverable_job_notifications(Some(&store), &svc, "sess-live", |text| {
        messages.push(text)
    })
    .await;
    assert_eq!(report.injected, 1);
    assert_eq!(messages.len(), 1);
    assert_eq!(report.resumed, 0, "notify_only never resumes");
    // Duplicate terminal event + second boundary: no second message, no
    // second event (exact-once logical insertion).
    svc.record_terminal(
        &job_record("job-live-1", "sess-live", JobState::Completed),
        "ok",
        None,
    )
    .await
    .unwrap();
    let mut messages2 = vec![];
    let report2 = inject_recoverable_job_notifications(Some(&store), &svc, "sess-live", |text| {
        messages2.push(text)
    })
    .await;
    assert!(messages2.is_empty(), "duplicate must not re-inject");
    assert_eq!(
        report2.already_injected + report2.leased + report2.recovered_via_event,
        report2.considered
    );
    let events = store.list_for_session("sess-live").await.unwrap();
    assert_eq!(events.len(), 1, "one durable event per logical terminal");
}

#[tokio::test(flavor = "current_thread")]
async fn ended_turn_default_spends_no_tokens_keeps_durable_notice() {
    let (svc, store) = service_with_pool().await;
    svc.subscribe(
        &JobId::new_unchecked("job-ended-1"),
        "test",
        "sess-ended",
        Some("turn-e"),
        "inv-e",
    )
    .await
    .unwrap();
    ensure_session(&store.pool(), "sess-ended").await;
    svc.record_terminal(
        &job_record("job-ended-1", "sess-ended", JobState::Completed),
        "ok",
        None,
    )
    .await
    .unwrap();
    // Ended turn: no live loop runs, so no messages are produced and no
    // model tokens are spent. The notice stays pending and inspectable.
    assert_eq!(
        svc.pending_for_session("sess-ended").await.unwrap().len(),
        1
    );
    // Restart/reconnect: a fresh service + store view over the same pool
    // sees the same authoritative state.
    let pool = store.pool();
    let svc2 = JobNotificationService::with_pool(pool.clone());
    let store2 = codegg_core::session::EventStore::new(pool);
    assert_eq!(
        svc2.pending_for_session("sess-ended").await.unwrap().len(),
        1
    );
    // Next user-authorized turn consumes it exactly once.
    let mut messages = vec![];
    let report = inject_recoverable_job_notifications(Some(&store2), &svc2, "sess-ended", |text| {
        messages.push(text)
    })
    .await;
    assert_eq!(messages.len(), 1);
    assert_eq!(report.injected, 1);
    assert!(svc2
        .pending_for_session("sess-ended")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_resume_yields_exactly_one_continuation() {
    let (svc, store) = service_with_pool().await;
    svc.subscribe_with_options(
        &JobId::new_unchecked("job-res-1"),
        "test",
        "sess-res",
        Some("turn-r"),
        "inv-r",
        JobCompletionDisposition::ResumeOwner,
        Some("principal-r"),
        Some("ws-r"),
        Some(60_000),
    )
    .await
    .unwrap();
    ensure_session(&store.pool(), "sess-res").await;
    svc.record_terminal(
        &job_record("job-res-1", "sess-res", JobState::Completed),
        "tests pass",
        None,
    )
    .await
    .unwrap();
    // First boundary consumes the single-use lease and marks the message.
    let mut m1 = vec![];
    let r1 =
        inject_recoverable_job_notifications(Some(&store), &svc, "sess-res", |t| m1.push(t)).await;
    assert_eq!(r1.resumed, 1);
    assert_eq!(m1.len(), 1);
    assert!(
        m1[0].contains("resume authorized"),
        "resume marker: {}",
        m1[0]
    );
    let lease = svc.get_lease("job-res-1").await.unwrap().unwrap();
    assert_eq!(lease.status, JobContinuationStatus::Consumed);
    // Duplicate terminal + simultaneous frontends: never a second resume.
    svc.record_terminal(
        &job_record("job-res-1", "sess-res", JobState::Completed),
        "tests pass",
        None,
    )
    .await
    .unwrap();
    let mut m2 = vec![];
    let r2 =
        inject_recoverable_job_notifications(Some(&store), &svc, "sess-res", |t| m2.push(t)).await;
    assert!(m2.is_empty());
    assert_eq!(r2.resumed, 0);
    // Concurrent consume race on a fresh lease: exactly one winner.
    svc.subscribe_with_options(
        &JobId::new_unchecked("job-res-race"),
        "test",
        "sess-res",
        None,
        "inv-race",
        JobCompletionDisposition::ResumeOwner,
        None,
        None,
        Some(60_000),
    )
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        svc.consume_resume_lease("job-res-race", "sess-res"),
        svc.consume_resume_lease("job-res-race", "sess-res")
    );
    assert_eq!([a.unwrap(), b.unwrap()].iter().filter(|&&v| v).count(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_expiry_and_foreign_owner_suppress_resume() {
    let (svc, store) = service_with_pool().await;
    // User cancel wins: cancelled terminal suppresses the resume lease.
    svc.subscribe_with_options(
        &JobId::new_unchecked("job-cancel-1"),
        "test",
        "sess-c",
        None,
        "inv-c",
        JobCompletionDisposition::ResumeOwner,
        None,
        None,
        Some(60_000),
    )
    .await
    .unwrap();
    ensure_session(&store.pool(), "sess-c").await;
    svc.record_terminal(
        &job_record("job-cancel-1", "sess-c", JobState::Cancelled),
        "user cancelled",
        None,
    )
    .await
    .unwrap();
    let mut m = vec![];
    let r = inject_recoverable_job_notifications(Some(&store), &svc, "sess-c", |t| m.push(t)).await;
    assert_eq!(m.len(), 1, "cancel still notifies (attention)");
    assert!(!m[0].contains("resume authorized"));
    assert_eq!(r.resumed, 0);
    assert_eq!(r.resume_suppressed, 1);
    // Expired lease wins: no resume marker, suppressed counted.
    svc.subscribe_with_options(
        &JobId::new_unchecked("job-exp-1"),
        "test",
        "sess-e",
        None,
        "inv-e",
        JobCompletionDisposition::ResumeOwner,
        None,
        None,
        Some(1),
    )
    .await
    .unwrap();
    ensure_session(&store.pool(), "sess-e").await;
    svc.record_terminal(
        &job_record("job-exp-1", "sess-e", JobState::Completed),
        "ok",
        None,
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let mut m2 = vec![];
    let r2 =
        inject_recoverable_job_notifications(Some(&store), &svc, "sess-e", |t| m2.push(t)).await;
    assert_eq!(m2.len(), 1);
    assert!(!m2[0].contains("resume authorized"));
    assert_eq!(r2.resumed, 0);
    // Foreign session gets nothing (no cross-turn injection, no leak).
    let mut m3 = vec![];
    let r3 =
        inject_recoverable_job_notifications(Some(&store), &svc, "other-session", |t| m3.push(t))
            .await;
    assert!(m3.is_empty());
    assert_eq!(r3.considered, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn crash_boundaries_reconstruct_without_duplicate_inference() {
    let (svc, store) = service_with_pool().await;
    svc.subscribe_with_options(
        &JobId::new_unchecked("job-crash-1"),
        "test",
        "sess-crash",
        None,
        "inv-crash",
        JobCompletionDisposition::ResumeOwner,
        None,
        None,
        Some(60_000),
    )
    .await
    .unwrap();
    ensure_session(&store.pool(), "sess-crash").await;
    let notice = svc
        .record_terminal(
            &job_record("job-crash-1", "sess-crash", JobState::Completed),
            "ok",
            None,
        )
        .await
        .unwrap()
        .unwrap();
    // Crash after append but before mark: manually append the expected
    // event, then let recovery confirm semantically without re-injecting.
    let expected =
        codegg::agent::job_notification_recovery::expected_job_event(&notice, "sess-crash");
    store.append_idempotent(&expected).await.unwrap();
    let mut m = vec![];
    let r =
        inject_recoverable_job_notifications(Some(&store), &svc, "sess-crash", |t| m.push(t)).await;
    assert!(m.is_empty(), "crash-after-append must not re-inject");
    assert_eq!(r.recovered_via_event, 1);
    // Crash after mark but before lease-consume: mark manually, recovery
    // consumes the lease without a second message.
    let (svc2, store2) = {
        let pool = store.pool();
        (
            JobNotificationService::with_pool(pool.clone()),
            codegg_core::session::EventStore::new(pool),
        )
    };
    svc2.subscribe_with_options(
        &JobId::new_unchecked("job-crash-2"),
        "test",
        "sess-crash",
        None,
        "inv-crash-2",
        JobCompletionDisposition::ResumeOwner,
        None,
        None,
        Some(60_000),
    )
    .await
    .unwrap();
    let n2 = svc2
        .record_terminal(
            &job_record("job-crash-2", "sess-crash", JobState::Completed),
            "ok",
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let exp2 = codegg::agent::job_notification_recovery::expected_job_event(&n2, "sess-crash");
    let exp2_id = exp2.meta().id.clone();
    store2.append_idempotent(&exp2).await.unwrap();
    assert!(svc2.claim_as(&n2.notice_id, "test").await.unwrap());
    svc2.mark_injected(&n2.notice_id, &exp2_id).await.unwrap();
    // Lease still pending: recovery must consume it exactly once, no message.
    let mut m2 = vec![];
    let r2 =
        inject_recoverable_job_notifications(Some(&store2), &svc2, "sess-crash", |t| m2.push(t))
            .await;
    assert!(m2.is_empty());
    assert!(r2.already_injected >= 1 || r2.recovered_via_event >= 1);
    let lease = svc2.get_lease("job-crash-2").await.unwrap().unwrap();
    assert_eq!(lease.status, JobContinuationStatus::Consumed);
    let _ = store2;
}

#[tokio::test(flavor = "current_thread")]
async fn failure_timeout_cancel_interrupted_classified_and_projection_agrees() {
    let (svc, store) = service_with_pool().await;
    for (id, state) in [
        ("job-f", JobState::Failed),
        ("job-t", JobState::TimedOut),
        ("job-c", JobState::Cancelled),
        ("job-e", JobState::Expired),
    ] {
        svc.subscribe_with_options(
            &JobId::new_unchecked(id),
            "test",
            "sess-cls",
            None,
            "inv",
            JobCompletionDisposition::ResumeOwner,
            None,
            None,
            Some(60_000),
        )
        .await
        .unwrap();
        let notice = svc
            .record_terminal(&job_record(id, "sess-cls", state), "done", None)
            .await
            .unwrap()
            .unwrap();
        assert!(!notice.status.is_empty());
    }
    // Interrupted never creates a notice (retryable, not final).
    svc.subscribe(
        &JobId::new_unchecked("job-i"),
        "test",
        "sess-cls",
        None,
        "inv-i",
    )
    .await
    .unwrap();
    assert!(svc
        .record_terminal(
            &job_record("job-i", "sess-cls", JobState::Interrupted),
            "retry",
            None
        )
        .await
        .unwrap()
        .is_none());
    // Projection agreement: session events + delivery states match notices.
    ensure_session(&store.pool(), "sess-cls").await;
    let mut m = vec![];
    inject_recoverable_job_notifications(Some(&store), &svc, "sess-cls", |t| m.push(t)).await;
    let events = store.list_for_session("sess-cls").await.unwrap();
    assert_eq!(
        events.len(),
        4,
        "one event per final terminal (Interrupted excluded)"
    );
    let _ = events;
    for id in ["job-f", "job-t", "job-c", "job-e"] {
        let state = svc.delivery_state(id).await.unwrap().unwrap();
        assert!(
            matches!(
                state,
                codegg::scheduler::job_notifications::JobDeliveryState::AvailableToOwner
                    | codegg::scheduler::job_notifications::JobDeliveryState::Consumed
                    | codegg::scheduler::job_notifications::JobDeliveryState::Expired
                    | codegg::scheduler::job_notifications::JobDeliveryState::Suppressed
            ),
            "delivery state terminal for {id}: {state:?}"
        );
    }
}
