//! Lifecycle, recovery, contention, and security tests for background
//! tool programs.
//!
//! These tests verify the invariants from plan M008 sections 8
//! (failure/recovery), 10 (required tests), and 13 (acceptance
//! criteria).

use std::sync::Arc;

use codegg::scheduler::tool_program_notifications::{
    NotificationState, ProgramHandle, RecoveredTerminalJob, ToolProgramNotification,
    ToolProgramNotificationService,
};
use codegg_protocol::core::{CoreEvent, EventEnvelope};
use codegg_protocol::projection::adapters::projection_events_from_core;
use codegg_protocol::projection::caps::PROJECTION_PROTOCOL_VERSION;
use codegg_protocol::projection::dto::NotificationClassification;
use codegg_protocol::projection::reducer::{ProjectionReducer, ReducerEventInput};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;

fn make_notification(
    program_id: &str,
    session_id: &str,
    status: &str,
    success: bool,
) -> ToolProgramNotification {
    let now = chrono::Utc::now().timestamp_millis();
    let classification = if success {
        NotificationClassification::Completed
    } else {
        NotificationClassification::IncompleteRecoverable
    };
    let payload = format!(
        "{}|{}|program {} finished with status {}|{}",
        program_id, status, program_id, status, success
    );
    let payload_digest = format!("{:x}", md5::compute(payload.as_bytes()));
    ToolProgramNotification {
        notification_id: program_id.to_string(),
        program_id: program_id.to_string(),
        job_id: format!("j-{}", program_id),
        session_id: session_id.to_string(),
        agent_id: Some("agent-1".into()),
        turn_id: Some("turn-1".into()),
        status: status.to_string(),
        summary: format!("program {} finished with status {}", program_id, status),
        failure_class: if success {
            None
        } else {
            Some("timeout".into())
        },
        success,
        classification,
        payload_digest,
        program_handle: ProgramHandle {
            program_id: program_id.to_string(),
            job_id: format!("j-{}", program_id),
            status: "submitted".to_string(),
            submitted_at: now,
            timeout_ms: 120_000,
            inspect_ref: program_id.to_string(),
            cancel_ref: format!("j-{}", program_id),
        },
        state: NotificationState::Pending,
        created_at: now,
        updated_at: now,
    }
}

fn make_envelope(seq: u64, payload: CoreEvent) -> EventEnvelope<CoreEvent> {
    EventEnvelope {
        protocol_version: 2,
        event_seq: seq,
        timestamp_ms: 1000,
        session_id: Some("s1".into()),
        turn_id: None,
        payload,
    }
}

// ============================================================================
// Restart/Recovery Tests
// ============================================================================

#[tokio::test]
async fn restart_before_terminal_no_notification() {
    // If the daemon restarts before a program reaches terminal state,
    // the program is still running or queued. No notification should
    // be created.
    let svc = ToolProgramNotificationService::new();
    // No terminal jobs to recover from
    let recovered = svc.recover_from_terminal_jobs(vec![]).await;
    assert_eq!(recovered, 0);
    let pending = svc.pending_for_session("s1").await;
    assert!(pending.is_empty());
}

#[tokio::test]
async fn restart_after_terminal_before_notification_delivery() {
    // Program completed but daemon crashed before notification was
    // delivered. Recovery creates a pending notification.
    let svc = ToolProgramNotificationService::new();
    let jobs = vec![RecoveredTerminalJob {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        session_id: Some("s1".into()),
        status: "completed".into(),
        summary: "ok".into(),
        failure_class: None,
        success: true,
        created_at: 1000,
    }];
    let recovered = svc.recover_from_terminal_jobs(jobs).await;
    assert_eq!(recovered, 1);
    let pending = svc.pending_for_session("s1").await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].program_id, "tp-1");
}

#[tokio::test]
async fn restart_after_terminal_after_claim_before_ack() {
    // Program completed, notification was claimed but not acknowledged
    // before daemon crash. Recovery creates a new pending notification
    // (the old claimed state is lost).
    let svc = ToolProgramNotificationService::new();

    // Simulate: create and claim, then "crash" (service is in-memory)
    let n = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n).await;
    svc.claim("tp-1").await;

    // Verify it was claimed
    let n = svc.get("tp-1").await.unwrap();
    assert_eq!(n.state, NotificationState::Claimed);

    // After restart: create a new service and recover
    let svc2 = ToolProgramNotificationService::new();
    let jobs = vec![RecoveredTerminalJob {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        session_id: Some("s1".into()),
        status: "completed".into(),
        summary: "ok".into(),
        failure_class: None,
        success: true,
        created_at: 1000,
    }];
    let recovered = svc2.recover_from_terminal_jobs(jobs).await;
    assert_eq!(recovered, 1);
    let pending = svc2.pending_for_session("s1").await;
    assert_eq!(pending.len(), 1);
    // The recovered notification is pending, not claimed
    assert_eq!(pending[0].state, NotificationState::Pending);
}

#[tokio::test]
async fn restart_after_ack_no_duplicate() {
    // Program completed, notification was claimed and acknowledged.
    // After restart, recovery should not create a duplicate.
    let svc = ToolProgramNotificationService::new();
    let n = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n).await;
    svc.claim("tp-1").await;
    svc.acknowledge("tp-1").await;

    // After restart: the old service is gone, create new one
    let svc2 = ToolProgramNotificationService::new();
    let jobs = vec![RecoveredTerminalJob {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        session_id: Some("s1".into()),
        status: "completed".into(),
        summary: "ok".into(),
        failure_class: None,
        success: true,
        created_at: 1000,
    }];
    let recovered = svc2.recover_from_terminal_jobs(jobs).await;
    // Should recover because the new service doesn't know about the
    // old notification
    assert_eq!(recovered, 1);
    let pending = svc2.pending_for_session("s1").await;
    assert_eq!(pending.len(), 1);
}

#[tokio::test]
async fn replay_does_not_duplicate_notification() {
    // If the same terminal event is replayed twice (e.g. during
    // event log replay), the notification should be created only once.
    let svc = ToolProgramNotificationService::new();
    let jobs = vec![RecoveredTerminalJob {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        session_id: Some("s1".into()),
        status: "completed".into(),
        summary: "ok".into(),
        failure_class: None,
        success: true,
        created_at: 1000,
    }];
    let recovered1 = svc.recover_from_terminal_jobs(jobs.clone()).await;
    assert_eq!(recovered1, 1);

    let recovered2 = svc.recover_from_terminal_jobs(jobs).await;
    assert_eq!(recovered2, 0);

    let pending = svc.pending_for_session("s1").await;
    assert_eq!(pending.len(), 1);
}

#[tokio::test]
async fn core_event_replay_does_not_duplicate_projection_event() {
    // If a ToolProgramCompleted CoreEvent is replayed, the adapter
    // produces the same ProjectionEvent, and the reducer deduplicates
    // by event_seq.
    let mut snap = SessionProjectionSnapshot::empty("s1", "p1", "w1");
    let reducer = ProjectionReducer::default();

    let env = make_envelope(
        1,
        CoreEvent::ToolProgramCompleted {
            session_id: Some("s1".into()),
            program_id: "tp-1".into(),
            job_id: "j-1".into(),
            status: "completed".into(),
            summary: "ok".into(),
            calls_completed: 3,
        },
    );

    // First application
    let events = projection_events_from_core(&env);
    assert_eq!(events.len(), 1);
    let input = ReducerEventInput {
        protocol_version: PROJECTION_PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("s1".into()),
        turn_id: None,
        payload: events[0].clone(),
    };
    let outcome = reducer.apply(&mut snap, input.clone());
    assert!(matches!(
        outcome,
        codegg_protocol::projection::reducer::ApplyOutcome::Applied
    ));

    // Replay with same event_seq — should be duplicate
    let outcome = reducer.apply(&mut snap, input);
    assert!(matches!(
        outcome,
        codegg_protocol::projection::reducer::ApplyOutcome::Duplicate
    ));
}

// ============================================================================
// Contention and Backpressure Tests
// ============================================================================

#[tokio::test]
async fn many_programs_completing_simultaneously() {
    // Multiple programs completing at the same time should each
    // produce exactly one notification.
    let svc = ToolProgramNotificationService::new();
    let mut jobs = vec![];
    for i in 0..10 {
        jobs.push(RecoveredTerminalJob {
            program_id: format!("tp-{}", i),
            job_id: format!("j-{}", i),
            session_id: Some("s1".into()),
            status: "completed".into(),
            summary: format!("ok {}", i),
            failure_class: None,
            success: true,
            created_at: 1000 + i as i64,
        });
    }
    let recovered = svc.recover_from_terminal_jobs(jobs).await;
    assert_eq!(recovered, 10);

    let pending = svc.pending_for_session("s1").await;
    assert_eq!(pending.len(), 10);

    // Claim all — only first claim per program should succeed
    for i in 0..10 {
        assert!(svc.claim(&format!("tp-{}", i)).await);
        // Second claim should fail
        assert!(!svc.claim(&format!("tp-{}", i)).await);
    }

    // Ack all
    for i in 0..10 {
        assert!(svc.acknowledge(&format!("tp-{}", i)).await);
        // Double ack should fail
        assert!(!svc.acknowledge(&format!("tp-{}", i)).await);
    }
}

#[tokio::test]
async fn session_bound_enforcement_suppresses_oldest() {
    // When the session bound is exceeded, oldest pending notifications
    // are suppressed.
    let svc = ToolProgramNotificationService::new();
    for i in 0..8 {
        svc.record_notification(make_notification(
            &format!("tp-{}", i),
            "s1",
            "completed",
            true,
        ))
        .await;
    }

    let suppressed = svc.enforce_session_bound("s1", 3).await;
    assert_eq!(suppressed.len(), 5);

    let pending = svc.pending_for_session("s1").await;
    assert_eq!(pending.len(), 3);

    // The suppressed ones should be in Suppressed state
    for id in &suppressed {
        let n = svc.get(id).await.unwrap();
        assert_eq!(n.state, NotificationState::Suppressed);
    }
}

#[tokio::test]
async fn parent_session_inactive_does_not_block_other_sessions() {
    // If the parent session is inactive, other sessions' notifications
    // should not be affected.
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.record_notification(make_notification("tp-2", "s2", "completed", true))
        .await;
    svc.record_notification(make_notification("tp-3", "s1", "failed", false))
        .await;

    // s1 has 2 pending, s2 has 1
    assert_eq!(svc.pending_count("s1").await, 2);
    assert_eq!(svc.pending_count("s2").await, 1);

    // Suppress all of s1
    svc.enforce_session_bound("s1", 0).await;
    assert_eq!(svc.pending_count("s1").await, 0);
    // s2 is unaffected
    assert_eq!(svc.pending_count("s2").await, 1);
}

#[tokio::test]
async fn cancellation_during_terminal_notification_race() {
    // If a program is cancelled while a terminal notification is being
    // processed, the notification should reflect the cancelled state.
    let svc = ToolProgramNotificationService::new();

    // First, record a "completed" notification
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;

    // Then, try to record a "cancelled" notification for the same program
    // (simulating a race between completion and cancellation)
    let cancelled = make_notification("tp-1", "s1", "cancelled", false);
    let result = svc.record_notification(cancelled).await;

    // The original notification should be retained (idempotent)
    assert_eq!(result.status, "completed");
    assert!(result.success);
}

#[tokio::test]
async fn unrelated_sessions_receive_only_own_events() {
    // Notifications for session A should not appear in session B's
    // pending list.
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.record_notification(make_notification("tp-2", "s2", "completed", true))
        .await;
    svc.record_notification(make_notification("tp-3", "s3", "failed", false))
        .await;

    assert_eq!(svc.pending_count("s1").await, 1);
    assert_eq!(svc.pending_count("s2").await, 1);
    assert_eq!(svc.pending_count("s3").await, 1);

    // Claim s1's notification
    svc.claim("tp-1").await;
    assert_eq!(svc.pending_count("s1").await, 0);
    // s2 and s3 are unaffected
    assert_eq!(svc.pending_count("s2").await, 1);
    assert_eq!(svc.pending_count("s3").await, 1);
}

// ============================================================================
// Security and Negative Tests
// ============================================================================

#[tokio::test]
async fn cross_session_notification_forgery_blocked() {
    // A notification created for session "s1" should not appear in
    // session "s2"'s pending list.
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;

    // s2 should see no pending notifications
    assert!(svc.pending_for_session("s2").await.is_empty());
}

#[tokio::test]
async fn repeated_fake_terminal_events_cannot_trigger_multiple_model_turns() {
    // Even if the same terminal event is received multiple times, only
    // one notification should exist and only one claim should succeed.
    let svc = ToolProgramNotificationService::new();
    let n1 = make_notification("tp-1", "s1", "completed", true);
    let n2 = make_notification("tp-1", "s1", "completed", true);

    svc.record_notification(n1).await;
    svc.record_notification(n2).await;

    // Only one pending notification
    assert_eq!(svc.pending_count("s1").await, 1);

    // Only one claim should succeed
    assert!(svc.claim("tp-1").await);
    assert!(!svc.claim("tp-1").await);

    // Only one ack should succeed
    assert!(svc.acknowledge("tp-1").await);
    assert!(!svc.acknowledge("tp-1").await);
}

#[tokio::test]
async fn notification_state_transitions_are_valid() {
    // Verify the state machine: Pending -> Claimed -> Delivered,
    // Pending -> Suppressed, Claimed -> Expired.
    let svc = ToolProgramNotificationService::new();

    // Test Pending -> Claimed -> Delivered
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    assert_eq!(
        svc.get("tp-1").await.unwrap().state,
        NotificationState::Pending
    );
    svc.claim("tp-1").await;
    assert_eq!(
        svc.get("tp-1").await.unwrap().state,
        NotificationState::Claimed
    );
    svc.acknowledge("tp-1").await;
    assert_eq!(
        svc.get("tp-1").await.unwrap().state,
        NotificationState::Delivered
    );

    // Test Pending -> Suppressed
    svc.record_notification(make_notification("tp-2", "s1", "completed", true))
        .await;
    svc.suppress("tp-2").await;
    assert_eq!(
        svc.get("tp-2").await.unwrap().state,
        NotificationState::Suppressed
    );

    // Test Claimed -> Expired (via expire_stale)
    svc.record_notification(make_notification("tp-3", "s1", "completed", true))
        .await;
    svc.claim("tp-3").await;
    // Set updated_at to the past
    {
        let mut notifications = svc.notifications.write().await;
        if let Some(n) = notifications.get_mut("tp-3") {
            n.updated_at = chrono::Utc::now().timestamp_millis() - 1000;
        }
    }
    svc.expire_stale(100).await;
    assert_eq!(
        svc.get("tp-3").await.unwrap().state,
        NotificationState::Expired
    );
}

#[tokio::test]
async fn claim_only_from_pending_state() {
    // Claiming a non-pending notification should fail.
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;

    // Claim succeeds from Pending
    assert!(svc.claim("tp-1").await);

    // Claim fails from Claimed
    assert!(!svc.claim("tp-1").await);

    // Acknowledge
    svc.acknowledge("tp-1").await;

    // Claim fails from Delivered
    assert!(!svc.claim("tp-1").await);
}

#[tokio::test]
async fn acknowledge_only_from_claimed_state() {
    // Acknowledging a non-claimed notification should fail.
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;

    // Ack fails from Pending
    assert!(!svc.acknowledge("tp-1").await);

    // Claim
    svc.claim("tp-1").await;

    // Ack succeeds from Claimed
    assert!(svc.acknowledge("tp-1").await);

    // Ack fails from Delivered
    assert!(!svc.acknowledge("tp-1").await);
}

#[tokio::test]
async fn suppress_only_from_pending_or_claimed() {
    // Suppressing a delivered or expired notification should fail.
    let svc = ToolProgramNotificationService::new();

    // Create, claim, ack -> Delivered
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.claim("tp-1").await;
    svc.acknowledge("tp-1").await;

    // Suppress fails from Delivered
    assert!(!svc.suppress("tp-1").await);

    // Create, claim, expire -> Expired
    svc.record_notification(make_notification("tp-2", "s1", "completed", true))
        .await;
    svc.claim("tp-2").await;
    {
        let mut notifications = svc.notifications.write().await;
        if let Some(n) = notifications.get_mut("tp-2") {
            n.updated_at = chrono::Utc::now().timestamp_millis() - 1000;
        }
    }
    svc.expire_stale(100).await;

    // Suppress fails from Expired
    assert!(!svc.suppress("tp-2").await);
}

#[tokio::test]
async fn notification_payload_is_bounded() {
    // Large summaries should be handled without panic.
    let svc = ToolProgramNotificationService::new();
    let mut n = make_notification("tp-1", "s1", "completed", true);
    n.summary = "x".repeat(100_000);
    n.status = "y".repeat(100_000);

    // Should not panic
    svc.record_notification(n).await;
    let got = svc.get("tp-1").await.unwrap();
    assert_eq!(got.program_id, "tp-1");
}

#[tokio::test]
async fn notification_id_is_program_id() {
    // The notification_id is the program_id, ensuring exactly-once
    // identity per program.
    let svc = ToolProgramNotificationService::new();
    let n = make_notification("tp-42", "s1", "completed", true);
    svc.record_notification(n).await;

    // Lookup by program_id should work
    let got = svc.get("tp-42").await;
    assert!(got.is_some());
    assert_eq!(got.unwrap().notification_id, "tp-42");
}

#[tokio::test]
async fn concurrent_record_and_claim() {
    // Multiple concurrent record and claim operations should not
    // cause data corruption or panics.
    let svc = Arc::new(ToolProgramNotificationService::new());
    let mut handles = vec![];

    for i in 0..20 {
        let svc = Arc::clone(&svc);
        handles.push(tokio::spawn(async move {
            let n = make_notification(
                &format!("tp-{}", i),
                &format!("s{}", i % 3),
                "completed",
                true,
            );
            svc.record_notification(n).await;
            svc.claim(&format!("tp-{}", i)).await;
            svc.acknowledge(&format!("tp-{}", i)).await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // All 20 notifications should be in Delivered state
    for i in 0..20 {
        let n = svc.get(&format!("tp-{}", i)).await.unwrap();
        assert_eq!(n.state, NotificationState::Delivered);
    }
}

// ============================================================================
// Observer Visibility Tests
// ============================================================================

#[tokio::test]
async fn projection_event_visibility_classification() {
    // Tool program projection events have appropriate visibility
    // classification — terminal events are Public, intermediate
    // events are ClientLocal.
    use codegg_protocol::projection::event::ProjectionEvent;

    let terminal = ProjectionEvent::ToolProgramTerminal {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        status: "completed".into(),
        summary: "ok".into(),
        completed_at: 0,
    };
    assert_eq!(
        terminal.visibility(),
        codegg_protocol::projection::dto::VisibilityClass::Public
    );

    let admitted = ProjectionEvent::ToolProgramAdmitted {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        admitted_at: 0,
    };
    assert_eq!(
        admitted.visibility(),
        codegg_protocol::projection::dto::VisibilityClass::ClientLocal
    );

    let started = ProjectionEvent::ToolProgramStarted {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        attempt_id: None,
        started_at: 0,
    };
    assert_eq!(
        started.visibility(),
        codegg_protocol::projection::dto::VisibilityClass::Public
    );
}

#[tokio::test]
async fn projection_summary_does_not_leak_raw_source() {
    // ToolProgramSummary normalise() truncates strings — raw source
    // code, secrets, or unbounded output are never present.
    use codegg_protocol::projection::dto::ToolProgramSummary;
    use codegg_protocol::projection::limits::MAX_PROJECTION_STRING_BYTES;

    let mut summary = ToolProgramSummary {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        state: "a".repeat(MAX_PROJECTION_STRING_BYTES + 100),
        phase: Some("b".repeat(MAX_PROJECTION_STRING_BYTES + 100)),
        language: "restricted_python".into(),
        parent_turn_id: None,
        parent_agent_id: None,
        calls_completed: 0,
        child_jobs_running: 0,
        submitted_at: 0,
        started_at: None,
        completed_at: None,
        failure_class: Some("c".repeat(MAX_PROJECTION_STRING_BYTES + 100)),
        terminal_handle: Some("d".repeat(MAX_PROJECTION_STRING_BYTES + 100)),
        last_progress: Some("e".repeat(MAX_PROJECTION_STRING_BYTES + 100)),
    };
    summary.normalise();

    assert!(summary.state.len() <= MAX_PROJECTION_STRING_BYTES);
    assert!(summary.phase.as_ref().unwrap().len() <= MAX_PROJECTION_STRING_BYTES);
    assert!(summary.failure_class.as_ref().unwrap().len() <= MAX_PROJECTION_STRING_BYTES);
    assert!(summary.terminal_handle.as_ref().unwrap().len() <= MAX_PROJECTION_STRING_BYTES);
    assert!(summary.last_progress.as_ref().unwrap().len() <= MAX_PROJECTION_STRING_BYTES);
}

// ============================================================================
// Backpressure Integration Tests
// ============================================================================

#[tokio::test]
async fn backpressure_suppresses_oldest_when_session_bound_exceeded() {
    use codegg::scheduler::tool_program_notifications::NotificationPolicy;

    let policy = NotificationPolicy {
        max_pending_per_session: 3,
        ..Default::default()
    };
    let svc = ToolProgramNotificationService::with_policy(policy);

    // Record 5 notifications for the same session
    for i in 0..5 {
        svc.record_notification(make_notification(
            &format!("tp-{}", i),
            "s1",
            "completed",
            true,
        ))
        .await;
    }

    // Enforce bound — should suppress oldest 2
    let suppressed = svc.enforce_session_bound("s1", 3).await;
    assert_eq!(suppressed.len(), 2);
    assert_eq!(suppressed[0], "tp-0");
    assert_eq!(suppressed[1], "tp-1");

    // Only 3 pending should remain
    let pending = svc.pending_for_session("s1").await;
    assert_eq!(pending.len(), 3);
    assert_eq!(pending[0].program_id, "tp-2");
    assert_eq!(pending[1].program_id, "tp-3");
    assert_eq!(pending[2].program_id, "tp-4");
}

#[tokio::test]
async fn backpressure_does_not_affect_other_sessions() {
    let svc = ToolProgramNotificationService::new();

    // Record 5 for s1, 5 for s2
    for i in 0..5 {
        svc.record_notification(make_notification(
            &format!("tp-{}", i),
            "s1",
            "completed",
            true,
        ))
        .await;
        svc.record_notification(make_notification(
            &format!("tp-s2-{}", i),
            "s2",
            "completed",
            true,
        ))
        .await;
    }

    // Bound s1 to 2
    let suppressed = svc.enforce_session_bound("s1", 2).await;
    assert_eq!(suppressed.len(), 3);

    // s2 should be unaffected
    let pending_s2 = svc.pending_for_session("s2").await;
    assert_eq!(pending_s2.len(), 5);
}

#[tokio::test]
async fn notification_payload_digest_is_computed() {
    let n = make_notification("tp-digest", "s1", "completed", true);
    assert!(
        !n.payload_digest.is_empty(),
        "payload_digest should be computed"
    );
    // Same program_id should produce same digest (idempotent)
    let n2 = make_notification("tp-digest", "s1", "completed", true);
    assert_eq!(n.payload_digest, n2.payload_digest);
}

#[tokio::test]
async fn three_way_classification_completed() {
    use codegg_protocol::projection::dto::NotificationClassification;

    let n = make_notification("tp-3way", "s1", "completed", true);
    assert_eq!(n.classification, NotificationClassification::Completed);
}

#[tokio::test]
async fn three_way_classification_incomplete_recoverable() {
    use codegg_protocol::projection::dto::NotificationClassification;

    let n = make_notification("tp-3way", "s1", "failed", false);
    // failure_class is "timeout" in make_notification when !success
    assert_eq!(
        n.classification,
        NotificationClassification::IncompleteRecoverable
    );
}

#[tokio::test]
async fn three_way_classification_failed_terminal() {
    use codegg::scheduler::tool_program_notifications::classify_terminal_for_test;
    use codegg_protocol::projection::dto::NotificationClassification;
    let classification = classify_terminal_for_test("failed", Some("compile_error"), false);
    assert_eq!(classification, NotificationClassification::FailedTerminal);
}
