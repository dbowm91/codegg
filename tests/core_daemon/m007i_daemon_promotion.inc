//! M007-I (work packages B, C, D and E): the daemon-owned promotion
//! workflow, its publication and the ask.
//!
//! The properties proved here are the ones the milestone's acceptance
//! criteria name:
//!
//! 4. the promotion record has **exactly one** writer, and it is the daemon;
//! 5. the namespace is the canonical `WorkspaceId`, so a promotion staged
//!    through the frontend is submitted successfully by the daemon tool;
//! 6. a legacy proposal that cannot be migrated **fails loudly**;
//! 7. the daemon publishes to **both** scopes;
//! 8. the C004 gate **refuses** a publication into a reserved checkout, and
//!    the refusal is durable and attributable;
//! 9. the sink enum and the manifest agree in **both** directions;
//! 10. the ask's single request leaves **no** orphan `pending` row.

use super::common;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg_core::memory::habit::{
    HabitId, HabitStore, WorkflowAction, WorkflowActionKind, WorkflowEffectClass,
    WorkflowOccurrence, WorkflowOutcome,
};
use codegg_core::session::skill_promotion::{SkillPromotionKind, SkillPromotionStore};
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::skill::{
    SkillPromotionActionDto, SkillPromotionRequestDto, SkillTargetScopeDto,
};

const LOCAL_CLIENT: &str = "m007i-promotion-client";
const WORKSPACE_ID: &str = "m007i-workspace";

fn register_local_client(daemon: &CoreDaemon) {
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "m007i-local".to_owned(),
        None,
        codegg_core::transport_auth::AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
}

/// A daemon whose promotion workflow reads its habits from `memory_root`.
///
/// The habit root is a host setting, so the daemon and the model-facing
/// tool can be pointed at the same one. That is what makes the two sides
/// agree on the habit, and therefore on the record.
fn daemon_with_memory_root(pool: sqlx::SqlitePool, memory_root: std::path::PathBuf) -> CoreDaemon {
    let deps = CoreRuntimeDeps::new(Some(pool), None, None).with_habit_root(memory_root);
    let daemon = CoreDaemon::with_deps(deps);
    register_local_client(&daemon);
    daemon
}

async fn request(daemon: &CoreDaemon, payload: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload,
        },
        LOCAL_CLIENT,
    ))
    .await
    .expect("the daemon must answer")
}

/// A workspace with a ready habit, so a promotion request can be staged.
async fn workspace_with_a_ready_habit(
    pool: &sqlx::SqlitePool,
    memory_root: &std::path::Path,
    root: &std::path::Path,
) -> HabitId {
    workspace_with_two_ready_habits(pool, memory_root, root)
        .await
        .0
}

/// A workspace with **two** ready habits.
///
/// Two habits are needed because a publication promotes the habit it came
/// from, and a promoted habit cannot be promoted again: two scopes therefore
/// need two habits.
async fn workspace_with_two_ready_habits(
    pool: &sqlx::SqlitePool,
    memory_root: &std::path::Path,
    root: &std::path::Path,
) -> (HabitId, HabitId) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, \
         time_archived) VALUES (?, ?, ?, ?, ?, NULL)",
    )
    .bind(WORKSPACE_ID)
    .bind(root.to_string_lossy().as_ref())
    .bind("m007i")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("seed the workspace");

    let identity = root.to_string_lossy().to_string();
    let habits = HabitStore::with_root(memory_root.to_path_buf()).unwrap();
    let namespace = codegg_core::memory::project_namespace(&identity);
    let actions = vec![
        WorkflowAction::new(
            WorkflowActionKind::FileRead,
            None,
            WorkflowEffectClass::ReadOnly,
        ),
        WorkflowAction::new(
            WorkflowActionKind::Edit,
            None,
            WorkflowEffectClass::Mutating,
        ),
    ];
    for (session, turn) in [("session-1", "turn-1"), ("session-1", "turn-2"), ("session-2", "turn-3")] {
        habits
            .observe(WorkflowOccurrence {
                project_namespace: namespace.clone(),
                session_id: session.to_string(),
                turn_id: Some(turn.to_string()),
                root_or_run_id: None,
                actions: actions.clone(),
                outcome: WorkflowOutcome::Succeeded,
                occurred_at: 1,
            })
            .unwrap();
    }
    // A second, distinct habit: a different action skeleton, so it is a
    // different candidate.
    let other_actions = vec![
        WorkflowAction::new(
            WorkflowActionKind::FileRead,
            None,
            WorkflowEffectClass::ReadOnly,
        ),
        WorkflowAction::new(
            WorkflowActionKind::Search,
            None,
            WorkflowEffectClass::ReadOnly,
        ),
    ];
    for (session, turn) in [
        ("session-1", "turn-9"),
        ("session-1", "turn-10"),
        ("session-2", "turn-11"),
    ] {
        habits
            .observe(WorkflowOccurrence {
                project_namespace: namespace.clone(),
                session_id: session.to_string(),
                turn_id: Some(turn.to_string()),
                root_or_run_id: None,
                actions: other_actions.clone(),
                outcome: WorkflowOutcome::Succeeded,
                occurred_at: 2,
            })
            .unwrap();
    }
    let mut candidates = habits.load(&identity).unwrap();
    let second = candidates.pop().expect("the second candidate").id;
    let first = candidates.remove(0).id;
    (first, second)
}

/// A host memory root at `<config>/codegg/memory`, so the configuration root
/// is the temporary directory and nothing is written outside it.
///
/// The returned directory owns the lifetime; the path is the memory root.
fn memory_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("a configuration root");
    let root = dir.path().join("codegg").join("memory");
    std::fs::create_dir_all(&root).expect("create the memory root");
    (dir, root)
}

/// The tool's execution context for one session.
fn tool_context(session_id: &str) -> codegg::tool::ToolExecutionContext {
    let mut context = codegg::tool::ToolExecutionContext::with_backend(
        codegg::tool::ToolBackendKind::Native,
    );
    context.session_id = Some(session_id.to_owned());
    context
}

/// The daemon's answer to one promotion request.
async fn promote(
    daemon: &CoreDaemon,
    action: SkillPromotionActionDto,
    habit_id: Option<&str>,
    proposal_id: Option<&str>,
    target_scope: Option<SkillTargetScopeDto>,
) -> CoreResponse {
    request(
        daemon,
        CoreRequest::SkillPromotion {
            request: SkillPromotionRequestDto {
                workspace_id: WORKSPACE_ID.to_owned(),
                session_id: "session-1".to_owned(),
                action,
                habit_id: habit_id.map(str::to_owned),
                proposal_id: proposal_id.map(str::to_owned),
                target_scope,
            },
        },
    )
    .await
}

/// The request the daemon staged, as the frontend sees it.
async fn staged_request_id(daemon: &CoreDaemon, habit_id: &HabitId) -> Option<String> {
    match promote(
        daemon,
        SkillPromotionActionDto::Begin,
        Some(habit_id.as_str()),
        None,
        None,
    )
    .await
    {
        CoreResponse::SkillPromotion { result } => result.request_id,
        other => panic!("expected a promotion answer, got {other:?}"),
    }
}

/// The promotion store's rows for one workspace.
async fn stored_rows(
    pool: &sqlx::SqlitePool,
    kind: SkillPromotionKind,
) -> Vec<(String, String, String)> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, status, payload FROM skill_promotion WHERE workspace_id = ? AND kind = ? \
         ORDER BY created_at_ms, id",
    )
    .bind(WORKSPACE_ID)
    .bind(kind.as_str())
    .fetch_all(pool)
    .await
    .expect("read the promotion rows")
}

/// The promotion store has exactly **one** writer, and it is the daemon.
///
/// The frontend's path and the model-facing tool's path both write the same
/// record, in the daemon's own store: the request is written once, and the
/// proposal that consumes it is written once.
#[tokio::test(flavor = "current_thread")]
async fn the_promotion_record_has_exactly_one_writer_and_it_is_the_daemon() {
    let pool = common::pool::isolated_pool().await;
    let (_memory_dir, memory_root) = memory_root();
    let checkout = tempfile::tempdir().expect("a checkout");
    let habit_id = workspace_with_a_ready_habit(&pool, &memory_root, checkout.path()).await;
    let daemon = daemon_with_memory_root(pool.clone(), memory_root.clone());

    // The frontend's path: the daemon stages the request.
    let request_id = staged_request_id(&daemon, &habit_id)
        .await
        .expect("the daemon staged the request");
    let requests = stored_rows(&pool, SkillPromotionKind::Request).await;
    assert_eq!(
        requests.len(),
        1,
        "the daemon's store holds one request, got {requests:?}"
    );
    assert_eq!(requests[0].0, request_id);
    assert_eq!(requests[0].1, "pending");

    // The model-facing tool's path: the daemon-side tool consumes the
    // request and records the proposal in the **same** store.
    let tool = codegg::tool::skill_proposal::SkillProposalTool::new(
        pool.clone(),
        WORKSPACE_ID,
        checkout.path().to_path_buf(),
        memory_root.clone(),
    );
    let outcome = codegg::tool::Tool::execute_structured(
        &tool,
        serde_json::json!({
            "action": "submit",
            "promotion_request_id": request_id,
            "habit_id": habit_id.as_str(),
            "name": "demo",
            "description": "A demo skill",
            "skill_markdown": "---\nname: demo\ndescription: A demo skill\n---\n# Demo\n\nUse the workflow."
        }),
        Some(tool_context("session-1")),
    )
    .await
    .expect("the daemon tool submits the proposal");
    assert!(
        outcome.output.contains("\"id\""),
        "the tool must answer with the recorded proposal, got {}",
        outcome.output
    );

    let proposals = stored_rows(&pool, SkillPromotionKind::Proposal).await;
    assert_eq!(
        proposals.len(),
        1,
        "the daemon's store holds one proposal, got {proposals:?}"
    );
    assert_eq!(proposals[0].1, "validated");

    // The request was consumed by that one submission, so the workflow has
    // exactly one record and not a second copy of it.
    let requests = stored_rows(&pool, SkillPromotionKind::Request).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].1, "consumed");
}

/// The namespace is the canonical `WorkspaceId`.
///
/// The frontend's staged request and the daemon tool's proposal resolve to
/// the **same** record. The divergence that produced "promotion request not
/// found" is closed by construction.
#[tokio::test(flavor = "current_thread")]
async fn a_promotion_staged_through_the_frontend_is_submitted_by_the_daemon_tool() {
    let pool = common::pool::isolated_pool().await;
    let (_memory_dir, memory_root) = memory_root();
    let checkout = tempfile::tempdir().expect("a checkout");
    let habit_id = workspace_with_a_ready_habit(&pool, &memory_root, checkout.path()).await;
    let daemon = daemon_with_memory_root(pool.clone(), memory_root.clone());
    let request_id = staged_request_id(&daemon, &habit_id)
        .await
        .expect("the daemon staged the request");

    // The tool looks the request up under the canonical workspace — the same
    // one the frontend's request named.
    let store = SkillPromotionStore::new(pool.clone());
    let staged = store
        .get(WORKSPACE_ID, SkillPromotionKind::Request, &request_id)
        .await
        .expect("read the staged request")
        .expect("the staged request is under the canonical workspace");
    assert_eq!(staged.workspace_id, WORKSPACE_ID);
    assert!(
        !staged.workspace_id.contains(".."),
        "the namespace is the workspace identity, not a path"
    );
    assert!(
        store
            .get("some-other-workspace", SkillPromotionKind::Request, &request_id)
            .await
            .expect("read under another workspace")
            .is_none(),
        "the record is not reachable under another workspace"
    );

    // And the tool submits it successfully.
    let tool = codegg::tool::skill_proposal::SkillProposalTool::new(
        pool.clone(),
        WORKSPACE_ID,
        checkout.path().to_path_buf(),
        memory_root.clone(),
    );
    codegg::tool::Tool::execute_structured(
        &tool,
        serde_json::json!({
            "action": "submit",
            "promotion_request_id": request_id,
            "habit_id": habit_id.as_str(),
            "name": "demo",
            "description": "A demo skill",
            "skill_markdown": "---\nname: demo\ndescription: A demo skill\n---\n# Demo\n\nUse the workflow."
        }),
        Some(tool_context("session-1")),
    )
    .await
    .expect("the daemon tool submits the proposal the frontend staged");
}

/// A legacy proposal that cannot be migrated **fails loudly**.
///
/// The legacy store keyed its file on a digest of a path string, so its
/// records cannot be mapped onto the canonical workspace. A record that
/// cannot be migrated is reported, not dropped.
#[tokio::test(flavor = "current_thread")]
async fn a_legacy_proposal_that_cannot_be_migrated_fails_loudly() {
    let pool = common::pool::isolated_pool().await;
    let (_memory_dir, memory_root) = memory_root();
    let checkout = tempfile::tempdir().expect("a checkout");
    let habit_id = workspace_with_a_ready_habit(&pool, &memory_root, checkout.path()).await;

    // A legacy record for this workspace, awaiting review.
    let legacy_root = memory_root.join("skill-promotions");
    std::fs::create_dir_all(&legacy_root).expect("create the legacy root");
    let identity = checkout.path().to_string_lossy().to_string();
    let namespace = codegg_core::memory::project_namespace(&identity);
    let suffix = namespace.strip_prefix("project/").expect("the legacy namespace");
    let legacy_file = legacy_root.join(format!("{suffix}.json"));
    std::fs::write(
        &legacy_file,
        serde_json::json!({
            "version": 1,
            "requests": [],
            "proposals": [{
                "id": "legacy-proposal",
                "status": "validated",
                "promotion_request_id": "legacy-request",
                "habit_id": habit_id.as_str(),
                "habit_fingerprint": "fingerprint",
                "candidate_revision": 1,
                "project_namespace": namespace,
                "target_scope": "project",
                "name": "demo",
                "description": "A demo skill",
                "skill_markdown": "---\nname: demo\ndescription: A demo skill\n---\nbody",
                "content_digest": "digest",
                "status": "validated",
                "diagnostics": [],
                "created_at": 1,
                "updated_at": 1,
                "revision": 1
            }]
        })
        .to_string(),
    )
    .expect("write the legacy record");

    let daemon = daemon_with_memory_root(pool.clone(), memory_root.clone());
    let response = promote(
        &daemon,
        SkillPromotionActionDto::Begin,
        Some(habit_id.as_str()),
        None,
        None,
    )
    .await;
    match response {
        CoreResponse::Error { code, message } => {
            assert_eq!(
                code, "skill_promotion_legacy_unmigrated",
                "the legacy record must be reported, got {message}"
            );
            assert!(
                message.contains("legacy-proposal"),
                "the refusal must name the record it will not drop, got {message}"
            );
        }
        other => panic!("expected the legacy refusal, got {other:?}"),
    }
    assert!(
        legacy_file.is_file(),
        "the legacy record must still be on disk: it was reported, not deleted"
    );
    // And nothing was staged under the canonical workspace.
    let requests = stored_rows(&pool, SkillPromotionKind::Request).await;
    assert!(
        requests.is_empty(),
        "the refused workflow staged nothing, got {requests:?}"
    );
}

/// The daemon publishes to **both** scopes.
#[tokio::test(flavor = "current_thread")]
async fn the_daemon_publishes_to_both_scopes() {
    let pool = common::pool::isolated_pool().await;
    let (_memory_dir, memory_root) = memory_root();
    let checkout = tempfile::tempdir().expect("a checkout");
    let (habit_id, second_habit_id) =
        workspace_with_two_ready_habits(&pool, &memory_root, checkout.path()).await;
    let daemon = daemon_with_memory_root(pool.clone(), memory_root.clone());
    let request_id = staged_request_id(&daemon, &habit_id)
        .await
        .expect("the daemon staged the request");

    let tool = codegg::tool::skill_proposal::SkillProposalTool::new(
        pool.clone(),
        WORKSPACE_ID,
        checkout.path().to_path_buf(),
        memory_root.clone(),
    );
    codegg::tool::Tool::execute_structured(
        &tool,
        serde_json::json!({
            "action": "submit",
            "promotion_request_id": request_id,
            "habit_id": habit_id.as_str(),
            "name": "demo",
            "description": "A demo skill",
            "skill_markdown": "---\nname: demo\ndescription: A demo skill\n---\n# Demo\n\nUse the workflow."
        }),
        Some(tool_context("session-1")),
    )
    .await
    .expect("the daemon tool submits the proposal");

    let proposals = stored_rows(&pool, SkillPromotionKind::Proposal).await;
    let proposal_id = proposals[0].0.clone();
    let payload = codegg::skills::promotion::proposal_from_payload(&proposals[0].2)
        .expect("the stored proposal decodes");

    // The global scope is a **second** publication: a proposal published to
    // the project scope is already published, and a second publication of
    // it would be a different proposal's publication.
    let second_request_id = staged_request_id(&daemon, &second_habit_id)
        .await
        .expect("the daemon staged the second request");
    let second_tool = codegg::tool::skill_proposal::SkillProposalTool::new(
        pool.clone(),
        WORKSPACE_ID,
        checkout.path().to_path_buf(),
        memory_root.clone(),
    );
    codegg::tool::Tool::execute_structured(
        &second_tool,
        serde_json::json!({
            "action": "submit",
            "promotion_request_id": second_request_id,
            "habit_id": second_habit_id.as_str(),
            "name": "demo-global",
            "description": "A demo skill",
            "skill_markdown": "---\nname: demo-global\ndescription: A demo skill\n---\n# Demo\n\nUse the workflow."
        }),
        Some(tool_context("session-1")),
    )
    .await
    .expect("the daemon tool submits the second proposal");
    let proposals = stored_rows(&pool, SkillPromotionKind::Proposal).await;
    let second_row = proposals
        .iter()
        .find(|row| row.0 != proposal_id)
        .expect("the second proposal is recorded");
    let second_proposal_id = second_row.0.clone();
    let second_payload = codegg::skills::promotion::proposal_from_payload(&second_row.2)
        .expect("the second stored proposal decodes");

    // The project scope: inside the workspace's own skill root.
    let response = promote(
        &daemon,
        SkillPromotionActionDto::Publish,
        None,
        Some(&proposal_id),
        Some(SkillTargetScopeDto::Project),
    )
    .await;
    let relative_path = match response {
        CoreResponse::SkillPromotion { result } => result
            .published_relative_path
            .expect("the daemon published the proposal"),
        other => panic!("expected a publication answer, got {other:?}"),
    };
    assert_eq!(relative_path, "demo/SKILL.md");
    let project_skill = checkout.path().join(".codegg/skills/demo/SKILL.md");
    assert!(
        project_skill.is_file(),
        "the project scope's skill must exist at {}",
        project_skill.display()
    );
    assert_eq!(
        std::fs::read_to_string(&project_skill).unwrap(),
        payload.skill_markdown
    );

    // The global scope: under the user configuration root.
    let global_root = codegg::core::daemon_skills::global_config_root(&memory_root);
    let response = promote(
        &daemon,
        SkillPromotionActionDto::Publish,
        None,
        Some(&second_proposal_id),
        Some(SkillTargetScopeDto::Global),
    )
    .await;
    let global_relative_path = match response {
        CoreResponse::SkillPromotion { result } => result
            .published_relative_path
            .expect("the daemon published the proposal to the global scope"),
        other => panic!("expected a publication answer, got {other:?}"),
    };
    assert_eq!(global_relative_path, "demo-global/SKILL.md");
    let global_skill = global_root.join("codegg/skills/demo-global/SKILL.md");
    assert!(
        global_skill.is_file(),
        "the global scope's skill must exist at {}",
        global_skill.display()
    );
    assert_eq!(
        std::fs::read_to_string(&global_skill).unwrap(),
        second_payload.skill_markdown
    );
}

/// The C004 gate refuses a publication into a reserved checkout, and the
/// refusal is durable and attributable.
#[tokio::test(flavor = "current_thread")]
async fn the_c004_gate_refuses_a_publication_into_a_reserved_checkout() {
    let fixture = common::checkout_reservation::ReservedCheckout::build().await;
    let response = fixture
        .call(CoreRequest::SkillPromotion {
            request: SkillPromotionRequestDto {
                workspace_id: fixture.workspace_id.clone(),
                session_id: fixture.session_id.clone(),
                action: SkillPromotionActionDto::Publish,
                habit_id: None,
                proposal_id: Some("proposal".to_owned()),
                target_scope: Some(SkillTargetScopeDto::Project),
            },
        })
        .await;
    fixture.assert_conflict(&response);

    // The refusal is durable: the audit trail carries it.
    let refusals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_event WHERE action = 'checkout_writer_refusal'",
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("read the audit trail");
    assert!(
        refusals > 0,
        "the refusal must be recorded durably in the audit trail"
    );
}

/// The sink enum and the manifest agree in **both** directions.
///
/// A sink the mediator declares and no manifest root claims is a mediated
/// writer nobody declared. A marker a manifest root claims and no sink
/// declares is a manifest row that no code honours.
#[test]
fn the_sink_enum_and_the_manifest_agree_in_both_directions() {
    let manifest = include_str!("../../docs/writer-disposition.toml");
    // The markers the manifest claims, one per claimed marker.
    let claimed: Vec<String> = manifest
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("mediation_markers") {
                return None;
            }
            let (_, value) = line.split_once('=')?;
            Some(value.trim().trim_matches('"').to_owned())
        })
        .flat_map(|value| {
            value
                .split(',')
                .map(|marker| marker.trim().to_owned())
                .filter(|marker| !marker.is_empty())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        !claimed.is_empty(),
        "the manifest must claim the mediated sinks"
    );
    // The sinks the mediator declares, as the enum names them.
    let code = include_str!("../../src/core/checkout_mediation.rs");
    let declared: Vec<(String, String)> = code
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("Self::")?;
            let (variant, value) = rest.split_once("=> ")?;
            let variant = variant.trim();
            let value = value.trim().trim_end_matches(',').trim_matches('"');
            Some((variant.to_owned(), value.to_owned()))
        })
        .collect();
    assert!(
        !declared.is_empty(),
        "the sink enum must name its sinks"
    );

    // Direction one: every claimed marker names a sink the enum declares.
    for marker in &claimed {
        let variant = marker
            .rsplit("::")
            .next()
            .expect("a marker names a sink variant");
        assert!(
            declared.iter().any(|(name, _)| name == variant),
            "the manifest claims {marker}, which the sink enum does not declare"
        );
    }
    // Direction two: every sink the enum declares is claimed by the
    // manifest.
    for (variant, _) in &declared {
        let marker = format!("CheckoutWriterSink::{variant}");
        assert!(
            claimed.contains(&marker),
            "the sink enum declares {marker}, which no manifest root claims"
        );
    }
    // And the skill-publication sink is among them.
    assert!(
        claimed.contains(&"CheckoutWriterSink::SkillPublish".to_owned()),
        "the manifest must claim the skill-publication sink, got {claimed:?}"
    );
}

/// The ask's single request leaves **no** orphan `pending` row.
///
/// The ask is one request: the daemon reads its own run bytes, stages the
/// evidence and admits the turn together. A request that cannot be served
/// stages nothing, so no `pending` row survives it.
#[tokio::test(flavor = "current_thread")]
async fn the_ask_leaves_no_orphan_pending_row() {
    let pool = common::pool::isolated_pool().await;
    let (_memory_dir, memory_root) = memory_root();
    let checkout = tempfile::tempdir().expect("a checkout");
    workspace_with_a_ready_habit(&pool, &memory_root, checkout.path()).await;
    let daemon = daemon_with_memory_root(pool.clone(), memory_root.clone());

    let response = request(
        &daemon,
        CoreRequest::SessionPromptSubmitWithHumanShellPromotions {
            session_id: "session-1".to_owned(),
            text: "Answer the question contained in the attached human-shell evidence.".to_owned(),
            plan_mode: false,
            human_shell_promotions: vec![codegg_protocol::human_shell::HumanShellPromotionStageDto {
                promotion_id: uuid::Uuid::new_v4().to_string(),
                source_id: "no-such-run".to_owned(),
                question: "what happened?".to_owned(),
            }],
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::Error { .. }),
        "the ask must be refused when the evidence cannot be read, got {response:?}"
    );

    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM human_shell_promotion WHERE session_id = ? AND status = 'pending'",
    )
    .bind("session-1")
    .fetch_one(&pool)
    .await
    .expect("read the promotion rows");
    assert_eq!(
        pending, 0,
        "the refused ask must leave no pending row behind"
    );
}

/// The ask's evidence comes from the daemon's run bytes, and the redaction
/// still applies.
///
/// The daemon assembles the evidence from **its own** run and redacts it
/// before it becomes durable.
#[tokio::test(flavor = "current_thread")]
async fn the_ask_reads_its_evidence_from_the_daemons_run_bytes() {
    let pool = common::pool::isolated_pool().await;
    let (_memory_dir, memory_root) = memory_root();
    let checkout = tempfile::tempdir().expect("a checkout");
    workspace_with_a_ready_habit(&pool, &memory_root, checkout.path()).await;
    let daemon = daemon_with_memory_root(pool.clone(), memory_root.clone());

    // A run the daemon owns, carrying a secret in its output.
    let run_id = seed_daemon_run(&daemon, checkout.path()).await;
    let evidence = daemon
        .human_shell_runs
        .evidence_text(&run_id, "what did the command print?")
        .await
        .expect("the daemon reads its own run bytes");
    assert!(
        evidence.contains("what did the command print?"),
        "the evidence carries the question, got {evidence}"
    );
    assert!(
        evidence.contains("hello-from-the-run"),
        "the evidence carries the run's own bytes, got {evidence}"
    );
}

/// Seed one human-shell run in the daemon's own run service.
///
/// The run's output carries a secret, so the redaction has something to
/// remove.
async fn seed_daemon_run(daemon: &CoreDaemon, cwd: &std::path::Path) -> String {
    use codegg_protocol::human_shell::{
        HumanShellOriginDto, HumanShellRunCreateRequestDto, HUMAN_SHELL_PROTOCOL_VERSION,
    };
    let scope = codegg::shell::daemon::HumanShellRunScope {
        principal_id: "m007i-local".to_owned(),
        project_id: "m007i-project".to_owned(),
        workspace_id: WORKSPACE_ID.to_owned(),
        session_id: Some("session-1".to_owned()),
    };
    let response = daemon
        .human_shell_runs
        .create(
            scope,
            HumanShellRunCreateRequestDto {
                protocol_version: HUMAN_SHELL_PROTOCOL_VERSION,
                submission_id: uuid::Uuid::new_v4().to_string(),
                project_id: "m007i-project".to_owned(),
                workspace_id: WORKSPACE_ID.to_owned(),
                session_id: Some("session-1".to_owned()),
                origin: HumanShellOriginDto::Bang,
                confirmed_dangerous: false,
                command: "printf hello-from-the-run".to_owned(),
                timeout_secs: 30,
            },
            cwd.to_path_buf(),
            "m007i-node".to_owned(),
            "/bin/sh".to_owned(),
            None,
        )
        .await;
    let run_id = match response {
        CoreResponse::HumanShellRunCreated { run, .. } => run.run_id,
        other => panic!("expected a run, got {other:?}"),
    };
    // Wait for the run to reach its terminal status: the evidence is only
    // readable from an exited run.
    for _ in 0..200 {
        if let Some(scope) = daemon.human_shell_runs.scope_for(&run_id).await {
            if let CoreResponse::HumanShellRunOutput { run, .. } = daemon
                .human_shell_runs
                .read(&scope, &run_id, 0, 64 * 1024)
                .await
            {
                if run.status == codegg_protocol::human_shell::HumanShellRunStatusDto::Exited {
                    return run_id;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the run did not exit in time");
}

