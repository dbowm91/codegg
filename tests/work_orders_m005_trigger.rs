//! Project Work Orders M005 — external task-trigger capability and endpoint.
//!
//! Closure qualification for the plan: verifier-only trigger records,
//! one-time secret display, authorized create/list-metadata/revoke,
//! narrow idempotent fire with replay/race/restart safety, repeat
//! re-arm semantics, privacy-safe failures, trigger-bearer isolation
//! from principal authority, and secret-free audit/events/errors.
//!
//! The HTTP fire-route section is gated on the `server` feature and
//! speaks raw HTTP over loopback TCP so the real routing, method,
//! header, body-limit, and rate-limit behavior is exercised without
//! extra test-only client dependencies.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreEvent, CoreRequest, CoreResponse};
use codegg::protocol::work_order::TaskTriggerCreateRequest;
use codegg_core::identity::{PrincipalId, ProjectId, TaskTriggerId};
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};
use codegg_core::work_order::{
    is_task_trigger_presentation, split_presented_trigger, GateKind, GateSpec, NewTaskTrigger,
    NewWorkOrder, OccurrenceState, ReleaseGateSet, TaskTriggerStatus, WorkOrder, WorkOrderError,
    WorkOrderService,
};

const TRIGGER_REF: &str = "hook-1";
const BASE_NOW: i64 = 1_700_000_000_000;

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn project() -> ProjectId {
    ProjectId::parse("project-1").unwrap()
}

fn creator() -> PrincipalId {
    PrincipalId::parse("local-owner").unwrap()
}

fn external_gate_set() -> ReleaseGateSet {
    ReleaseGateSet {
        join: codegg_core::work_order::GateJoin::All,
        gates: vec![GateSpec {
            kind: GateKind::ExternalTrigger,
            delay_secs: None,
            not_before_ms: None,
            lane_id: None,
            trigger_ref: Some(TRIGGER_REF.to_owned()),
        }],
    }
}

fn gated_input(prompt: &str) -> NewWorkOrder {
    NewWorkOrder {
        title: Some("Triggered task".to_owned()),
        prompt: prompt.to_owned(),
        requested_model: None,
        requested_approval: None,
        requested_sandbox: None,
        workspace_policy: None,
        gates: external_gate_set(),
        repeat_count: 1,
        sequence_lane_id: None,
        parent_session_id: None,
        parent_turn_id: None,
        parent_work_order_id: None,
        idempotency_key: None,
    }
}

async fn gated_work_order(service: &WorkOrderService, now_ms: i64) -> WorkOrder {
    let outcome = service
        .create_work_order(
            &project(),
            &creator(),
            gated_input("do the gated thing"),
            now_ms,
        )
        .await
        .expect("create gated work order");
    service
        .create_occurrence(&project(), &outcome.work_order.id, None, now_ms)
        .await
        .expect("create occurrence");
    outcome.work_order
}

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

async fn create_keyed_trigger(
    service: &WorkOrderService,
    work_order: &WorkOrder,
    key: &str,
) -> String {
    service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: Some(key.to_owned()),
            },
            BASE_NOW,
        )
        .await
        .expect("create")
        .plaintext_secret
        .unwrap()
}

// ── Crypto / record ───────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn trigger_secret_is_verifier_only_at_rest() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool.clone()));
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let outcome = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect("create trigger");
    assert!(!outcome.duplicate);
    let plaintext = outcome.plaintext_secret.expect("secret returned once");
    let (locator, secret) = split_presented_trigger(&plaintext).expect("parseable bearer");
    assert_eq!(locator, outcome.trigger.id.as_str());
    assert!(is_task_trigger_presentation(&plaintext));

    // The stored row carries a verifier, never the secret.
    let (verifier,): (String,) =
        sqlx::query_as("SELECT secret_verifier FROM task_trigger WHERE trigger_id = ?")
            .bind(locator.clone())
            .fetch_one(&pool)
            .await
            .expect("read verifier");
    assert_eq!(verifier.len(), 64);
    assert_ne!(verifier, secret);
    assert!(!verifier.contains(&secret));
    // No table holds the plaintext: the trigger rows serialize without
    // any secret segment.
    let dump: Vec<(String, String, String)> =
        sqlx::query_as("SELECT trigger_id, secret_verifier, verifier_version FROM task_trigger")
            .fetch_all(&pool)
            .await
            .expect("dump trigger rows");
    let dump_json = serde_json::to_string(&dump).expect("serialize");
    assert!(!dump_json.contains(&secret));
    assert!(!dump_json.contains(&plaintext));
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_create_validates_gate_binding() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    // No external gate at all: creation fails closed.
    let immediate = service
        .create_work_order(
            &project(),
            &creator(),
            NewWorkOrder {
                gates: ReleaseGateSet::immediate(),
                ..gated_input("immediate")
            },
            BASE_NOW,
        )
        .await
        .expect("create immediate work order");
    let error = service
        .create_task_trigger(
            &project(),
            &immediate.work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect_err("trigger without gate must fail");
    assert!(matches!(error, WorkOrderError::Invalid { .. }));

    // Unknown trigger reference: fails closed.
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let error = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: Some("someone-else-hook".to_owned()),
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect_err("unbound reference must fail");
    assert!(matches!(error, WorkOrderError::Invalid { .. }));

    // Terminal work order: no new triggers.
    service
        .transition_work_order(
            &project(),
            &work_order.id,
            codegg_core::work_order::WorkOrderState::Cancelled,
            BASE_NOW,
        )
        .await
        .expect("cancel work order");
    let error = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect_err("terminal work order must fail");
    assert!(matches!(error, WorkOrderError::Invalid { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_create_secret_shown_once_with_keyed_convergence() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let input = NewTaskTrigger {
        trigger_ref: None,
        expires_at_ms: None,
        max_fires: Some(3),
        idempotency_key: Some("create-key-1".to_owned()),
    };
    let first = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            input.clone(),
            BASE_NOW,
        )
        .await
        .expect("create");
    assert!(first.plaintext_secret.is_some());
    assert!(!first.duplicate);
    // Same key retries converge without re-issuing the secret.
    let second = service
        .create_task_trigger(&project(), &work_order.id, &creator(), input, BASE_NOW + 1)
        .await
        .expect("retry converges");
    assert!(second.duplicate);
    assert!(second.plaintext_secret.is_none());
    assert_eq!(second.trigger.id, first.trigger.id);
    // Same key with a different binding conflicts explicitly.
    let conflict = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: Some(9),
                idempotency_key: Some("create-key-1".to_owned()),
            },
            BASE_NOW + 2,
        )
        .await
        .expect_err("conflicting reuse must fail");
    assert!(matches!(conflict, WorkOrderError::IdempotencyConflict(_)));
}

// ── Fire / idempotency / races ────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn trigger_fire_latches_once_with_stable_keyed_receipts() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let created = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect("create");
    let bearer = created.plaintext_secret.unwrap();

    let first = service
        .fire_task_trigger(&bearer, Some("fire-key-1"), BASE_NOW + 10)
        .await
        .expect("first fire accepted");
    assert!(first.latched);
    assert!(!first.duplicate);
    let occurrences = service
        .list_occurrences(&project(), &work_order.id, None)
        .await
        .expect("list occurrences");
    assert_eq!(occurrences.occurrences.len(), 1);
    assert!(
        occurrences.occurrences[0]
            .gate_latches
            .contains(&GateKind::ExternalTrigger),
        "fire must latch the bound gate"
    );

    // Same key returns the same receipt (stable retry semantics: one
    // key means one result, including the latched flag).
    let replay = service
        .fire_task_trigger(&bearer, Some("fire-key-1"), BASE_NOW + 11)
        .await
        .expect("keyed replay converges");
    assert!(replay.latched);
    assert!(replay.duplicate);
    assert_eq!(replay.receipt_id, first.receipt_id);

    // Keyless duplicate delivery is inert for the latched occurrence and
    // consumes no additional fire budget.
    let dup = service
        .fire_task_trigger(&bearer, None, BASE_NOW + 12)
        .await
        .expect("duplicate delivery is harmless");
    assert!(!dup.latched);
    let trigger = service
        .get_task_trigger(&project(), &created.trigger.id)
        .await
        .expect("get trigger")
        .expect("trigger present");
    assert_eq!(trigger.fire_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trigger_concurrent_fires_latch_exactly_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("concurrent.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", path.display()))
        .await
        .expect("file pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    let service = WorkOrderService::with_defaults(Some(pool));
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let created = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect("create");
    let bearer = created.plaintext_secret.unwrap();

    let mut handles = Vec::new();
    for i in 0..16u32 {
        let service = service.clone();
        let bearer = bearer.clone();
        handles.push(tokio::spawn(async move {
            service
                .fire_task_trigger(&bearer, Some(&format!("race-key-{i}")), BASE_NOW + 10)
                .await
                .expect("concurrent fire resolves")
        }));
    }
    let mut latched = 0;
    for handle in handles {
        if handle.await.expect("join").latched {
            latched += 1;
        }
    }
    assert_eq!(latched, 1, "exactly one concurrent fire latches");
    let trigger = service
        .get_task_trigger(&project(), &created.trigger.id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(trigger.fire_count, 1, "fire budget consumed exactly once");
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_revoke_expire_exhaust_share_one_privacy_shape() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let expected = "task trigger not found or inactive";

    // Unknown locator.
    let unknown = service
        .fire_task_trigger(
            "cggtr_does-not-exist.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            None,
            BASE_NOW,
        )
        .await
        .expect_err("unknown trigger fails");
    assert_eq!(
        unknown.to_string(),
        format!("work order not found: {expected}")
    );

    // Wrong secret on a real locator.
    let bearer = create_keyed_trigger(&service, &work_order, "k-revoke").await;
    let (locator, _) = split_presented_trigger(&bearer).unwrap();
    let wrong = format!("cggtr_{locator}.BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
    let rejected = service
        .fire_task_trigger(&wrong, None, BASE_NOW)
        .await
        .expect_err("wrong secret fails");
    assert_eq!(rejected.to_string(), unknown.to_string());

    // Revoked trigger shares the shape.
    let bearer = create_keyed_trigger(&service, &work_order, "k-revoked").await;
    let (locator, _) = split_presented_trigger(&bearer).unwrap();
    let trigger_id = TaskTriggerId::parse(&locator).unwrap();
    let revoked = service
        .revoke_task_trigger(&project(), &trigger_id, BASE_NOW + 1)
        .await
        .expect("revoke");
    assert_eq!(revoked.status_at(BASE_NOW + 1), TaskTriggerStatus::Revoked);
    // Revocation is monotonic: a second revoke stays revoked.
    let again = service
        .revoke_task_trigger(&project(), &trigger_id, BASE_NOW + 2)
        .await
        .expect("revoke is idempotent");
    assert_eq!(again.status_at(BASE_NOW + 2), TaskTriggerStatus::Revoked);
    let after_revoke = service
        .fire_task_trigger(&bearer, None, BASE_NOW + 3)
        .await
        .expect_err("revoked trigger fails closed");
    assert_eq!(after_revoke.to_string(), unknown.to_string());

    // Expired trigger shares the shape.
    let expiring = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: Some(BASE_NOW + 100),
                max_fires: None,
                idempotency_key: Some("k-expiry".to_owned()),
            },
            BASE_NOW,
        )
        .await
        .expect("create expiring")
        .plaintext_secret
        .unwrap();
    let expired = service
        .fire_task_trigger(&expiring, None, BASE_NOW + 100)
        .await
        .expect_err("expired trigger fails closed");
    assert_eq!(expired.to_string(), unknown.to_string());

    // None of the failures reveal locator, project, secret, or verifier.
    for error in [&unknown, &rejected, &after_revoke, &expired] {
        let text = error.to_string();
        assert!(!text.contains(&locator), "error leaks locator: {text}");
        assert!(!text.contains("project-1"), "error leaks project: {text}");
        assert!(!text.contains("cggtr_"), "error leaks bearer: {text}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_fire_while_running_is_inert_with_repeat_rearm() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    let mut input = gated_input("repeatable gated work");
    input.repeat_count = 2;
    let outcome = service
        .create_work_order(&project(), &creator(), input, BASE_NOW)
        .await
        .expect("create repeatable work order");
    let work_order = outcome.work_order;
    let first_occurrence = service
        .create_occurrence(&project(), &work_order.id, None, BASE_NOW)
        .await
        .expect("create first occurrence");
    let created = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect("create")
        .plaintext_secret
        .unwrap();

    // Latch the first occurrence, then move it to running: a racing fire
    // is inert and never pre-latches the not-yet-created repeat.
    let fired = service
        .fire_task_trigger(&created, None, BASE_NOW + 1)
        .await
        .expect("fire first");
    assert!(fired.latched);
    service
        .claim_occurrence(&project(), &first_occurrence.id, BASE_NOW + 2)
        .await
        .expect("claim");
    let racing = service
        .fire_task_trigger(&created, None, BASE_NOW + 3)
        .await
        .expect("racing fire is inert");
    assert!(!racing.latched);
    let trigger = service
        .list_task_triggers(&project(), Some(&work_order.id), None)
        .await
        .expect("list");
    assert_eq!(trigger.triggers[0].fire_count, 1);

    // Complete the first occurrence, re-arm the repeat, and prove the
    // same trigger fires the next occurrence exactly once.
    for (state, code) in [
        (
            OccurrenceState::Running,
            None::<codegg_core::work_order::AttentionCode>,
        ),
        (OccurrenceState::Completed, None),
    ] {
        service
            .transition_occurrence(
                &project(),
                &first_occurrence.id,
                state,
                code,
                None,
                BASE_NOW + 4,
            )
            .await
            .expect("advance first occurrence");
    }
    let repeat = service
        .create_next_repeat_occurrence(
            &project(),
            &work_order.id,
            &first_occurrence.id,
            BASE_NOW + 5,
        )
        .await
        .expect("re-arm repeat");
    assert!(!repeat.duplicate);
    assert!(
        !repeat
            .occurrence
            .gate_latches
            .contains(&GateKind::ExternalTrigger),
        "re-armed occurrence starts unlatched"
    );
    let second = service
        .fire_task_trigger(&created, None, BASE_NOW + 6)
        .await
        .expect("fire re-armed occurrence");
    assert!(second.latched);
    let trigger = service
        .list_task_triggers(&project(), Some(&work_order.id), None)
        .await
        .expect("list");
    assert_eq!(trigger.triggers[0].fire_count, 2);
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_max_fires_exact_with_exhausted_rejection() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    let mut input = gated_input("bounded gated work");
    input.repeat_count = 2;
    let outcome = service
        .create_work_order(&project(), &creator(), input, BASE_NOW)
        .await
        .expect("create");
    let work_order = outcome.work_order;
    let first = service
        .create_occurrence(&project(), &work_order.id, None, BASE_NOW)
        .await
        .expect("first occurrence");
    let bearer = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: Some(1),
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect("create")
        .plaintext_secret
        .unwrap();
    assert!(
        service
            .fire_task_trigger(&bearer, None, BASE_NOW + 1)
            .await
            .expect("first fire")
            .latched
    );
    // Re-arm: the trigger is exhausted, so the next occurrence cannot
    // fire even though its gate is unlatched.
    service
        .claim_occurrence(&project(), &first.id, BASE_NOW + 2)
        .await
        .expect("claim first");
    for state in [OccurrenceState::Running, OccurrenceState::Completed] {
        service
            .transition_occurrence(&project(), &first.id, state, None, None, BASE_NOW + 2)
            .await
            .expect("complete first");
    }
    service
        .create_next_repeat_occurrence(&project(), &work_order.id, &first.id, BASE_NOW + 3)
        .await
        .expect("re-arm");
    let exhausted = service
        .fire_task_trigger(&bearer, None, BASE_NOW + 4)
        .await
        .expect_err("exhausted trigger fails closed");
    assert!(matches!(exhausted, WorkOrderError::NotFound(_)));
    let trigger = service
        .list_task_triggers(&project(), Some(&work_order.id), None)
        .await
        .expect("list");
    assert_eq!(
        trigger.triggers[0].status_at(BASE_NOW + 4),
        TaskTriggerStatus::Exhausted
    );
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_fire_on_cancelled_work_order_is_inert() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    let work_order = gated_work_order(&service, BASE_NOW).await;
    let bearer = service
        .create_task_trigger(
            &project(),
            &work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
            BASE_NOW,
        )
        .await
        .expect("create")
        .plaintext_secret
        .unwrap();
    service
        .transition_work_order(
            &project(),
            &work_order.id,
            codegg_core::work_order::WorkOrderState::Cancelled,
            BASE_NOW + 1,
        )
        .await
        .expect("cancel work order");
    // Cancellation makes future fire inert without revealing details:
    // success-shaped, never latched, no budget consumed, and the
    // cancellation itself is not reactivated.
    let outcome = service
        .fire_task_trigger(&bearer, None, BASE_NOW + 2)
        .await
        .expect("cancelled fire is inert");
    assert!(!outcome.latched);
    let trigger = service
        .get_task_trigger(
            &project(),
            &TaskTriggerId::parse(&split_presented_trigger(&bearer).unwrap().0).unwrap(),
        )
        .await
        .expect("get")
        .expect("present");
    assert_eq!(trigger.fire_count, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_state_and_receipts_survive_restart() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("restart.db");
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let locator: String;
    let bearer: String;
    let receipt: String;
    let work_order_id: codegg_core::identity::WorkOrderId;
    {
        let pool = sqlx::SqlitePool::connect(&url).await.expect("open");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let service = WorkOrderService::with_defaults(Some(pool.clone()));
        let work_order = gated_work_order(&service, BASE_NOW).await;
        work_order_id = work_order.id.clone();
        let created = service
            .create_task_trigger(
                &project(),
                &work_order.id,
                &creator(),
                NewTaskTrigger {
                    trigger_ref: None,
                    expires_at_ms: None,
                    max_fires: Some(5),
                    idempotency_key: Some("restart-create".to_owned()),
                },
                BASE_NOW,
            )
            .await
            .expect("create");
        locator = created.trigger.id.as_str().to_owned();
        bearer = created.plaintext_secret.clone().unwrap();
        let outcome = service
            .fire_task_trigger(
                created.plaintext_secret.as_deref().unwrap(),
                Some("restart-fire"),
                BASE_NOW + 1,
            )
            .await
            .expect("fire");
        assert!(outcome.latched);
        receipt = outcome.receipt_id;
        // Revoke a second trigger before restart to pin revocation.
        let second = service
            .create_task_trigger(
                &project(),
                &work_order.id,
                &creator(),
                NewTaskTrigger {
                    trigger_ref: None,
                    expires_at_ms: None,
                    max_fires: None,
                    idempotency_key: Some("restart-create-2".to_owned()),
                },
                BASE_NOW,
            )
            .await
            .expect("create second");
        service
            .revoke_task_trigger(&project(), &second.trigger.id, BASE_NOW + 2)
            .await
            .expect("revoke second");
        pool.close().await;
    }
    {
        let pool = sqlx::SqlitePool::connect(&url).await.expect("reopen");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("remigrate");
        let service = WorkOrderService::with_defaults(Some(pool));
        let trigger = service
            .get_task_trigger(&project(), &TaskTriggerId::parse(&locator).unwrap())
            .await
            .expect("get")
            .expect("survives restart");
        assert_eq!(trigger.fire_count, 1);
        assert_eq!(trigger.max_fires, Some(5));
        assert_eq!(trigger.status_at(BASE_NOW + 10), TaskTriggerStatus::Active);
        // The keyed receipt converges after restart.
        let page = service
            .list_task_triggers(&project(), None, None)
            .await
            .expect("list");
        assert_eq!(page.triggers.len(), 2);
        assert!(
            page.triggers
                .iter()
                .any(|row| row.status_at(BASE_NOW + 10) == TaskTriggerStatus::Revoked),
            "revocation survives restart"
        );
        let occurrences = service
            .list_occurrences(&project(), &work_order_id, None)
            .await
            .expect("list occurrences");
        assert!(
            occurrences
                .occurrences
                .iter()
                .any(|occurrence| occurrence.gate_latches.contains(&GateKind::ExternalTrigger)),
            "gate latch survives restart"
        );
        // The keyed receipt converges after restart: same key returns
        // the stored receipt instead of re-firing.
        let replay = service
            .fire_task_trigger(&bearer, Some("restart-fire"), BASE_NOW + 11)
            .await
            .expect("keyed replay after restart");
        assert!(replay.latched);
        assert!(replay.duplicate);
        assert_eq!(replay.receipt_id, receipt);
    }
}

// ── Daemon management boundary ────────────────────────────────────────

async fn catalog_project(pool: &sqlx::SqlitePool, name: &str) -> ProjectId {
    use codegg_core::workspace::{
        SqliteWorkspaceStore, WorkspaceId, WorkspaceRecord, WorkspaceStore,
    };
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let now = chrono::Utc::now();
    let workspace = WorkspaceRecord {
        id: WorkspaceId::new(),
        canonical_root: std::path::PathBuf::from(format!("/tmp/trigger-{n}")),
        display_name: format!("Trigger test workspace {n}"),
        created_at: now,
        last_opened_at: now,
        archived_at: None,
    };
    let store = SqliteWorkspaceStore::new(pool.clone());
    WorkspaceStore::upsert(&store, &workspace)
        .await
        .expect("test workspace registration");
    let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
    catalog
        .register_local_project(
            codegg_core::project_catalog::RegisterLocalProject {
                display_name: name.to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "trigger-test",
        )
        .await
        .expect("test project registration")
        .project_id
}

async fn human_token(team: &TeamStore, name: &str, client_id: &str) -> AuthenticatedPrincipal {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .unwrap();
    tokens
        .verify_for_client(&plaintext, client_id)
        .await
        .unwrap()
}

async fn member_in_project(
    team: &TeamStore,
    project: &ProjectId,
    name: &str,
    client_id: &str,
    role: ProjectRole,
) -> AuthenticatedPrincipal {
    let principal = human_token(team, name, client_id).await;
    let record = team
        .get_principal(principal.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(project, &record.id, role)
        .await
        .unwrap();
    principal
}

fn register_client(daemon: &CoreDaemon, client_id: &str, principal: AuthenticatedPrincipal) {
    daemon.clients.register_with_principal(
        client_id.to_owned(),
        format!("{client_id}-name"),
        None,
        principal,
    );
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{}-{}", client, uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

fn trigger_secret(response: &CoreResponse) -> Option<String> {
    match response {
        CoreResponse::WorkOrderTrigger { secret, .. } => secret.clone(),
        other => panic!("expected trigger response, got {other:?}"),
    }
}

fn trigger_metadata_dto(
    response: &CoreResponse,
) -> codegg::protocol::work_order::TaskTriggerMetadataDto {
    match response {
        CoreResponse::WorkOrderTrigger { trigger, .. } => trigger.clone(),
        other => panic!("expected trigger response, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_management_authorization_matrix() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = catalog_project(&pool, "Triggerland").await;

    // Owner-equivalent local client creates the gated work order, then a
    // contributor creates a trigger and receives its secret once.
    let created_wo = match call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkOrderCreate {
            request: codegg::protocol::work_order::WorkOrderCreateRequest {
                project_id: project.as_str().to_owned(),
                title: Some("Gated".to_owned()),
                prompt: "gated prompt".to_owned(),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: vec![codegg::protocol::work_order::WorkOrderGateDto {
                    kind: "external_trigger".to_owned(),
                    delay_secs: None,
                    not_before_ms: None,
                    lane_id: None,
                    trigger_ref: Some(TRIGGER_REF.to_owned()),
                }],
                gate_join: Some("all".to_owned()),
                repeat_count: Some(1),
                sequence_lane_id: None,
                parent_session_id: None,
                parent_turn_id: None,
                parent_work_order_id: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => work_order,
        other => panic!("expected work order, got {other:?}"),
    };

    let contributor = member_in_project(
        &team,
        &project,
        "Connie",
        "client-connie",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-connie", contributor);
    let viewer =
        member_in_project(&team, &project, "Vera", "client-vera", ProjectRole::Viewer).await;
    register_client(&daemon, "client-vera", viewer);
    let outsider = human_token(&team, "Ollie", "client-ollie").await;
    register_client(&daemon, "client-ollie", outsider);

    let create = || TaskTriggerCreateRequest {
        project_id: project.as_str().to_owned(),
        work_order_id: created_wo.work_order_id.clone(),
        trigger_ref: None,
        expires_at_ms: None,
        max_fires: None,
        idempotency_key: None,
    };
    let created = call(
        &daemon,
        "client-connie",
        CoreRequest::WorkOrderTriggerCreate { request: create() },
    )
    .await;
    let bearer = trigger_secret(&created).expect("contributor receives secret once");
    let metadata = trigger_metadata_dto(&created);
    assert_eq!(metadata.project_id, project.as_str());
    // The creation response JSON carries the secret exactly once by
    // construction; every later surface must not.
    let created_json = serde_json::to_string(&created).expect("serialize");
    assert!(created_json.contains(&bearer));

    // Viewer may read metadata but never receives or mints secrets.
    let listed = call(
        &daemon,
        "client-vera",
        CoreRequest::WorkOrderTriggerList {
            request: codegg::protocol::work_order::TaskTriggerListRequest {
                project_id: project.as_str().to_owned(),
                work_order_id: None,
                limit: None,
            },
        },
    )
    .await;
    let listed_json = serde_json::to_string(&listed).expect("serialize");
    assert!(!listed_json.contains(&bearer), "list leaks secret");
    assert!(!listed_json.contains("verifier"), "list leaks verifier");
    match &listed {
        CoreResponse::WorkOrderTriggerList { triggers, .. } => assert_eq!(triggers.len(), 1),
        other => panic!("expected trigger list, got {other:?}"),
    }
    let fetched = call(
        &daemon,
        "client-vera",
        CoreRequest::WorkOrderTriggerGet {
            trigger_id: metadata.trigger_id.clone(),
        },
    )
    .await;
    assert!(
        trigger_secret(&fetched).is_none(),
        "get re-issues no secret"
    );
    let fetched_json = serde_json::to_string(&fetched).expect("serialize");
    assert!(!fetched_json.contains(&bearer));

    // Viewer cannot create or revoke (modify authority required).
    // Like every other work-order operation, denials use the
    // privacy-preserving not-found shape rather than an explicit
    // authorization code, so denied callers cannot distinguish absence
    // from lack of authority.
    for (who, request) in [
        (
            "client-vera",
            CoreRequest::WorkOrderTriggerCreate { request: create() },
        ),
        (
            "client-vera",
            CoreRequest::WorkOrderTriggerRevoke {
                trigger_id: metadata.trigger_id.clone(),
            },
        ),
        (
            "client-ollie",
            CoreRequest::WorkOrderTriggerCreate { request: create() },
        ),
    ] {
        let denied = call(&daemon, who, request).await;
        assert_eq!(error_code(&denied), "project_not_found");
        let denied_json = serde_json::to_string(&denied).expect("serialize");
        assert!(
            !denied_json.contains(&metadata.trigger_id),
            "denial leaks trigger locator"
        );
        assert!(!denied_json.contains(&bearer), "denial leaks secret");
    }
    let denied_list = call(
        &daemon,
        "client-ollie",
        CoreRequest::WorkOrderTriggerList {
            request: codegg::protocol::work_order::TaskTriggerListRequest {
                project_id: project.as_str().to_owned(),
                work_order_id: None,
                limit: None,
            },
        },
    )
    .await;
    assert_eq!(error_code(&denied_list), "project_not_found");
    assert!(
        !serde_json::to_string(&denied_list)
            .expect("serialize")
            .contains(&metadata.trigger_id),
        "denial leaks trigger locator"
    );

    // Contributor revokes; the trigger reports revoked metadata.
    let revoked = call(
        &daemon,
        "client-connie",
        CoreRequest::WorkOrderTriggerRevoke {
            trigger_id: metadata.trigger_id.clone(),
        },
    )
    .await;
    assert_eq!(trigger_metadata_dto(&revoked).status, "revoked");
    assert!(trigger_secret(&revoked).is_none());

    // A revoked trigger no longer fires (service-level privacy shape).
    let service = WorkOrderService::with_defaults(Some(pool.clone()));
    let after = service
        .fire_task_trigger(&bearer, None, BASE_NOW + 50_000)
        .await
        .expect_err("revoked bearer fails");
    assert!(matches!(after, WorkOrderError::NotFound(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_fire_reaches_coordinator_exactly_once() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let project = catalog_project(&pool, "Fireland").await;

    // Gated work order with one waiting occurrence behind the trigger.
    let created_wo = match call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkOrderCreate {
            request: codegg::protocol::work_order::WorkOrderCreateRequest {
                project_id: project.as_str().to_owned(),
                title: Some("Fire me".to_owned()),
                prompt: "fire prompt".to_owned(),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: Some("serialized".to_owned()),
                gates: vec![codegg::protocol::work_order::WorkOrderGateDto {
                    kind: "external_trigger".to_owned(),
                    delay_secs: None,
                    not_before_ms: None,
                    lane_id: None,
                    trigger_ref: Some(TRIGGER_REF.to_owned()),
                }],
                gate_join: Some("all".to_owned()),
                repeat_count: Some(1),
                sequence_lane_id: None,
                parent_session_id: None,
                parent_turn_id: None,
                parent_work_order_id: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => work_order,
        other => panic!("expected work order, got {other:?}"),
    };
    let service = WorkOrderService::with_defaults(Some(pool.clone()));
    let work_order_id =
        codegg_core::identity::WorkOrderId::parse(&created_wo.work_order_id).unwrap();
    let occurrence = service
        .create_occurrence(&project, &work_order_id, None, BASE_NOW)
        .await
        .expect("create occurrence");

    let created = call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkOrderTriggerCreate {
            request: TaskTriggerCreateRequest {
                project_id: project.as_str().to_owned(),
                work_order_id: created_wo.work_order_id.clone(),
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
        },
    )
    .await;
    let bearer = trigger_secret(&created).expect("secret once");

    // Observe the structural event stream: fire must publish identity-
    // only events with no secret content.
    let mut events = daemon.event_log.subscribe();
    let outcome = daemon
        .fire_work_order_trigger(&bearer, None, BASE_NOW + 5)
        .await
        .expect("fire accepted");
    assert!(outcome.latched);
    let stored = service
        .get_occurrence(&project, &occurrence.id)
        .await
        .expect("get occurrence")
        .expect("present");
    assert!(stored.gate_latches.contains(&GateKind::ExternalTrigger));

    // The committed latch reaches the coordinator through the fire
    // path's own wake: the occurrence is already running with exactly
    // one canonical session and one initial job linked.
    let materialized = service
        .get_occurrence(&project, &occurrence.id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(
        materialized.state,
        codegg_core::work_order::OccurrenceState::Running
    );
    assert!(
        materialized.session_id.is_some(),
        "materialization links one canonical session"
    );
    assert!(
        materialized.job_id.is_some(),
        "materialization submits one initial job"
    );

    // An explicit wake converges with no further work.
    let advanced = daemon
        .wake_work_orders_for_project(&project, BASE_NOW + 6)
        .await
        .expect("wake converges");
    assert_eq!(advanced, 0, "wake converges with no duplicate work");

    // Duplicate fire plus wake cannot duplicate the session or job.
    let replay = daemon
        .fire_work_order_trigger(&bearer, None, BASE_NOW + 7)
        .await
        .expect("replay resolves");
    assert!(!replay.latched);
    let advanced_again = daemon
        .wake_work_orders_for_project(&project, BASE_NOW + 8)
        .await
        .expect("second wake converges");
    assert_eq!(advanced_again, 0);
    let settled = service
        .get_occurrence(&project, &occurrence.id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(settled.session_id, materialized.session_id);
    assert_eq!(settled.job_id, materialized.job_id);

    // Drain the observed events: trigger lifecycle is visible without
    // secrets, and the occurrence change is published.
    let mut saw_trigger_fired = false;
    let mut saw_occurrence = false;
    while let Ok(envelope) = events.try_recv() {
        let json = serde_json::to_string(&envelope.payload).expect("event serializes");
        assert!(!json.contains(&bearer), "event leaks bearer");
        assert!(!json.contains("verifier"), "event leaks verifier");
        match &envelope.payload {
            CoreEvent::WorkOrderTriggerChanged { change, .. } if change == "fired" => {
                saw_trigger_fired = true;
            }
            CoreEvent::WorkOrderOccurrenceChanged { .. } => {
                saw_occurrence = true;
            }
            _ => {}
        }
    }
    assert!(saw_trigger_fired, "fire publishes trigger event");
    assert!(saw_occurrence, "materialization publishes occurrence event");
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_audit_rows_carry_no_secret() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let project = catalog_project(&pool, "Auditland").await;
    let created_wo = match call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkOrderCreate {
            request: codegg::protocol::work_order::WorkOrderCreateRequest {
                project_id: project.as_str().to_owned(),
                title: Some("Audited".to_owned()),
                prompt: "audit prompt".to_owned(),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: vec![codegg::protocol::work_order::WorkOrderGateDto {
                    kind: "external_trigger".to_owned(),
                    delay_secs: None,
                    not_before_ms: None,
                    lane_id: None,
                    trigger_ref: Some(TRIGGER_REF.to_owned()),
                }],
                gate_join: Some("all".to_owned()),
                repeat_count: Some(1),
                sequence_lane_id: None,
                parent_session_id: None,
                parent_turn_id: None,
                parent_work_order_id: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => work_order,
        other => panic!("expected work order, got {other:?}"),
    };
    let created = call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkOrderTriggerCreate {
            request: TaskTriggerCreateRequest {
                project_id: project.as_str().to_owned(),
                work_order_id: created_wo.work_order_id.clone(),
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
        },
    )
    .await;
    let bearer = trigger_secret(&created).expect("secret once");
    let metadata = trigger_metadata_dto(&created);
    call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkOrderTriggerRevoke {
            trigger_id: metadata.trigger_id.clone(),
        },
    )
    .await;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, metadata_json FROM audit_event WHERE metadata_json LIKE '%task_trigger%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    assert!(
        rows.len() >= 2,
        "create + revoke audit rows present (found {})",
        rows.len()
    );
    for (_, metadata_json) in &rows {
        assert!(
            !metadata_json.contains(&bearer),
            "audit leaks bearer: {metadata_json}"
        );
        assert!(
            !metadata_json.contains("verifier"),
            "audit leaks verifier: {metadata_json}"
        );
    }
}

// ── HTTP fire route (server feature) ────────────────────────────────────
//
// Raw loopback HTTP against the real router: routing, methods, headers,
// body caps, rate limits, and privacy shapes exactly as served.

#[cfg(feature = "server")]
mod http_fire {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, Once};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use codegg::server::routes::task_trigger::task_trigger_router;
    use codegg::server::{ServerState, WsRateLimiter};

    struct HttpResponse {
        status: u16,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    impl HttpResponse {
        fn body_str(&self) -> String {
            String::from_utf8_lossy(&self.body).into_owned()
        }
    }

    async fn raw_request(port: u16, request: &[u8]) -> HttpResponse {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        stream.write_all(request).await.expect("write");
        stream.flush().await.expect("flush");
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let header_end: usize = loop {
            let n = tokio::time::timeout_at(deadline, stream.read(&mut tmp))
                .await
                .expect("read timeout")
                .expect("read");
            if n == 0 {
                panic!("connection closed before headers complete");
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
                break pos + 4;
            }
            if buf.len() > 1_000_000 {
                panic!("headers too large");
            }
        };
        let header_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
        let mut lines = header_text.lines();
        let status_line = lines.next().expect("status line");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .expect("status code")
            .parse()
            .expect("status parses");
        let mut headers = HashMap::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_lowercase(), value.trim().to_owned());
            }
        }
        let content_length: usize = headers
            .get("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        while buf.len() - header_end < content_length {
            let n = tokio::time::timeout_at(deadline, stream.read(&mut tmp))
                .await
                .expect("body timeout")
                .expect("read body");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = buf[header_end..].to_vec();
        HttpResponse {
            status,
            headers,
            body,
        }
    }

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    async fn read_one_response(
        stream: &mut tokio::net::TcpStream,
        buf: &mut Vec<u8>,
    ) -> HttpResponse {
        let mut tmp = [0u8; 4096];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let header_end: usize = loop {
            if buf.len() > 1_000_000 {
                panic!("headers too large");
            }
            if let Some(pos) = find_subsequence(buf, b"\r\n\r\n") {
                break pos + 4;
            }
            let n = tokio::time::timeout_at(deadline, stream.read(&mut tmp))
                .await
                .expect("read timeout")
                .expect("read");
            if n == 0 {
                panic!("connection closed before headers complete");
            }
            buf.extend_from_slice(&tmp[..n]);
        };
        let header_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
        let mut lines = header_text.lines();
        let status_line = lines.next().expect("status line");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .expect("status code")
            .parse()
            .expect("status parses");
        let mut headers = HashMap::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_lowercase(), value.trim().to_owned());
            }
        }
        let content_length: usize = headers
            .get("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        while buf.len() - header_end < content_length {
            let n = tokio::time::timeout_at(deadline, stream.read(&mut tmp))
                .await
                .expect("body timeout")
                .expect("read body");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = buf[header_end..header_end + content_length].to_vec();
        buf.drain(..header_end + content_length);
        HttpResponse {
            status,
            headers,
            body,
        }
    }

    /// Sequential keep-alive requests on ONE connection: the server's
    /// socket-keyed limiter observes one peer address, so the budget
    /// applies across the pipelined requests (fresh connections carry
    /// fresh ephemeral ports and therefore fresh budgets, by design).
    async fn raw_requests_keepalive(port: u16, requests: &[Vec<u8>]) -> Vec<HttpResponse> {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let mut buf = Vec::new();
        let mut out = Vec::with_capacity(requests.len());
        for request in requests {
            stream.write_all(request).await.expect("write");
            stream.flush().await.expect("flush");
            out.push(read_one_response(&mut stream, &mut buf).await);
        }
        out
    }

    struct Fixture {
        port: u16,
        bearer: String,
        trigger_id: String,
        daemon: Arc<CoreDaemon>,
        project: ProjectId,
    }

    async fn fixture(max_requests: usize) -> Fixture {
        let pool = test_pool().await;
        let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None));
        let project = catalog_project(&pool, "Httpland").await;
        let created_wo = match call(
            &daemon,
            "local-daemon",
            CoreRequest::WorkOrderCreate {
                request: codegg::protocol::work_order::WorkOrderCreateRequest {
                    project_id: project.as_str().to_owned(),
                    title: Some("HTTP gated".to_owned()),
                    prompt: "http gated prompt".to_owned(),
                    requested_model: None,
                    requested_approval: None,
                    requested_sandbox: None,
                    workspace_policy: Some("serialized".to_owned()),
                    gates: vec![codegg::protocol::work_order::WorkOrderGateDto {
                        kind: "external_trigger".to_owned(),
                        delay_secs: None,
                        not_before_ms: None,
                        lane_id: None,
                        trigger_ref: Some(TRIGGER_REF.to_owned()),
                    }],
                    gate_join: Some("all".to_owned()),
                    repeat_count: Some(1),
                    sequence_lane_id: None,
                    parent_session_id: None,
                    parent_turn_id: None,
                    parent_work_order_id: None,
                    idempotency_key: None,
                },
            },
        )
        .await
        {
            CoreResponse::WorkOrder { work_order, .. } => work_order,
            other => panic!("expected work order, got {other:?}"),
        };
        let service = WorkOrderService::with_defaults(Some(pool.clone()));
        let work_order_id =
            codegg_core::identity::WorkOrderId::parse(&created_wo.work_order_id).unwrap();
        service
            .create_occurrence(&project, &work_order_id, None, BASE_NOW)
            .await
            .expect("occurrence");
        let created = call(
            &daemon,
            "local-daemon",
            CoreRequest::WorkOrderTriggerCreate {
                request: TaskTriggerCreateRequest {
                    project_id: project.as_str().to_owned(),
                    work_order_id: created_wo.work_order_id.clone(),
                    trigger_ref: None,
                    expires_at_ms: None,
                    max_fires: None,
                    idempotency_key: None,
                },
            },
        )
        .await;
        let bearer = trigger_secret(&created).expect("secret once");
        let trigger_id = trigger_metadata_dto(&created).trigger_id;
        let state = ServerState {
            pool: pool.clone(),
            mcp_service: Arc::new(tokio::sync::RwLock::new(codegg::mcp::McpService::new())),
            config: codegg::config::schema::Config::default(),
            ws_rate_limiter: Arc::new(WsRateLimiter::new(100, 60)),
            daemon: Some(daemon.clone()),
            projection_lifecycle_seam: Default::default(),
            connection_task_probe: None,
            probe_factory: None,
            transport_test_config: None,
        };
        let router = task_trigger_router(state, max_requests, 60);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });
        Fixture {
            port,
            bearer,
            trigger_id,
            daemon,
            project,
        }
    }

    fn post_fire(trigger_id: &str, bearer: Option<&str>, extra: &str, body: &[u8]) -> Vec<u8> {
        let auth = bearer
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        format!(
            "POST /api/v1/task-triggers/{trigger_id}/fire{extra} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes()
        .into_iter()
        .chain(body.iter().copied())
        .collect()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_post_fire_accepted_then_replayed() {
        let fx = fixture(1000).await;
        let first = raw_request(
            fx.port,
            &post_fire(&fx.trigger_id, Some(&fx.bearer), "", b""),
        )
        .await;
        assert_eq!(first.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&first.body).expect("json");
        assert_eq!(body["status"], "accepted");
        assert!(body["receipt_id"].is_string());
        assert!(!first.body_str().contains(&fx.bearer));

        let second = raw_request(
            fx.port,
            &post_fire(&fx.trigger_id, Some(&fx.bearer), "", b""),
        )
        .await;
        assert_eq!(second.status, 200);
        let replay: serde_json::Value = serde_json::from_slice(&second.body).expect("json");
        assert_eq!(replay["status"], "already_fired");

        // Keyed retries converge on one receipt.
        let keyed = |key: &str| {
            format!(
                "POST /api/v1/task-triggers/{}/fire HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nIdempotency-Key: {key}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                fx.trigger_id, fx.bearer
            )
            .into_bytes()
        };
        let a = raw_request(fx.port, &keyed("stable-key")).await;
        let b = raw_request(fx.port, &keyed("stable-key")).await;
        assert_eq!(a.status, 200);
        assert_eq!(b.status, 200);
        let ra: serde_json::Value = serde_json::from_slice(&a.body).expect("json");
        let rb: serde_json::Value = serde_json::from_slice(&b.body).expect("json");
        assert_eq!(ra["receipt_id"], rb["receipt_id"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_get_has_no_side_effect() {
        let fx = fixture(1000).await;
        let request = format!(
            "GET /api/v1/task-triggers/{}/fire HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            fx.trigger_id, fx.bearer
        );
        let response = raw_request(fx.port, request.as_bytes()).await;
        assert_eq!(response.status, 405);
        // Zero side effect: the gate stays unlatched and no fire budget
        // is consumed.
        let service = WorkOrderService::with_defaults(fx.daemon.pool.clone());
        let page = service
            .list_task_triggers(&fx.project, None, None)
            .await
            .expect("list");
        assert_eq!(page.triggers.len(), 1);
        assert_eq!(page.triggers[0].fire_count, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_failures_share_one_privacy_shape() {
        let fx = fixture(1000).await;
        let (locator, _) = split_presented_trigger(&fx.bearer).unwrap();
        let wrong = format!("cggtr_{locator}.BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
        let unknown = "cggtr_does-not-exist.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let missing = raw_request(fx.port, &post_fire(&fx.trigger_id, None, "", b"")).await;
        let bad_secret =
            raw_request(fx.port, &post_fire(&fx.trigger_id, Some(&wrong), "", b"")).await;
        let no_such = raw_request(
            fx.port,
            &post_fire("does-not-exist", Some(unknown), "", b""),
        )
        .await;
        // Revoke, then fire: same shape as unknown/wrong.
        call(
            &fx.daemon,
            "local-daemon",
            CoreRequest::WorkOrderTriggerRevoke {
                trigger_id: fx.trigger_id.clone(),
            },
        )
        .await;
        let revoked = raw_request(
            fx.port,
            &post_fire(&fx.trigger_id, Some(&fx.bearer), "", b""),
        )
        .await;
        for response in [&missing, &bad_secret, &no_such, &revoked] {
            assert_eq!(response.status, 401, "body: {}", response.body_str());
            let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
            assert_eq!(body["code"], "trigger_invalid");
            assert_eq!(body["message"], "invalid or inactive trigger");
        }
        assert_eq!(missing.body, bad_secret.body);
        assert_eq!(bad_secret.body, no_such.body);
        assert_eq!(no_such.body, revoked.body);
        for response in [&missing, &bad_secret, &no_such, &revoked] {
            assert!(!response.body_str().contains(&fx.bearer));
            assert!(!response.body_str().contains(&locator));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_query_string_secret_is_ignored() {
        let fx = fixture(1000).await;
        // A secret smuggled in the URL without a header authenticates
        // nothing.
        let smuggled = format!(
            "POST /api/v1/task-triggers/{}/fire?token={} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            fx.trigger_id, fx.bearer
        );
        let denied = raw_request(fx.port, smuggled.as_bytes()).await;
        assert_eq!(denied.status, 401);
        // A wrong secret in the query is ignored when the header bearer
        // is valid: the URL is never consulted for credentials.
        let ignored = format!(
            "POST /api/v1/task-triggers/{}/fire?token=wrong HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            fx.trigger_id, fx.bearer
        );
        let accepted = raw_request(fx.port, ignored.as_bytes()).await;
        assert_eq!(accepted.status, 200);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_body_is_rejected_and_bounded() {
        let fx = fixture(1000).await;
        let small = raw_request(
            fx.port,
            &post_fire(
                &fx.trigger_id,
                Some(&fx.bearer),
                "",
                b"{\"prompt\":\"smuggled\"}",
            ),
        )
        .await;
        assert_eq!(small.status, 400);
        let big_body = vec![b'x'; 5000];
        let big = raw_request(
            fx.port,
            &post_fire(&fx.trigger_id, Some(&fx.bearer), "", &big_body),
        )
        .await;
        assert!(
            big.status == 400 || big.status == 413,
            "oversized body rejected, got {}",
            big.status
        );
        assert!(!big.body_str().contains(&fx.bearer));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_principal_bearers_never_fire() {
        let fx = fixture(1000).await;
        let team = TeamStore::new(fx.daemon.pool.clone().expect("pool"));
        let tokens = PersonalTokenStore::new(fx.daemon.pool.clone().expect("pool"));
        let record = team
            .create_principal(PrincipalKind::Human, "Netop")
            .await
            .unwrap();
        let (personal, _) = tokens
            .create_personal_token(&record.id, "device", None)
            .await
            .unwrap();
        // A principal token at the fire route is rejected without
        // minting any principal session through this endpoint.
        let attempt = raw_request(
            fx.port,
            &post_fire(&fx.trigger_id, Some(&personal), "", b""),
        )
        .await;
        assert_eq!(attempt.status, 401);
        // And the trigger bearer cannot authenticate as a normal
        // principal on Core APIs: it matches neither the personal-token
        // path nor the configured global bearer.
        let config = codegg::config::schema::Config::default();
        let pool = fx.daemon.pool.clone().expect("pool");
        let rejected = codegg::server::middleware::auth::resolve_bearer_principal(
            &fx.bearer,
            &config,
            &pool,
            "client-trigger",
        )
        .await;
        assert!(
            rejected.is_err(),
            "trigger bearer must not bind a principal"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_rate_limit_is_bounded_and_secret_free() {
        let fx = fixture(3).await;
        // One keep-alive connection: the socket-keyed limiter observes
        // one peer address, so the 4th request exhausts the budget.
        let request = format!(
            "POST /api/v1/task-triggers/{}/fire HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n",
            fx.trigger_id
        )
        .into_bytes();
        let responses = raw_requests_keepalive(
            fx.port,
            &[request.clone(), request.clone(), request.clone(), request],
        )
        .await;
        let statuses: Vec<u16> = responses.iter().map(|response| response.status).collect();
        assert_eq!(statuses[..3], [401, 401, 401]);
        assert_eq!(statuses[3], 429);
        for response in &responses {
            assert!(!response.body_str().contains(&fx.bearer));
        }
        let limited = &responses[3];
        assert!(limited.headers.contains_key("retry-after"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_fire_without_daemon_is_unavailable() {
        let pool = test_pool().await;
        let state = ServerState {
            pool,
            mcp_service: Arc::new(tokio::sync::RwLock::new(codegg::mcp::McpService::new())),
            config: codegg::config::schema::Config::default(),
            ws_rate_limiter: Arc::new(WsRateLimiter::new(100, 60)),
            daemon: None,
            projection_lifecycle_seam: Default::default(),
            connection_task_probe: None,
            probe_factory: None,
            transport_test_config: None,
        };
        let router = task_trigger_router(state, 1000, 60);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });
        let response = raw_request(
            port,
            &post_fire(
                "any-trigger",
                Some("cggtr_any-trigger.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                "",
                b"",
            ),
        )
        .await;
        assert_eq!(response.status, 503);
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body["code"], "trigger_unavailable");
    }

    #[derive(Clone, Default)]
    struct LogCapture {
        buf: Arc<Mutex<Vec<u8>>>,
    }

    impl std::io::Write for LogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().expect("log lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = LogCapture;

        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_authorization_header_never_reaches_logs() {
        static INIT: Once = Once::new();
        static CAPTURE: std::sync::LazyLock<LogCapture> =
            std::sync::LazyLock::new(LogCapture::default);
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_writer(CAPTURE.clone())
                .with_ansi(false)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
        let fx = fixture(1000).await;
        let response = raw_request(
            fx.port,
            &post_fire(&fx.trigger_id, Some(&fx.bearer), "", b""),
        )
        .await;
        assert_eq!(response.status, 200);
        // Wrong-secret attempt also logs only the locator.
        let (locator, _) = split_presented_trigger(&fx.bearer).unwrap();
        let wrong = format!("cggtr_{locator}.BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
        let denied = raw_request(fx.port, &post_fire(&fx.trigger_id, Some(&wrong), "", b"")).await;
        assert_eq!(denied.status, 401);
        // Give the spawned server task a chance to flush its events.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let logs = String::from_utf8_lossy(&CAPTURE.buf.lock().expect("log lock")).into_owned();
        assert!(
            logs.contains(&locator),
            "capture must observe trigger log lines"
        );
        assert!(
            !logs.contains(&fx.bearer),
            "Authorization bearer reached logs"
        );
        let (_, secret) = split_presented_trigger(&fx.bearer).unwrap();
        assert!(!logs.contains(&secret), "secret segment reached logs");
    }
}
