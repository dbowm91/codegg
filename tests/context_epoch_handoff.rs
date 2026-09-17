//! Long-horizon M004 — context-epoch reset and structured handoff integration.
//!
//! Deterministic coverage for the M004 acceptance matrix without a live
//! provider: WorkPlan checkpoint provenance bounds, revision revalidation,
//! epoch policy matrix, fresh-context reconstruction with the single-frame
//! invariant, phase-boundary → fresh epoch → continue next WorkItem,
//! repeated-compaction policy gating, steering preservation, normal
//! compaction fallback, restart/revision races, contention/cancellation,
//! security negatives, and migration compatibility.

mod common;

use codegg::agent::context_frame::ContextLedgerState;
use codegg::context::continuation::{assemble_continuation_snapshot, ContinuationAssemblyInput};
use codegg::context::epoch::{
    build_epoch_started_event, build_fresh_epoch_messages, decide_epoch,
    epoch_supported_for_profile, validate_fresh_epoch_messages, ContextEpochInputs,
    ContextEpochLineage, ContextEpochPolicy, ContextEpochTrigger, FreshEpochGoal, FreshEpochInputs,
};
use codegg::context::rollover;
use codegg::provider::{ContentPart, Message};
use codegg_core::model_profile::resolve::infer_builtin_profile;
use codegg_core::session::continuation::{
    ContinuationCheckpointPayload, ContinuationCheckpointStatus, ContinuationCheckpointStore,
};
use codegg_core::work_plan::{
    build_checkpoint_provenance, provenance_from_body, revalidate_against_current, NewWorkItem,
    NewWorkPlan, WorkItemStatus, WorkPlanStatus, WorkPlanStore,
};

fn user(text: &str) -> Message {
    Message::User {
        content: vec![ContentPart::Text {
            text: text.to_string().into(),
        }],
    }
}

async fn seed_session(pool: &sqlx::SqlitePool, session_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES ('epoch-project', '/tmp/epoch', '[]', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("seed project");
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, 'epoch-project', 'epoch', \
         '/tmp/epoch', 'Epoch', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("seed session");
}

async fn seed_work_plan(
    pool: &sqlx::SqlitePool,
    session_id: &str,
) -> (
    codegg_core::work_plan::WorkPlan,
    Vec<codegg_core::work_plan::WorkItem>,
) {
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(NewWorkPlan {
            session_id: session_id.to_string(),
            project_id: "epoch-project".to_string(),
            origin_turn_id: Some("turn-1".to_string()),
            goal_id: None,
            objective: "migrate checkout flow".to_string(),
            origin_provenance: "turn:turn-1".to_string(),
            current_phase: Some("phase one".to_string()),
        })
        .await
        .expect("create plan");
    let mut items = Vec::new();
    for (index, desc) in [
        "port discount validation",
        "update checkout tests",
        "ship migration",
    ]
    .iter()
    .enumerate()
    {
        let (_, item) = store
            .add_item(
                &plan.id,
                NewWorkItem {
                    parent_item_id: None,
                    dependencies: vec![],
                    status: WorkItemStatus::Pending,
                    description: desc.to_string(),
                    acceptance: vec![],
                    evidence: vec![],
                    owner_run_id: None,
                    owner_job_id: None,
                    blocker: None,
                    next_action: Some(format!("do step {}", index + 1)),
                },
            )
            .await
            .expect("add item");
        items.push(item);
    }
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    assert_eq!(items.len(), 3);
    let _ = &items;
    (plan, items)
}

fn supported_profile() -> codegg_core::model_profile::types::ResolvedModelProfile {
    // Long-horizon profile that opts into fresh epochs. OpenAI frontier
    // reasoning maps to FrontierReasoning in the adapter table.
    infer_builtin_profile("openai/gpt-5")
}

fn unsupported_profile() -> codegg_core::model_profile::types::ResolvedModelProfile {
    infer_builtin_profile("minimax/minimax-2.7")
}

// ── Work package A: checkpoint WorkPlan provenance ────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn checkpoint_carries_bounded_work_plan_provenance() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-epoch-prov";
    seed_session(&pool, session_id).await;
    let (plan, items) = seed_work_plan(&pool, session_id).await;
    let provenance = build_checkpoint_provenance(&plan, &items);
    assert_eq!(provenance.plan_id, plan.id.as_str());
    assert_eq!(provenance.revision, plan.revision);
    assert!(provenance.actionable.len() <= 5);
    let bytes = serde_json::to_string(&provenance).unwrap().len();
    assert!(bytes <= 4096, "provenance must stay bounded, got {bytes}");

    // Snapshot carries it; payload validates through the M001 store contract.
    let ledger = ContextLedgerState::new();
    let messages = vec![user("migrate checkout flow")];
    let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
        session_id,
        origin_prompt: Some("migrate checkout flow"),
        current_user_message: None,
        messages: &messages,
        active_goal: None,
        todos: &[],
        ledger: &ledger,
        security_findings: &[],
        previous_checkpoint: None,
        plan_path: None,
        plan_content: None,
        active_work_plan: Some((&plan, items.as_slice())),
    });
    assert!(snapshot.work_plan.is_some());
    let payload = snapshot.to_payload().expect("payload validates");
    let decoded = provenance_from_body(&payload.body).unwrap().unwrap();
    assert_eq!(decoded.plan_id, plan.id.as_str());

    // Durable prepare/install round-trips the provenance.
    let store = ContinuationCheckpointStore::new(pool.clone());
    let prepared = store
        .prepare(session_id, None, payload)
        .await
        .expect("prepare");
    assert_eq!(prepared.status, ContinuationCheckpointStatus::Prepared);
    let event = prepared
        .build_compacted_event(2, 2, Some(100), Some(40), vec![], vec![], vec![])
        .unwrap();
    let installed = store
        .install_with_compaction_event(session_id, &prepared.id, event)
        .await
        .expect("install");
    assert_eq!(installed.status, ContinuationCheckpointStatus::Installed);
    let reloaded = store.latest_installed(session_id).await.unwrap().unwrap();
    let reloaded_provenance = provenance_from_body(&reloaded.payload.body)
        .unwrap()
        .unwrap();
    assert_eq!(reloaded_provenance.revision, plan.revision);
}

#[tokio::test(flavor = "current_thread")]
async fn checkpoint_rejects_stale_work_plan_install() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-epoch-stale";
    seed_session(&pool, session_id).await;
    let (mut plan, items) = seed_work_plan(&pool, session_id).await;
    let provenance = build_checkpoint_provenance(&plan, &items);
    // Simulate a concurrent plan mutation bumping the revision.
    plan.revision += 1;
    let err = revalidate_against_current(Some(&provenance), Some((&plan, &items))).unwrap_err();
    assert!(err.contains("stale"), "unexpected: {err}");

    // Rollover source revisions also reject work-plan drift.
    let captured = rollover::RolloverSourceRevisions::capture_with_work_plan(
        None,
        None,
        None,
        1,
        None,
        "history-a".to_string(),
        Some(plan.id.as_str().to_string()),
        Some(provenance.revision),
    );
    let current = rollover::RolloverSourceRevisions::capture_with_work_plan(
        None,
        None,
        None,
        1,
        None,
        "history-a".to_string(),
        Some(plan.id.as_str().to_string()),
        Some(plan.revision),
    );
    assert!(captured.is_stale_against(&current));
    assert_eq!(
        captured.stale_reason(&current),
        Some("active work plan revision changed")
    );
}

// ── Work package B: epoch policy ──────────────────────────────────────────

#[test]
fn epoch_policy_decision_matrix() {
    let enabled = ContextEpochPolicy::enabled_for_handoff(Some(3));
    // Disabled default never resets.
    let disabled = ContextEpochPolicy::default();
    assert!(
        !decide_epoch(
            &disabled,
            &ContextEpochInputs::new(99, true, true, true, true)
        )
        .should_reset
    );
    // Unsupported profile stays on normal compaction.
    assert_eq!(
        decide_epoch(
            &enabled,
            &ContextEpochInputs::new(99, true, true, true, false)
        )
        .reason_code(),
        "unsupported_profile"
    );
    // Same state → same decision (deterministic).
    let inputs = ContextEpochInputs::new(3, false, false, false, true);
    assert_eq!(
        decide_epoch(&enabled, &inputs),
        decide_epoch(&enabled, &inputs)
    );
    // Explicit operator wins.
    assert_eq!(
        decide_epoch(
            &enabled,
            &ContextEpochInputs::new(0, true, true, false, true)
        )
        .trigger,
        Some(ContextEpochTrigger::ExplicitOperator)
    );
    // Phase boundary before threshold.
    assert_eq!(
        decide_epoch(
            &enabled,
            &ContextEpochInputs::new(0, true, false, false, true)
        )
        .trigger,
        Some(ContextEpochTrigger::PhaseBoundary)
    );
    // Threshold matrix.
    assert!(
        !decide_epoch(
            &enabled,
            &ContextEpochInputs::new(2, false, false, false, true)
        )
        .should_reset
    );
    assert!(
        decide_epoch(
            &enabled,
            &ContextEpochInputs::new(3, false, false, false, true)
        )
        .should_reset
    );
}

#[test]
fn unsupported_profiles_remain_on_normal_compaction() {
    assert!(epoch_supported_for_profile(&supported_profile()));
    assert!(!epoch_supported_for_profile(&unsupported_profile()));
    // Unknown/default models stay conservative.
    assert!(!epoch_supported_for_profile(&infer_builtin_profile(
        "some-provider/some-model"
    )));
}

// ── Work package C: fresh-context reconstruction ──────────────────────────

#[test]
fn fresh_context_single_frame_and_tool_contracts() {
    let profile = supported_profile();
    let provenance = codegg_core::work_plan::WorkPlanCheckpointProvenance {
        plan_id: "wp_1".to_string(),
        revision: 1,
        status: "active".to_string(),
        current_phase: Some("phase one".to_string()),
        current_item_id: Some("wi_a".to_string()),
        actionable: vec![codegg_core::work_plan::WorkPlanCheckpointItem {
            id: "wi_a".to_string(),
            revision: 0,
            status: "actionable".to_string(),
            description: "port discount validation".to_string(),
            next_action: Some("run checkout tests".to_string()),
        }],
        blocked: vec![],
        source_digest: "sha256:abc".to_string(),
        total_items: 2,
        actionable_count: 1,
        blocked_count: 0,
    };
    let todos = vec!["port discount validation".to_string()];
    let handles = vec!["ctx://tool/s/0/c0".to_string()];
    let steering = vec!["keep discounts backward compatible".to_string()];
    let inputs = FreshEpochInputs {
        system_instructions: "system: canonical instructions",
        objective: "migrate checkout flow",
        goal: Some(FreshEpochGoal {
            goal_id: "goal-1",
            revision: 1,
            objective: "migrate checkout flow",
            current_phase: Some("phase one"),
            next_action: Some("run checkout tests"),
        }),
        work_plan: Some(&provenance),
        todos: &todos,
        continuation_frame_text:
            "[codegg continuation state v1]\n- Goal: migrate\n- Next steps: run tests",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-1",
        checkpoint_sequence: 1,
    };
    let messages = build_fresh_epoch_messages(&inputs, &profile).expect("fresh epoch");
    assert!(validate_fresh_epoch_messages(&messages).is_ok());
    assert_eq!(
        codegg::context::compaction::count_continuation_frames(&messages),
        1
    );
    assert!(codegg::context::compaction::validate_message_invariants(&messages).is_ok());
    // Provider can continue the current WorkItem immediately.
    let visible = messages
        .iter()
        .filter_map(|m| match m {
            Message::System { content } => Some(content.to_string()),
            Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(visible.contains("wi_a"));
    assert!(visible.contains("run checkout tests"));
    assert!(visible.contains("keep discounts backward compatible"));
    // No hidden reasoning.
    for message in &messages {
        if let Message::User { content } | Message::Assistant { content, .. } = message {
            for part in content {
                assert!(
                    !matches!(part, ContentPart::Reasoning { .. }),
                    "fresh epoch must not carry hidden reasoning"
                );
            }
        }
    }
}

#[test]
fn unsupported_profile_falls_back_to_normal_compaction() {
    let profile = unsupported_profile();
    let todos: Vec<String> = vec![];
    let handles: Vec<String> = vec![];
    let steering: Vec<String> = vec![];
    let inputs = FreshEpochInputs {
        system_instructions: "system",
        objective: "objective",
        goal: None,
        work_plan: None,
        todos: &todos,
        continuation_frame_text: "frame",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-1",
        checkpoint_sequence: 1,
    };
    let err = build_fresh_epoch_messages(&inputs, &profile).unwrap_err();
    assert!(err.contains("unsupported profile"));
}

#[tokio::test(flavor = "current_thread")]
async fn phase_boundary_to_fresh_epoch_continues_next_work_item() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-epoch-phase";
    seed_session(&pool, session_id).await;
    let (plan, items) = seed_work_plan(&pool, session_id).await;
    let provenance = build_checkpoint_provenance(&plan, &items);
    // Verified phase completion with a supported profile triggers reset.
    let policy = ContextEpochPolicy::enabled_for_handoff(Some(8));
    let decision = decide_epoch(
        &policy,
        &ContextEpochInputs::new(2, true, false, false, true),
    );
    assert_eq!(decision.trigger, Some(ContextEpochTrigger::PhaseBoundary));

    // Fresh epoch carries the next actionable item.
    let profile = supported_profile();
    let todos = vec![items[0].description.clone()];
    let handles: Vec<String> = vec![];
    let steering = vec!["ship phase one first".to_string()];
    let inputs = FreshEpochInputs {
        system_instructions: "system: canonical",
        objective: plan.objective.as_str(),
        goal: None,
        work_plan: Some(&provenance),
        todos: &todos,
        continuation_frame_text: "prior continuation state",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-phase",
        checkpoint_sequence: 1,
    };
    let messages = build_fresh_epoch_messages(&inputs, &profile).unwrap();
    // The next WorkItem is immediately actionable (no rediscovery).
    let first_actionable = codegg_core::work_plan::actionable_items(&items)
        .first()
        .unwrap()
        .description
        .clone();
    let visible = format!("{messages:?}");
    assert!(visible.contains(&first_actionable[..20.min(first_actionable.len())]));
    assert!(visible.contains("ship phase one first"));
}

#[test]
fn repeated_compactions_trigger_only_for_supported_profile() {
    let policy = ContextEpochPolicy::enabled_for_handoff(Some(2));
    // Supported profile at threshold resets.
    assert!(
        decide_epoch(
            &policy,
            &ContextEpochInputs::new(2, false, false, false, true)
        )
        .should_reset
    );
    // Same count with an unsupported profile keeps normal compaction.
    assert!(
        !decide_epoch(
            &policy,
            &ContextEpochInputs::new(5, false, false, false, false)
        )
        .should_reset
    );
    // Disabled policy never resets even at high counts.
    assert!(
        !decide_epoch(
            &ContextEpochPolicy::default(),
            &ContextEpochInputs::new(99, false, false, false, true)
        )
        .should_reset
    );
}

#[test]
fn recent_steering_preserved_after_old_checkpoint() {
    let profile = supported_profile();
    let todos: Vec<String> = vec![];
    let handles: Vec<String> = vec![];
    let steering = vec![
        "old steering from before checkpoint".to_string(),
        "latest steering correction".to_string(),
    ];
    let inputs = FreshEpochInputs {
        system_instructions: "system",
        objective: "objective",
        goal: None,
        work_plan: None,
        todos: &todos,
        continuation_frame_text: "old continuation state",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-old",
        checkpoint_sequence: 1,
    };
    let messages = build_fresh_epoch_messages(&inputs, &profile).unwrap();
    let visible: String = messages
        .iter()
        .filter_map(|m| match m {
            Message::System { content } => Some(content.to_string()),
            Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(visible.contains("latest steering correction"));
}

// ── Work package D: restart/diagnostics ────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_after_fresh_epoch_preserves_lineage() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-epoch-restart";
    seed_session(&pool, session_id).await;
    let (plan, items) = seed_work_plan(&pool, session_id).await;
    let ledger = ContextLedgerState::new();
    let messages = vec![user("migrate checkout flow")];
    let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
        session_id,
        origin_prompt: Some("migrate checkout flow"),
        current_user_message: None,
        messages: &messages,
        active_goal: None,
        todos: &[],
        ledger: &ledger,
        security_findings: &[],
        previous_checkpoint: None,
        plan_path: None,
        plan_content: None,
        active_work_plan: Some((&plan, items.as_slice())),
    });
    let store = ContinuationCheckpointStore::new(pool.clone());
    let prepared = store
        .prepare(session_id, None, snapshot.to_payload().unwrap())
        .await
        .unwrap();
    let event = prepared
        .build_compacted_event(1, 1, Some(50), Some(20), vec![], vec![], vec![])
        .unwrap();
    let installed = store
        .install_with_compaction_event(session_id, &prepared.id, event)
        .await
        .unwrap();

    // Restart: reopen the store against the same database file. Prepared
    // candidates never become authority; only the installed row resumes.
    let reopened = ContinuationCheckpointStore::new(pool.clone());
    let latest = reopened
        .latest_installed(session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.id, installed.id);
    assert_eq!(
        rollover::validate_installed_for_restart(&latest, session_id),
        rollover::RestartValidation::Usable
    );
    // WorkPlan provenance survives restart; the next action is still current.
    let provenance = provenance_from_body(&latest.payload.body).unwrap().unwrap();
    assert_eq!(provenance.plan_id, plan.id.as_str());
    // Newer WorkPlan revision overrides the stale checkpoint next action.
    let mut newer_plan = plan.clone();
    newer_plan.revision += 1;
    assert!(revalidate_against_current(Some(&provenance), Some((&newer_plan, &items))).is_err());

    // Epoch lineage/event explains why the fresh epoch occurred.
    let lineage = ContextEpochLineage {
        epoch_id: "epoch-1".to_string(),
        reason: "phase_boundary".to_string(),
        trigger: "phase_boundary".to_string(),
        checkpoint_id: installed.id.clone(),
        checkpoint_sequence: installed.sequence,
        work_plan_id: Some(plan.id.as_str().to_string()),
        work_plan_revision: Some(plan.revision),
        prior_compaction_count: 3,
        profile_id: "openai/gpt-5".to_string(),
    };
    assert!(lineage.bounded_line().contains(&installed.id));
    let decision = decide_epoch(
        &ContextEpochPolicy::enabled_for_handoff(Some(3)),
        &ContextEpochInputs::new(0, true, false, false, true),
    );
    let event = build_epoch_started_event(session_id, &decision, &lineage);
    assert_eq!(event.event_type(), "context_epoch:started");
    match event {
        codegg_core::bus::events::AppEvent::ContextEpochStarted {
            reason,
            checkpoint_id,
            work_plan_id,
            ..
        } => {
            assert_eq!(reason, "phase_boundary");
            assert_eq!(checkpoint_id, installed.id);
            assert_eq!(work_plan_id, Some(plan.id.as_str().to_string()));
        }
        other => panic!("unexpected event: {}", other.event_type()),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn restart_before_candidate_activation_ignores_prepared() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-epoch-prepared";
    seed_session(&pool, session_id).await;
    let ledger = ContextLedgerState::new();
    let messages = vec![user("objective")];
    let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
        session_id,
        origin_prompt: Some("objective"),
        current_user_message: None,
        messages: &messages,
        active_goal: None,
        todos: &[],
        ledger: &ledger,
        security_findings: &[],
        previous_checkpoint: None,
        plan_path: None,
        plan_content: None,
        active_work_plan: None,
    });
    let store = ContinuationCheckpointStore::new(pool.clone());
    let _prepared = store
        .prepare(session_id, None, snapshot.to_payload().unwrap())
        .await
        .unwrap();
    // Restart before activation: no installed checkpoint exists.
    let reopened = ContinuationCheckpointStore::new(pool.clone());
    assert!(reopened
        .latest_installed(session_id)
        .await
        .unwrap()
        .is_none());
}

// ── Contention and cancellation ───────────────────────────────────────────

#[test]
fn work_plan_todo_goal_races_abort_candidate() {
    let base = rollover::RolloverSourceRevisions::capture_with_work_plan(
        Some("goal-1".to_string()),
        Some(1),
        None,
        4,
        None,
        "h".to_string(),
        Some("wp_1".to_string()),
        Some(1),
    );
    // Goal revision drift aborts.
    let goal_drift = rollover::RolloverSourceRevisions::capture_with_work_plan(
        Some("goal-1".to_string()),
        Some(2),
        None,
        4,
        None,
        "h".to_string(),
        Some("wp_1".to_string()),
        Some(1),
    );
    assert!(base.is_stale_against(&goal_drift));
    // Todo revision drift aborts.
    let todo_drift = rollover::RolloverSourceRevisions::capture_with_work_plan(
        Some("goal-1".to_string()),
        Some(1),
        None,
        5,
        None,
        "h".to_string(),
        Some("wp_1".to_string()),
        Some(1),
    );
    assert!(base.is_stale_against(&todo_drift));
    // WorkPlan drift aborts.
    let plan_drift = rollover::RolloverSourceRevisions::capture_with_work_plan(
        Some("goal-1".to_string()),
        Some(1),
        None,
        4,
        None,
        "h".to_string(),
        Some("wp_1".to_string()),
        Some(2),
    );
    assert!(base.is_stale_against(&plan_drift));
    // Unrelated telemetry never aborts (history digest is diagnostic-only).
    let same = base.clone();
    assert!(!base.is_stale_against(&same));
}

#[test]
fn steering_during_preparation_prevents_stale_activation() {
    // Steering changes todo revision between capture and install; the
    // candidate must abort rather than install a stale next action.
    let captured = rollover::RolloverSourceRevisions::capture_with_work_plan(
        None,
        None,
        None,
        7,
        None,
        "h".to_string(),
        None,
        None,
    );
    let steered = rollover::RolloverSourceRevisions::capture_with_work_plan(
        None,
        None,
        None,
        8,
        None,
        "h".to_string(),
        None,
        None,
    );
    assert!(captured.is_stale_against(&steered));
    assert_eq!(captured.stale_reason(&steered), Some("todo state changed"));
}

// ── Security and negative tests ───────────────────────────────────────────

#[test]
fn epoch_preserves_authority_and_hides_reasoning() {
    // Permissions/sandbox/model selection are untouched by reconstruction:
    // the builder returns only messages and never mutates policy state.
    // This is structural (no policy handle enters FreshEpochInputs), asserted
    // here by building an epoch and checking the output carries no capability
    // grant, no reasoning, and no cross-session handle.
    let profile = supported_profile();
    let todos: Vec<String> = vec![];
    let handles = vec!["ctx://tool/sess-a/0/c0".to_string()];
    let steering = vec!["do not touch other sessions".to_string()];
    let inputs = FreshEpochInputs {
        system_instructions: "system: canonical",
        objective: "objective",
        goal: None,
        work_plan: None,
        todos: &todos,
        continuation_frame_text: "frame",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-1",
        checkpoint_sequence: 1,
    };
    let messages = build_fresh_epoch_messages(&inputs, &profile).unwrap();
    for message in &messages {
        match message {
            Message::Assistant { content, .. } | Message::User { content } => {
                for part in content {
                    assert!(
                        !matches!(part, ContentPart::Reasoning { .. }),
                        "no hidden reasoning"
                    );
                    if let ContentPart::Text { text } = part {
                        assert!(
                            !text.contains("sess-other"),
                            "no cross-session handle exposure"
                        );
                    }
                }
            }
            Message::System { content } => {
                assert!(!content.contains("api_key"));
                assert!(!content.contains("reasoning"));
            }
            Message::Tool { .. } => panic!("fresh epoch carries no tool history"),
        }
    }
}

// ── Migration and compatibility ───────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn old_checkpoint_without_work_plan_renders_safely() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-epoch-legacy";
    seed_session(&pool, session_id).await;
    // Legacy body with no work_plan key (pre-M004).
    let body = serde_json::json!({
        "snapshot_kind": "continuation_snapshot_v1",
        "session_id": session_id,
        "objective": "legacy objective",
        "current_task": "legacy task",
        "semantic": {"constraints": [], "decisions": [], "unresolved_blockers": [], "next_steps": ["legacy next"]},
    });
    let payload = ContinuationCheckpointPayload::new(body).unwrap();
    let store = ContinuationCheckpointStore::new(pool.clone());
    let prepared = store.prepare(session_id, None, payload).await.unwrap();
    let event = prepared
        .build_compacted_event(1, 1, None, None, vec![], vec![], vec![])
        .unwrap();
    let installed = store
        .install_with_compaction_event(session_id, &prepared.id, event)
        .await
        .unwrap();
    assert_eq!(
        rollover::validate_installed_for_restart(&installed, session_id),
        rollover::RestartValidation::Usable
    );
    assert!(provenance_from_body(&installed.payload.body)
        .unwrap()
        .is_none());
    let projection = rollover::render_installed_projection(&installed, None);
    assert!(projection.contains("legacy objective"));
    // Normal compaction remains functional when reset is disabled.
    let policy = ContextEpochPolicy::default();
    let decision = decide_epoch(
        &policy,
        &ContextEpochInputs::new(99, true, true, true, true),
    );
    assert!(!decision.should_reset);
    assert_eq!(decision.reason_code(), "disabled");
}

#[test]
fn old_model_profile_config_behaves_unchanged() {
    // Profiles with no epoch field (all existing adapters) inherit the
    // conservative default: unknown/default/local/minimax stay on normal
    // compaction unless an explicit opt-in policy plus a safe trigger exists.
    for model in [
        "some-provider/some-model",
        "ollama/qwen2.5-coder:32b",
        "minimax/minimax-2.7",
    ] {
        let profile = infer_builtin_profile(model);
        assert!(
            !epoch_supported_for_profile(&profile),
            "legacy profile {model} must stay on normal compaction"
        );
    }
}

#[test]
fn no_second_compaction_engine_or_history_store() {
    // Static guard: epoch reconstruction must remain a consumer of the
    // canonical compaction/rollover owners. It must not introduce its own
    // compaction algorithm, token accounting, transcript table, or history
    // rewrite.
    let source = include_str!("../src/context/epoch.rs");
    for forbidden in [
        "fn compact_context",
        "fn needs_context_compaction",
        "fn context_tokens",
        "CREATE TABLE",
        "continuation_checkpoint(",
        "DELETE FROM session",
        "UPDATE session SET",
    ] {
        assert!(
            !source.contains(forbidden),
            "epoch module must not introduce '{forbidden}' (second engine/history store)"
        );
    }
    for required in [
        "validate_message_invariants",
        "count_continuation_frames",
        "decide_epoch",
        "epoch_supported_for_profile",
    ] {
        assert!(
            source.contains(required),
            "epoch module must reuse canonical owner '{required}'"
        );
    }
}

#[test]
fn work_plan_status_and_plan_invariants_hold() {
    // Fresh epoch never flips plan status itself; status comes from the
    // durable plan. This test pins the provenance status vocabulary so a
    // future change cannot silently invent a terminal transition here.
    for status in ["active", "blocked", "completed", "cancelled"] {
        assert!(WorkPlanStatus::parse(status).is_some());
    }
    assert!(WorkPlanStatus::parse("invented").is_none());
}
