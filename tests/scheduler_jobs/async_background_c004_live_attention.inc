//! C004 production idle-delivery qualification (delivery slice).
//!
//! Proves, against a real `CoreDaemon`, that a durable terminal commit
//! (machine job or CI-watch-shaped notice) reaches an owner-scoped live
//! hint without any agent turn, without harness `maybe_dispatch`, and
//! without spending provider tokens:
//!
//! - The daemon owns one `JobNotificationService`, one `CiWatchService`,
//!   one `CiWatchIdleDriver` (when pooled), and one
//!   `ContinuationService`; the notifier fans out each first-write
//!   terminal to exactly one live `CoreEvent::BackgroundJobCompleted`.
//! - The live envelope carries session/notice/job identity, kind,
//!   terminal status, and a bounded redacted summary — never stdout
//!   dumps, credentials, raw CI logs, or remote instructions.
//! - Owner-scoped replay returns the identical notice id/status exactly
//!   once; an unaffiliated session filter returns none; a restarted
//!   daemon over the same pool replays the same notice id (persisted
//!   `EventLog`, no second agent turn).
//! - Duplicate terminal commits never publish a second live event.
//! - Autonomous provider resume stays blocked: owning the continuation
//!   store never starts a turn; grants alone dispatch nothing.
//!
//! Strictly forbidden in this file (per C004 §5): `maybe_dispatch`,
//! `CoreRequest::TurnSubmit`, `ci_watch.status`, user-message injection,
//! `observe_once`, `advance_due_watches`, or `pending_for_session` as a
//! substitute for live/replay proof. `pending_for_session` is never
//! called here.

use std::time::Duration;

use codegg::core::daemon::CoreDaemon;
use codegg::core::event_log::EventFilter;
use codegg::protocol::core::CoreEvent;
use codegg_core::jobs::{JobId, JobKind, JobPriority, JobRecord, JobSource, JobState};
use codegg_core::workspace::WorkspaceId;

use super::common;

fn job_record(job_id: &str, session: Option<&str>, state: JobState) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked(job_id),
        workspace_id: WorkspaceId::new_unchecked("ws-c004"),
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
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
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

fn owner_filter(session: &str) -> EventFilter {
    EventFilter {
        session_id: Some(session.to_string()),
        client_id: None,
        include_global: false,
    }
}

async fn wait_for_background_event(
    rx: &mut tokio::sync::broadcast::Receiver<codegg::protocol::core::EventEnvelope<CoreEvent>>,
    notice_id: &str,
    timeout: Duration,
) -> codegg::protocol::core::EventEnvelope<CoreEvent> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!("timed out waiting for live BackgroundJobCompleted {notice_id}"),
            recv = rx.recv() => {
                let env = recv.expect("event log sender must stay alive");
                if let CoreEvent::BackgroundJobCompleted { notice_id: got, .. } = &env.payload {
                    if got == notice_id {
                        return env;
                    }
                }
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_owns_singletons_and_publishes_live_completion_to_owner() {
    let pool = common::pool::isolated_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);

    // Daemon ownership proof: singletons exist and the notifier pointer is
    // the same handle the scheduler and every turn share.
    let notifier = daemon
        .deps
        .job_notification_service
        .clone()
        .expect("daemon owns a notifier");
    assert!(daemon.deps.ci_watch_service.is_some());
    assert!(daemon.ci_watch_driver.is_some());
    assert!(daemon.continuation_service.is_some());
    let again = daemon
        .deps
        .job_notification_service
        .clone()
        .expect("notifier handle");
    assert!(std::sync::Arc::ptr_eq(&notifier, &again));
    let store = daemon
        .continuation_service
        .clone()
        .expect("daemon owns continuation store");
    assert!(std::sync::Arc::ptr_eq(
        &store,
        daemon
            .deps
            .continuation_service
            .as_ref()
            .expect("deps handle")
    ));

    let mut rx = daemon.event_log.subscribe();
    let session = "sess-c004-owner";

    notifier
        .subscribe(
            &JobId::new_unchecked("job-c004-live-1"),
            "test",
            session,
            None,
            "inv-c004-live-1",
        )
        .await
        .expect("subscribe");

    let notice = notifier
        .record_terminal(
            &job_record("job-c004-live-1", Some(session), JobState::Completed),
            "build green",
            None,
        )
        .await
        .expect("record")
        .expect("subscribed terminal produces a notice");

    // Live proof: the owning daemon publishes without any turn.
    let env = wait_for_background_event(&mut rx, &notice.notice_id, Duration::from_secs(10)).await;
    assert_eq!(env.session_id.as_deref(), Some(session));
    match &env.payload {
        CoreEvent::BackgroundJobCompleted {
            session_id,
            notice_id,
            job_id,
            kind,
            status,
            summary,
        } => {
            assert_eq!(session_id, session);
            assert_eq!(notice_id, &notice.notice_id);
            assert_eq!(job_id, &notice.job_id);
            assert_eq!(kind, &notice.kind);
            assert_eq!(status, "completed");
            assert!(summary.contains("build green"));
            assert!(!summary.contains("Bearer"));
            assert!(!summary.contains("GITHUB_TOKEN"));
        }
        other => panic!("expected BackgroundJobCompleted, got {other:?}"),
    }

    // Exactly-once: a duplicate terminal commit is idempotent and never
    // publishes a second live event.
    let dup = notifier
        .record_terminal(
            &job_record("job-c004-live-1", Some(session), JobState::Completed),
            "build green",
            None,
        )
        .await
        .expect("duplicate record")
        .expect("duplicate returns the same notice");
    assert_eq!(dup.notice_id, notice.notice_id);
    let no_second = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(
        no_second.is_err(),
        "duplicate terminal must not publish a second live event"
    );

    // Replay proof: owner filter returns the identical notice exactly once;
    // an unaffiliated session sees nothing.
    let replay = daemon
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    let matching: Vec<_> = replay
        .iter()
        .filter_map(|e| match &e.payload {
            CoreEvent::BackgroundJobCompleted {
                notice_id, status, ..
            } if notice_id == &notice.notice_id => Some(status.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(matching, vec!["completed".to_string()]);
    let foreign = daemon
        .event_log
        .replay_from(0, &owner_filter("sess-c004-stranger"))
        .await;
    assert!(
        foreign
            .iter()
            .all(|e| e.session_id.as_deref() != Some(session)),
        "unaffiliated filter must not observe the owner's completion"
    );

    // Notify-only spends zero inference: no turn was admitted for the
    // session (no TurnStarted envelope exists for it).
    let replay_all = daemon
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    assert!(
        !replay_all
            .iter()
            .any(|e| matches!(e.payload, CoreEvent::TurnStarted { .. })),
        "delivery alone must never admit a turn"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn watch_shaped_terminal_shares_single_fanout_and_router_hint() {
    let pool = common::pool::isolated_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let notifier = daemon
        .deps
        .job_notification_service
        .clone()
        .expect("daemon notifier");
    let mut rx = daemon.event_log.subscribe();
    let session = "sess-c004-watch";
    let watch_job = "ciwatch:watch-c004-1";

    notifier
        .subscribe(
            &JobId::new_unchecked(watch_job),
            "ci_watch",
            session,
            None,
            "inv-c004-watch-1",
        )
        .await
        .expect("watch subscription");

    let notice = notifier
        .record_terminal_for_watch(
            watch_job,
            "ci_watch",
            session,
            None,
            "completed",
            "ci green",
            None,
        )
        .await
        .expect("watch terminal")
        .expect("watch terminal produces a notice");

    let env = wait_for_background_event(&mut rx, &notice.notice_id, Duration::from_secs(10)).await;
    match &env.payload {
        CoreEvent::BackgroundJobCompleted {
            status, summary, ..
        } => {
            assert_eq!(status, "completed");
            assert!(summary.contains("ci green"));
        }
        other => panic!("expected BackgroundJobCompleted, got {other:?}"),
    }

    // Ephemeral router hint exists (dedupe-keyed by notice id) but is not a
    // second completion database: the durable row stays the authority.
    assert!(
        daemon.notification_router.queue_len().await >= 1,
        "router should hold the completion hint"
    );
    let stored = notifier
        .get(&notice.notice_id)
        .await
        .expect("load notice")
        .expect("notice durable");
    assert_eq!(stored.status, "completed");
}

#[tokio::test(flavor = "current_thread")]
async fn replay_survives_daemon_restart_without_second_turn() {
    let pool = common::pool::isolated_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let notifier = daemon
        .deps
        .job_notification_service
        .clone()
        .expect("notifier");
    let mut rx = daemon.event_log.subscribe();
    let session = "sess-c004-restart";

    notifier
        .subscribe(
            &JobId::new_unchecked("job-c004-restart-1"),
            "test",
            session,
            None,
            "inv-c004-restart-1",
        )
        .await
        .expect("subscribe");
    let notice = notifier
        .record_terminal(
            &job_record("job-c004-restart-1", Some(session), JobState::Completed),
            "restart proof",
            None,
        )
        .await
        .expect("record")
        .expect("notice");
    wait_for_background_event(&mut rx, &notice.notice_id, Duration::from_secs(10)).await;
    drop(rx);
    drop(daemon);

    // Simulated restart: a fresh daemon over the same pool replays the same
    // durable live event from `core_event_log` with no new turn.
    let restarted = CoreDaemon::new(Some(pool.clone()), None, None);
    let replay = restarted
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    let matching: Vec<_> = replay
        .iter()
        .filter_map(|e| match &e.payload {
            CoreEvent::BackgroundJobCompleted {
                notice_id, status, ..
            } if notice_id == &notice.notice_id => Some(status.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        matching,
        vec!["completed".to_string()],
        "restart must replay the identical notice id/status"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn service_fanout_fires_once_for_first_write_only() {
    use codegg::scheduler::job_notifications::JobNotificationService;

    let pool = common::pool::isolated_pool().await;
    let svc = JobNotificationService::with_pool(pool);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<
        codegg::scheduler::job_notifications::JobTerminalNotice,
    >();
    svc.set_completion_hook(tx).await;
    svc.subscribe(
        &JobId::new_unchecked("job-c004-once"),
        "test",
        "sess-c004-once",
        None,
        "inv-c004-once",
    )
    .await
    .expect("subscribe");
    let first = svc
        .record_terminal(
            &job_record("job-c004-once", Some("sess-c004-once"), JobState::Completed),
            "once",
            None,
        )
        .await
        .expect("record")
        .expect("notice");
    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("fanout fires")
        .expect("notice");
    assert_eq!(got.notice_id, first.notice_id);
    // Duplicate commit: same notice returned, no second fanout.
    let dup = svc
        .record_terminal(
            &job_record("job-c004-once", Some("sess-c004-once"), JobState::Completed),
            "once",
            None,
        )
        .await
        .expect("duplicate")
        .expect("notice");
    assert_eq!(dup.notice_id, first.notice_id);
    assert!(
        tokio::time::timeout(Duration::from_millis(300), rx.recv())
            .await
            .is_err(),
        "duplicate must not fan out"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn continuation_store_owned_but_idle_resume_stays_blocked() {
    use codegg::scheduler::idle_continuation::NewGrant;

    let pool = common::pool::isolated_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let store = daemon
        .continuation_service
        .clone()
        .expect("daemon owns continuation store");
    let mut rx = daemon.event_log.subscribe();
    let session = "sess-c004-blocked";

    // A grant row can exist (service-level authorize), but the production
    // daemon wires no coordinator/executor, so completion still delivers
    // attention only and admits no turn.
    let grant = store
        .authorize(NewGrant {
            notice_job_id: "job-c004-blocked-1".to_string(),
            session_id: session.to_string(),
            owner_principal: Some("user-1".to_string()),
            workspace_id: Some("ws-c004".to_string()),
            origin_turn_id: None,
            allowed_model: Some("test-model".to_string()),
            allowed_connection: None,
            max_continuations: Some(1),
            max_tokens: Some(1_000),
            max_tool_calls: Some(8),
            max_duration_ms: Some(60_000),
            ttl_ms: Some(600_000),
        })
        .await
        .expect("grant stores");
    assert!(store.revoke(&grant.grant_id).await.expect("revoke"));

    let notifier = daemon
        .deps
        .job_notification_service
        .clone()
        .expect("notifier");
    notifier
        .subscribe(
            &JobId::new_unchecked("job-c004-blocked-1"),
            "test",
            session,
            None,
            "inv-c004-blocked-1",
        )
        .await
        .expect("subscribe");
    let notice = notifier
        .record_terminal(
            &job_record("job-c004-blocked-1", Some(session), JobState::Completed),
            "blocked proof",
            None,
        )
        .await
        .expect("record")
        .expect("notice");
    wait_for_background_event(&mut rx, &notice.notice_id, Duration::from_secs(10)).await;

    // No turn admission followed the grant + completion: the only
    // session-scoped envelopes are the completion hint itself.
    let replay = daemon
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    assert!(
        replay.iter().all(|e| !matches!(
            e.payload,
            CoreEvent::TurnStarted { .. }
                | CoreEvent::TurnCompleted { .. }
                | CoreEvent::TurnFailed { .. }
        )),
        "blocked resume must never admit a provider turn"
    );
}

/// Crash-gap healing (ADR-0031 §Decision.5): a terminal committed
/// without any forwarder (crash between the notice commit and the hint
/// publish) gains its persisted live hint from startup reconciliation —
/// exactly once, with no turn, and a second reconciliation is a no-op.
#[tokio::test(flavor = "current_thread")]
async fn crash_between_commit_and_hint_heals_by_reconciliation() {
    use codegg::scheduler::job_notifications::JobNotificationService;

    let pool = common::pool::isolated_pool().await;
    let session = "sess-c004-crash-gap";

    // Commit the terminal through a bare service with no completion hook
    // and no EventLog: the durable notice exists, but no live hint was
    // ever published (the simulated crash window).
    let bare = JobNotificationService::with_pool(pool.clone());
    bare.subscribe(
        &JobId::new_unchecked("job-c004-crash-1"),
        "test",
        session,
        None,
        "inv-c004-crash-1",
    )
    .await
    .expect("subscribe");
    let notice = bare
        .record_terminal(
            &job_record("job-c004-crash-1", Some(session), JobState::Completed),
            "crash gap proof",
            None,
        )
        .await
        .expect("record")
        .expect("notice durable");

    // A fresh daemon over the same pool heals exactly one hint.
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let healed = daemon.reconcile_idle_completion_hints().await;
    assert_eq!(healed, 1, "one missing hint must be republished");

    let replay = daemon
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    let matching: Vec<_> = replay
        .iter()
        .filter_map(|e| match &e.payload {
            CoreEvent::BackgroundJobCompleted {
                notice_id, status, ..
            } if notice_id == &notice.notice_id => Some(status.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        matching,
        vec!["completed".to_string()],
        "healed hint carries the identical notice id/status exactly once"
    );

    // Healing is idempotent: a second pass republishes nothing.
    let again = daemon.reconcile_idle_completion_hints().await;
    assert_eq!(again, 0, "reconciliation must not duplicate hints");
    let replay2 = daemon
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    let matching2: Vec<_> = replay2
        .iter()
        .filter(|e| {
            matches!(&e.payload, CoreEvent::BackgroundJobCompleted { notice_id, .. } if notice_id == &notice.notice_id)
        })
        .collect();
    assert_eq!(
        matching2.len(),
        1,
        "still exactly one hint after re-reconciliation"
    );

    // And no turn was admitted by the healing path.
    assert!(
        replay2.iter().all(|e| !matches!(
            e.payload,
            CoreEvent::TurnStarted { .. }
                | CoreEvent::TurnCompleted { .. }
                | CoreEvent::TurnFailed { .. }
        )),
        "hint healing must never admit a provider turn"
    );
}

/// Cross-instance safety: a second daemon over the same pool observes
/// the first daemon's persisted hint (shared pool truth), heals nothing
/// new, and replays the identical notice — while the durable notice row
/// stays single and no turn is admitted anywhere.
#[tokio::test(flavor = "current_thread")]
async fn second_daemon_over_same_pool_never_duplicates_hints_or_turns() {
    let pool = common::pool::isolated_pool().await;
    let session = "sess-c004-two-daemons";

    let daemon_a = CoreDaemon::new(Some(pool.clone()), None, None);
    let notifier_a = daemon_a
        .deps
        .job_notification_service
        .clone()
        .expect("daemon A notifier");
    let mut rx_a = daemon_a.event_log.subscribe();
    notifier_a
        .subscribe(
            &JobId::new_unchecked("job-c004-two-1"),
            "test",
            session,
            None,
            "inv-c004-two-1",
        )
        .await
        .expect("subscribe");
    let notice = notifier_a
        .record_terminal(
            &job_record("job-c004-two-1", Some(session), JobState::Completed),
            "two-daemon proof",
            None,
        )
        .await
        .expect("record")
        .expect("notice");
    wait_for_background_event(&mut rx_a, &notice.notice_id, Duration::from_secs(10)).await;
    assert!(
        daemon_a
            .event_log
            .has_persisted_background_completion(&notice.notice_id)
            .await,
        "first daemon's hint is pool-visible truth"
    );

    // Second daemon instance over the same pool (stale-flock shape):
    // recovery heals nothing, replay shows the identical hint once.
    let daemon_b = CoreDaemon::new(Some(pool.clone()), None, None);
    assert!(
        daemon_b
            .event_log
            .has_persisted_background_completion(&notice.notice_id)
            .await,
        "pool truth is visible to the second instance"
    );
    let healed = daemon_b.reconcile_idle_completion_hints().await;
    assert_eq!(healed, 0, "no missing hint, nothing republished");
    let replay_b = daemon_b
        .event_log
        .replay_from(0, &owner_filter(session))
        .await;
    let matching: Vec<_> = replay_b
        .iter()
        .filter_map(|e| match &e.payload {
            CoreEvent::BackgroundJobCompleted {
                notice_id, status, ..
            } if notice_id == &notice.notice_id => Some(status.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        matching,
        vec!["completed".to_string()],
        "second instance replays the identical hint exactly once"
    );
    assert!(
        replay_b.iter().all(|e| !matches!(
            e.payload,
            CoreEvent::TurnStarted { .. }
                | CoreEvent::TurnCompleted { .. }
                | CoreEvent::TurnFailed { .. }
        )),
        "cross-instance replay must never admit a provider turn"
    );
}
