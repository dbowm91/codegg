//! C002 explicitly authorized idle owner continuation.
//!
//! Default stays notify-only with zero autonomous tokens; only an explicit
//! host-trusted grant may start one bounded idle owner turn. Fake executor
//! only — no paid inference, no new CI lane.

use std::sync::Arc;

use codegg::scheduler::idle_continuation::{
    ContinuationService, DispatchOutcome, FakeContinuationExecutor, IdleContinuationCoordinator,
    NewGrant,
};
use codegg::scheduler::job_notifications::{JobCompletionDisposition, JobNotificationService};

use super::common;

fn grant_for(
    job: &str,
    session: &str,
    principal: Option<&str>,
    workspace: Option<&str>,
) -> NewGrant {
    NewGrant {
        notice_job_id: job.to_string(),
        session_id: session.to_string(),
        owner_principal: principal.map(ToOwned::to_owned),
        workspace_id: workspace.map(ToOwned::to_owned),
        origin_turn_id: Some("turn-origin".to_string()),
        allowed_model: Some("test-model".to_string()),
        allowed_connection: None,
        max_continuations: Some(1),
        max_tokens: Some(1_000),
        max_tool_calls: Some(8),
        max_duration_ms: Some(60_000),
        ttl_ms: Some(600_000),
    }
}

// ---------------------------------------------------------------------------
// 1. No-actor opt-in: explicit grant + terminal → exactly one canonical turn
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn opt_in_grant_dispatches_exactly_one_idle_turn() {
    let pool = common::pool::isolated_pool().await;
    let _notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = IdleContinuationCoordinator::new(service.clone(), executor.clone());

    let grant = service
        .authorize(grant_for("job-1", "sess-1", Some("user-1"), Some("ws-1")))
        .await
        .expect("authorize");
    // Originating turn has ended; terminal arrives while the daemon is idle
    // with no further user request or tool call.
    let outcome = coord
        .maybe_dispatch(
            "job-1",
            "job-1",
            "sess-1",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "build passed",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(outcome, DispatchOutcome::Dispatched);
    assert_eq!(executor.call_count().await, 1);
    let calls = executor.calls().await;
    assert_eq!(calls[0].session_id, "sess-1");
    assert_eq!(calls[0].grant_id, grant.grant_id);
    assert_eq!(calls[0].model.as_deref(), Some("test-model"));
    // Same notice+grant replay never queues a second turn.
    let replay = coord
        .maybe_dispatch(
            "job-1",
            "job-1",
            "sess-1",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "build passed",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(replay, DispatchOutcome::AlreadyDispatched);
    assert_eq!(executor.call_count().await, 1);
}

// ---------------------------------------------------------------------------
// 2. Default: notify_only / missing grant spends zero tokens, zero turns
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn default_notify_only_or_missing_grant_spends_nothing() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = IdleContinuationCoordinator::new(service.clone(), executor.clone());

    // notify_only disposition never dispatches, even with a grant present.
    let _grant = service
        .authorize(grant_for("job-2", "sess-2", Some("user-1"), Some("ws-1")))
        .await
        .expect("authorize");
    let out = coord
        .maybe_dispatch(
            "job-2",
            "job-2",
            "sess-2",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            false,
            true,
            true,
        )
        .await;
    assert_eq!(out, DispatchOutcome::Suppressed);
    // Model-supplied resume_owner without a grant is notify-only (denied).
    let out2 = coord
        .maybe_dispatch(
            "job-9",
            "job-9",
            "sess-9",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(out2, DispatchOutcome::Suppressed);
    assert_eq!(executor.call_count().await, 0, "zero provider calls");
}

// ---------------------------------------------------------------------------
// 3. CAS/crash matrix + 50 contenders, two frontends
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn concurrent_contenders_admit_exactly_one_turn() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = Arc::new(IdleContinuationCoordinator::new(
        service.clone(),
        executor.clone(),
    ));
    let _grant = service
        .authorize(grant_for("job-3", "sess-3", Some("user-1"), Some("ws-1")))
        .await
        .expect("authorize");
    let barrier = Arc::new(tokio::sync::Barrier::new(50));
    let mut handles = Vec::new();
    for _ in 0..50 {
        let c = coord.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            c.maybe_dispatch(
                "job-3",
                "job-3",
                "sess-3",
                Some("user-1"),
                Some("ws-1"),
                "completed",
                "ok",
                true,
                true,
                true,
            )
            .await
        }));
    }
    let mut dispatched = 0;
    let mut already = 0;
    for h in handles {
        match h.await.expect("join") {
            DispatchOutcome::Dispatched => dispatched += 1,
            DispatchOutcome::AlreadyDispatched => already += 1,
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    assert_eq!(dispatched, 1, "exactly one admitted continuation");
    assert_eq!(already, 49);
    assert_eq!(executor.call_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn ambiguous_provider_start_becomes_attention_not_replay() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::failing());
    let coord = IdleContinuationCoordinator::new(service.clone(), executor.clone());
    let _grant = service
        .authorize(grant_for("job-4", "sess-4", Some("user-1"), Some("ws-1")))
        .await
        .expect("authorize");
    let out = coord
        .maybe_dispatch(
            "job-4",
            "job-4",
            "sess-4",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(out, DispatchOutcome::NeedsAttention);
    // The ambiguous turn was accepted (one provider entry) but never replayed.
    assert_eq!(executor.call_count().await, 1);
    let retry = coord
        .maybe_dispatch(
            "job-4",
            "job-4",
            "sess-4",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(retry, DispatchOutcome::NeedsAttention);
    assert_eq!(executor.call_count().await, 1, "no blind replay");
}

// ---------------------------------------------------------------------------
// 4. Security/concurrency: every mismatch fails closed with notice retained
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn negative_lineage_cases_all_fail_closed() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = IdleContinuationCoordinator::new(service.clone(), executor.clone());
    let grant = service
        .authorize(grant_for("job-5", "sess-5", Some("user-1"), Some("ws-1")))
        .await
        .expect("authorize");

    // Wrong session / principal / workspace / statuses all suppress.
    for (session, principal, workspace, status, idle, provider) in [
        (
            "sess-other",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            true,
            true,
        ),
        (
            "sess-5",
            Some("user-2"),
            Some("ws-1"),
            "completed",
            true,
            true,
        ),
        (
            "sess-5",
            Some("user-1"),
            Some("ws-other"),
            "completed",
            true,
            true,
        ),
        (
            "sess-5",
            Some("user-1"),
            Some("ws-1"),
            "cancelled",
            true,
            true,
        ),
        (
            "sess-5",
            Some("user-1"),
            Some("ws-1"),
            "expired",
            true,
            true,
        ),
        (
            "sess-5",
            Some("user-1"),
            Some("ws-1"),
            "needs_attention",
            true,
            true,
        ),
        (
            "sess-5",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            false,
            true,
        ),
        (
            "sess-5",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            true,
            false,
        ),
    ] {
        let out = coord
            .maybe_dispatch(
                "job-5", "job-5", session, principal, workspace, status, "ok", true, idle, provider,
            )
            .await;
        assert_eq!(
            out,
            DispatchOutcome::Suppressed,
            "case {session}/{status:?}"
        );
    }
    // Revocation wins before invocation.
    assert!(service.revoke(&grant.grant_id).await.expect("revoke"));
    let out = coord
        .maybe_dispatch(
            "job-5",
            "job-5",
            "sess-5",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(out, DispatchOutcome::Suppressed);
    assert_eq!(executor.call_count().await, 0);
    // An observer cannot authorize private results (no grant for their session).
    let out = coord
        .maybe_dispatch(
            "job-5",
            "job-5",
            "sess-attacker",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(out, DispatchOutcome::Suppressed);
}

#[tokio::test(flavor = "current_thread")]
async fn expired_grant_and_exhausted_budget_suppress() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = IdleContinuationCoordinator::new(service.clone(), executor.clone());
    // Expired grant.
    let expired = NewGrant {
        ttl_ms: Some(1),
        ..grant_for("job-6", "sess-6", Some("user-1"), Some("ws-1"))
    };
    let _g = service.authorize(expired).await.expect("authorize");
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let out = coord
        .maybe_dispatch(
            "job-6",
            "job-6",
            "sess-6",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(out, DispatchOutcome::Suppressed);
    // Exhausted continuation budget (max 1, already used).
    let _g2 = service
        .authorize(grant_for("job-7", "sess-7", Some("user-1"), Some("ws-1")))
        .await
        .expect("authorize");
    let first = coord
        .maybe_dispatch(
            "job-7",
            "job-7",
            "sess-7",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(first, DispatchOutcome::Dispatched);
    let second = coord
        .maybe_dispatch(
            "job-7",
            "job-7",
            "sess-7",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(second, DispatchOutcome::AlreadyDispatched);
    assert_eq!(executor.call_count().await, 1);
}

// ---------------------------------------------------------------------------
// 5. Compatibility: legacy resume leases without grants never idle-dispatch
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn legacy_resume_lease_without_grant_stays_notify_only() {
    // M003 leases are preserved verbatim: the next-turn marker path is
    // untouched. The idle coordinator requires a grant, so a legacy
    // `resume_owner` subscription without one stays notify-only here.
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let job_id = codegg_core::jobs::JobId::new_unchecked("legacy-job");
    notifier
        .subscribe_with_options(
            &job_id,
            "test",
            "sess-legacy",
            None,
            "inv-legacy",
            JobCompletionDisposition::ResumeOwner,
            Some("user-1"),
            Some("ws-1"),
            None,
        )
        .await
        .expect("legacy subscribe");
    let lease = notifier
        .get_lease("legacy-job")
        .await
        .expect("lease")
        .expect("lease row");
    assert_eq!(
        lease.status,
        codegg::scheduler::job_notifications::JobContinuationStatus::Pending
    );
    // No grant exists for this job, so idle dispatch suppresses.
    let service = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = IdleContinuationCoordinator::new(service, executor.clone());
    let out = coord
        .maybe_dispatch(
            "legacy-job",
            "legacy-job",
            "sess-legacy",
            Some("user-1"),
            Some("ws-1"),
            "completed",
            "ok",
            true,
            true,
            true,
        )
        .await;
    assert_eq!(out, DispatchOutcome::Suppressed);
    assert_eq!(executor.call_count().await, 0);
}
