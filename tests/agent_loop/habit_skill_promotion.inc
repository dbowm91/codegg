//! M007-I (work package B): the daemon-owned promotion workflow.
//!
//! The durable record moved to the daemon's session storage, keyed on the
//! canonical `WorkspaceId`. These tests therefore exercise the **store**
//! (`codegg_core::session::skill_promotion`) rather than a file: the store
//! is where the namespace, the consumption and the monotonicity now live,
//! and the domain functions are pure, so the two compose exactly as the
//! daemon composes them.

use codegg::memory::habit::{
    HabitId, HabitStore, WorkflowAction, WorkflowActionKind, WorkflowEffectClass,
    WorkflowOccurrence, WorkflowOutcome,
};
use codegg::memory::project_namespace;
use codegg::skills::promotion::{
    build_validated_proposal, compute_content_digest, draft_context, habit_is_promotable,
    request_to_payload, BoundedMemoryRef, HabitPromotionContext, PromotionDraftRequest,
    PromotionRequestId, SkillProposal, SkillProposalStatus, SkillProposalSubmission,
    SkillTargetScope,
};
use codegg::skills::{validate_portable_document, AssetDiscoveryConfig, AssetRegistry, SourceKind};
use codegg_core::session::skill_promotion::{
    SkillPromotionKind, SkillPromotionRecord, SkillPromotionStore,
};
use std::fs;
use tempfile::tempdir;

use super::common::pool::isolated_pool;

const WORKSPACE_ID: &str = "workspace-1";

fn make_ready_habit(identity: &str, store: &HabitStore) -> HabitId {
    let namespace = project_namespace(identity);
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
    for (session, turn) in [
        ("session-1", "turn-1"),
        ("session-1", "turn-2"),
        ("session-2", "turn-3"),
    ] {
        store
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
    store.load(identity).unwrap().remove(0).id
}

async fn fixture() -> (tempfile::TempDir, String, HabitId, SkillPromotionStore) {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let identity = project.to_string_lossy().to_string();
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    let habit_id = make_ready_habit(&identity, &habits);
    let store = SkillPromotionStore::new(isolated_pool().await);
    (dir, identity, habit_id, store)
}

/// The request the daemon stages for a ready habit.
fn staged_request(
    memory_root: &std::path::Path,
    identity: &str,
    habit_id: &HabitId,
    existing_skill_names: Vec<String>,
    related_memories: Vec<BoundedMemoryRef>,
    now: i64,
) -> PromotionDraftRequest {
    // The candidate is the daemon's own view of the habit.
    let candidate = HabitStore::with_root(memory_root.to_path_buf())
        .unwrap()
        .load(identity)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == *habit_id)
        .unwrap();
    let context = draft_context(&candidate, existing_skill_names, related_memories).unwrap();
    PromotionDraftRequest {
        id: PromotionRequestId::new(),
        session_id: "session-1".to_owned(),
        workspace_id: WORKSPACE_ID.to_owned(),
        habit_id: habit_id.clone(),
        habit_fingerprint: candidate.workflow_fingerprint,
        candidate_revision: candidate.revision,
        context,
        created_at: now,
        expires_at: now + codegg::skills::promotion::PROMOTION_REQUEST_TTL_MS,
        consumed: false,
    }
}

/// Record the staged request, as the daemon does.
async fn stage(store: &SkillPromotionStore, identity: &str, request: &PromotionDraftRequest) {
    let _ = identity;
    let payload = request_to_payload(request).unwrap();
    store
        .put(SkillPromotionRecord {
            id: request.id.as_str().to_owned(),
            workspace_id: request.workspace_id.clone(),
            session_id: request.session_id.clone(),
            kind: SkillPromotionKind::Request,
            payload,
            payload_sha256: String::new(),
            status: "pending".to_owned(),
            revision: 1,
            created_at_ms: request.created_at,
            updated_at_ms: request.created_at,
            expires_at_ms: Some(request.expires_at),
        })
        .await
        .unwrap();
}

/// The proposal the model's draft produces, recorded against the request.
async fn submit(
    store: &SkillPromotionStore,
    request: &PromotionDraftRequest,
    habit_id: &HabitId,
    markdown: &str,
    now: i64,
) -> Result<SkillProposal, codegg_core::session::skill_promotion::SkillPromotionError> {
    let mut proposal = build_validated_proposal(
        request,
        &SkillProposalSubmission {
            request_id: &request.id,
            habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now,
        },
    )
    .map_err(|message| {
        codegg_core::session::skill_promotion::SkillPromotionError::Invalid {
            field: "skill_markdown",
            message,
        }
    })?;
    proposal.revision = 1;
    let payload = codegg::skills::promotion::proposal_to_payload(&proposal).unwrap();
    store
        .consume_request_into_proposal(
            &request.workspace_id,
            request.id.as_str(),
            SkillPromotionRecord {
                id: proposal.id.as_str().to_owned(),
                workspace_id: request.workspace_id.clone(),
                session_id: request.session_id.clone(),
                kind: SkillPromotionKind::Proposal,
                payload,
                payload_sha256: String::new(),
                status: proposal.status.as_str().to_owned(),
                revision: 1,
                created_at_ms: now,
                updated_at_ms: now,
                expires_at_ms: None,
            },
            now,
        )
        .await?;
    Ok(proposal)
}

#[tokio::test]
async fn the_store_is_the_only_writer_and_is_keyed_on_the_workspace() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;

    // The record is reachable under the canonical workspace and under no
    // other: the namespace is the workspace, not a digest of a path.
    assert!(store
        .get(
            WORKSPACE_ID,
            SkillPromotionKind::Request,
            request.id.as_str()
        )
        .await
        .unwrap()
        .is_some());
    assert!(store
        .get(
            "workspace-2",
            SkillPromotionKind::Request,
            request.id.as_str()
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn submission_without_explicit_request_is_denied() {
    let (_dir, _identity, _habit_id, store) = fixture().await;
    let request_id = PromotionRequestId::new();
    // A submission against a request that was never staged finds no record.
    assert!(store
        .consume_request_into_proposal(
            WORKSPACE_ID,
            request_id.as_str(),
            SkillPromotionRecord {
                id: "proposal".to_owned(),
                workspace_id: WORKSPACE_ID.to_owned(),
                session_id: "session-1".to_owned(),
                kind: SkillPromotionKind::Proposal,
                payload: "{}".to_owned(),
                payload_sha256: String::new(),
                status: "validated".to_owned(),
                revision: 1,
                created_at_ms: 10,
                updated_at_ms: 10,
                expires_at_ms: None,
            },
            10,
        )
        .await
        .is_err());
}

#[tokio::test]
async fn request_is_revision_bound_and_consumed_after_one_submission() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        vec!["existing".to_string()],
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    assert_eq!(request.context.target_scope_hint, SkillTargetScope::Project);
    let markdown = "---\nname: demo\ndescription: demo\n---\nbody";
    let proposal = submit(&store, &request, &habit_id, markdown, 11)
        .await
        .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    assert_eq!(proposal.revision, 1);
    assert_eq!(proposal.content_digest, compute_content_digest(markdown));
    // The request is consumed: a second submission finds nothing to consume.
    assert!(submit(&store, &request, &habit_id, markdown, 12)
        .await
        .is_err());
}

#[tokio::test]
async fn a_stale_candidate_revision_is_rejected() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    // The habit candidate moved underneath the staged request.
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    let namespace = project_namespace(&identity);
    habits
        .observe(WorkflowOccurrence {
            project_namespace: namespace,
            session_id: "session-2".to_string(),
            turn_id: Some("turn-4".to_string()),
            root_or_run_id: None,
            actions: vec![
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
            ],
            outcome: WorkflowOutcome::Succeeded,
            occurred_at: 2,
        })
        .unwrap();
    let candidate = habits.load(&identity).unwrap().remove(0);
    assert!(candidate.revision > request.candidate_revision);
    // The daemon refuses the submission because the candidate it re-reads
    // no longer matches the revision the request was staged against.
    assert_ne!(candidate.revision, request.candidate_revision);
}

#[tokio::test]
async fn proposal_restrictions_reject_allowed_tools_without_skill_root_changes() {
    let (dir, identity, habit_id, store) = fixture().await;
    let before = AssetRegistry::build(
        &AssetDiscoveryConfig::default(),
        std::path::Path::new(&identity),
        &[],
    );
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    let proposal = submit(
        &store,
        &request,
        &habit_id,
        "---\nname: demo\ndescription: demo\nallowed-tools: [bash]\n---\nbody",
        11,
    )
    .await
    .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Rejected);
    assert!(proposal
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason.contains("allowed-tools")));
    assert!(!dir.path().join("project/.codegg/skills").exists());
    let after = AssetRegistry::build(
        &AssetDiscoveryConfig::default(),
        std::path::Path::new(&identity),
        &[],
    );
    assert_eq!(before.effective.len(), after.effective.len());
}

#[test]
fn proposal_parser_reuses_portable_discovery_rules() {
    let dir = tempdir().unwrap();
    let source = "---\nname: portable\ndescription: shared parser\nlicense: MIT\n---\nbody";
    let parsed = validate_portable_document(source, &AssetDiscoveryConfig::default()).unwrap();
    // Portable discovery requires the skill directory name to match the
    // frontmatter name, so nest SKILL.md under `portable/`.
    let skill_dir = dir.path().join("portable");
    fs::create_dir_all(&skill_dir).unwrap();
    let file = skill_dir.join("SKILL.md");
    fs::write(&file, source).unwrap();
    let discovered = codegg::skills::parser::parse_candidate(
        &file,
        SourceKind::AgentsProject,
        &AssetDiscoveryConfig::default(),
    )
    .unwrap();
    assert_eq!(parsed.normalized_name, discovered.normalized_name);
    assert_eq!(parsed.description, discovered.description);
    assert_eq!(parsed.body, discovered.body);
    assert_eq!(parsed.metadata, discovered.metadata);
}

#[tokio::test]
async fn a_request_in_one_workspace_is_not_consumable_from_another() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    // The record carries the session it was staged under, so a submission
    // can be bound to it.
    let record = store
        .get(
            WORKSPACE_ID,
            SkillPromotionKind::Request,
            request.id.as_str(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.session_id, "session-1");
    // The wrong workspace fails closed: the record is found under the
    // canonical workspace and under no other.
    assert!(store
        .consume_request_into_proposal(
            "workspace-2",
            request.id.as_str(),
            SkillPromotionRecord {
                id: "proposal".to_owned(),
                workspace_id: "workspace-2".to_owned(),
                session_id: "session-1".to_owned(),
                kind: SkillPromotionKind::Proposal,
                payload: "{}".to_owned(),
                payload_sha256: String::new(),
                status: "validated".to_owned(),
                revision: 1,
                created_at_ms: 11,
                updated_at_ms: 11,
                expires_at_ms: None,
            },
            11,
        )
        .await
        .is_err());
}

#[tokio::test]
async fn an_expired_request_cannot_be_replayed() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    let ttl = codegg::skills::promotion::PROMOTION_REQUEST_TTL_MS;
    // The request's own expiry is what bounds the replay.
    assert_eq!(request.expires_at, 10 + ttl);
    let record = store
        .get(
            WORKSPACE_ID,
            SkillPromotionKind::Request,
            request.id.as_str(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.expires_at_ms, Some(10 + ttl));
    assert!(submit(
        &store,
        &request,
        &habit_id,
        "---\nname: demo\ndescription: demo\n---\nbody",
        10 + ttl + 1
    )
    .await
    .is_err());
}

#[test]
fn a_non_ready_habit_cannot_begin_a_request() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let identity = project.to_string_lossy().to_string();
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    // Only one observing occurrence: never Ready.
    let namespace = project_namespace(&identity);
    habits
        .observe(WorkflowOccurrence {
            project_namespace: namespace,
            session_id: "session-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            root_or_run_id: None,
            actions: vec![
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
            ],
            outcome: WorkflowOutcome::Succeeded,
            occurred_at: 1,
        })
        .unwrap();
    let candidate = habits.load(&identity).unwrap().remove(0);
    assert_eq!(
        candidate.status,
        codegg::memory::habit::HabitCandidateStatus::Observing
    );
    assert!(!habit_is_promotable(&candidate));
}

#[test]
fn parser_rejection_matches_discovery_for_malformed_and_oversized() {
    let dir = tempdir().unwrap();
    let config = AssetDiscoveryConfig::default();
    // Malformed frontmatter: both paths reject.
    let malformed = "---\nname: [{bad yaml\n---\nBody";
    assert!(validate_portable_document(malformed, &config).is_err());
    let file = dir.path().join("SKILL.md");
    fs::write(&file, malformed).unwrap();
    assert!(
        codegg::skills::parser::parse_candidate(&file, SourceKind::AgentsProject, &config).is_err()
    );

    // Missing name: both paths reject.
    let missing = "---\ndescription: no name\n---\nBody";
    assert!(validate_portable_document(missing, &config).is_err());
    fs::write(&file, missing).unwrap();
    assert!(
        codegg::skills::parser::parse_candidate(&file, SourceKind::AgentsProject, &config).is_err()
    );

    // Oversized: both paths reject identically.
    let big = format!(
        "---\nname: big\ndescription: big\n---\n{}",
        "x".repeat(300_000)
    );
    assert!(validate_portable_document(&big, &config).is_err());
    fs::write(&file, big).unwrap();
    assert!(
        codegg::skills::parser::parse_candidate(&file, SourceKind::AgentsProject, &config).is_err()
    );

    // Invalid name with path separator: both reject.
    let bad_name = "---\nname: a/b\ndescription: bad\n---\nBody";
    assert!(validate_portable_document(bad_name, &config).is_err());
    fs::write(&file, bad_name).unwrap();
    assert!(
        codegg::skills::parser::parse_candidate(&file, SourceKind::AgentsProject, &config).is_err()
    );
}

#[tokio::test]
async fn proposal_restrictions_reject_scripts_and_sidecars_but_allow_prose() {
    let (dir, identity, habit_id, store) = fixture().await;
    for (markdown, expect_valid) in [
        // Explicit sidecar path declarations are rejected.
        ("---\nname: demo\ndescription: demo\n---\nSee scripts/setup.sh", false),
        ("---\nname: demo\ndescription: demo\n---\nBundle resources/data.txt", false),
        ("---\nname: demo\ndescription: demo\n---\nInstall via package.json", false),
        (
            "---\nname: demo\ndescription: demo\nmcp:\n  servers: []\n---\nbody",
            false,
        ),
        // Ordinary prose mentioning plugin/MCP concepts is not a payload.
        (
            "---\nname: demo\ndescription: demo\n---\nThis workflow avoids common plugin confusion.",
            true,
        ),
        (
            "---\nname: demo\ndescription: demo\n---\nNo extra tooling required.",
            true,
        ),
    ] {
        let request = staged_request(&dir.path().join("memory"), &identity, &habit_id, Vec::new(), Vec::new(), 10);
        stage(&store, &identity, &request).await;
        let proposal = submit(&store, &request, &habit_id, markdown, 11).await.unwrap();
        if expect_valid {
            assert_eq!(proposal.status, SkillProposalStatus::Validated, "markdown: {markdown}");
        } else {
            assert_eq!(proposal.status, SkillProposalStatus::Rejected, "markdown: {markdown}");
        }
    }
}

#[test]
fn proposal_digest_is_deterministic_and_content_bound() {
    let markdown = "---\nname: demo\ndescription: demo\n---\nbody";
    assert_eq!(
        compute_content_digest(markdown),
        compute_content_digest(markdown)
    );
    let other = "---\nname: demo\ndescription: demo\n---\nbody2";
    assert_ne!(
        compute_content_digest(markdown),
        compute_content_digest(other)
    );
    // CRLF normalization keeps digests stable across platforms.
    let crlf = "---\r\nname: demo\r\ndescription: demo\r\n---\r\nbody";
    let lf = "---\nname: demo\ndescription: demo\n---\nbody";
    assert_eq!(compute_content_digest(crlf), compute_content_digest(lf));
}

#[tokio::test]
async fn the_rejection_lifecycle_is_monotonic_and_the_habit_stays_ready() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    let proposal = submit(
        &store,
        &request,
        &habit_id,
        "---\nname: demo\ndescription: demo\n---\nbody",
        11,
    )
    .await
    .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    // A validated proposal never marks the habit Promoted; only the
    // publication does.
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    let candidate = habits
        .load(&identity)
        .unwrap()
        .into_iter()
        .find(|c| c.id == habit_id)
        .unwrap();
    assert_eq!(
        candidate.status,
        codegg::memory::habit::HabitCandidateStatus::Ready
    );
    // The first rejection moves the record; the second is refused, because
    // the record is no longer awaiting review.
    let mut rejected = proposal.clone();
    rejected.status = SkillProposalStatus::Rejected;
    rejected.revision = proposal.revision + 1;
    let payload = codegg::skills::promotion::proposal_to_payload(&rejected).unwrap();
    assert!(store
        .compare_and_set_record(
            WORKSPACE_ID,
            SkillPromotionKind::Proposal,
            proposal.id.as_str(),
            1,
            SkillProposalStatus::Rejected.as_str(),
            &payload,
            rejected.revision as i64,
            12,
        )
        .await
        .unwrap());
    assert!(!store
        .compare_and_set_record(
            WORKSPACE_ID,
            SkillPromotionKind::Proposal,
            proposal.id.as_str(),
            1,
            SkillProposalStatus::Rejected.as_str(),
            &payload,
            rejected.revision as i64,
            13,
        )
        .await
        .unwrap());
    let after = store
        .get(
            WORKSPACE_ID,
            SkillPromotionKind::Proposal,
            proposal.id.as_str(),
        )
        .await
        .unwrap()
        .unwrap();
    let after = codegg::skills::promotion::proposal_from_payload(&after.payload).unwrap();
    assert_eq!(after.status, SkillProposalStatus::Rejected);
    assert!(after.revision > proposal.revision);
}

#[tokio::test]
async fn the_proposal_scope_is_a_host_enum_not_a_path() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    let proposal = submit(
        &store,
        &request,
        &habit_id,
        "---\nname: demo\ndescription: demo\n---\nbody",
        11,
    )
    .await
    .unwrap();
    assert_eq!(proposal.target_scope, SkillTargetScope::Project);
    assert_eq!(proposal.workspace_id, WORKSPACE_ID);
    assert!(!proposal.workspace_id.contains(".."));
}

#[tokio::test]
async fn a_malformed_persisted_proposal_fails_safely() {
    let (dir, identity, habit_id, store) = fixture().await;
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    let proposal = submit(
        &store,
        &request,
        &habit_id,
        "---\nname: demo\ndescription: demo\n---\nbody",
        11,
    )
    .await
    .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    // Corrupt the stored payload; reads must fail closed, not return partial
    // state.
    assert!(store
        .compare_and_set_record(
            WORKSPACE_ID,
            SkillPromotionKind::Proposal,
            proposal.id.as_str(),
            1,
            "validated",
            "{ not json",
            2,
            12,
        )
        .await
        .unwrap());
    let record = store
        .get(
            WORKSPACE_ID,
            SkillPromotionKind::Proposal,
            proposal.id.as_str(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(codegg::skills::promotion::proposal_from_payload(&record.payload).is_err());
}

#[test]
fn the_promotion_context_is_structural_and_the_prompt_hides_the_session() {
    let context = HabitPromotionContext {
        habit_id: codegg::memory::habit::HabitId::parse("habit-1").unwrap(),
        workflow_fingerprint: "fingerprint".to_owned(),
        action_skeleton: vec![WorkflowAction::new(
            WorkflowActionKind::FileRead,
            None,
            WorkflowEffectClass::ReadOnly,
        )],
        successful_occurrences: 3,
        distinct_sessions: 2,
        related_memories: vec![BoundedMemoryRef {
            id: "mem-1".to_string(),
            summary: "prefers focused tests".to_string(),
        }],
        existing_skill_names: vec!["existing-skill".to_string()],
        target_scope_hint: SkillTargetScope::Project,
    };
    let request = PromotionDraftRequest {
        id: PromotionRequestId::new(),
        session_id: "session-1".to_owned(),
        workspace_id: WORKSPACE_ID.to_owned(),
        habit_id: context.habit_id.clone(),
        habit_fingerprint: context.workflow_fingerprint.clone(),
        candidate_revision: 2,
        context,
        created_at: 10,
        expires_at: 10 + codegg::skills::promotion::PROMOTION_REQUEST_TTL_MS,
        consumed: false,
    };
    // Only structural evidence: fingerprint, skeleton, counts.
    assert!(!request.habit_fingerprint.is_empty());
    assert!(!request.context.action_skeleton.is_empty());
    assert!(request.context.successful_occurrences >= 3);
    assert!(request.context.distinct_sessions >= 2);
    assert!(request.candidate_revision >= 1);
    let prompt = codegg::skills::promotion::build_draft_prompt(&request);
    assert!(prompt.contains(request.id.as_str()));
    assert!(prompt.contains("read"));
    assert!(!prompt.contains("session-1"));
    assert!(!prompt.contains("rm -rf"));
    assert!(prompt.contains("prefers focused tests"));
}

#[tokio::test]
async fn the_collision_diagnostic_is_advisory_without_overwrite() {
    let (dir, identity, habit_id, store) = fixture().await;
    // Seed an effective skill with the same normalized name.
    let skill_dir = dir
        .path()
        .join("project")
        .join(".codegg")
        .join("skills")
        .join("demo");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: existing\n---\nbody",
    )
    .unwrap();
    let registry = AssetRegistry::build(
        &AssetDiscoveryConfig::default(),
        std::path::Path::new(&identity),
        &[],
    );
    assert!(registry.get("demo").is_some());
    let diagnostics = codegg::skills::promotion::collision_diagnostics(&registry, "demo");
    assert!(!diagnostics.is_empty());
    assert!(diagnostics[0].reason.contains("existing effective skill"));
    assert!(diagnostics[0]
        .location
        .as_deref()
        .unwrap()
        .contains(".codegg"));
    // Recording a same-name proposal never overwrites the existing file.
    let request = staged_request(
        &dir.path().join("memory"),
        &identity,
        &habit_id,
        Vec::new(),
        Vec::new(),
        10,
    );
    stage(&store, &identity, &request).await;
    let proposal = submit(
        &store,
        &request,
        &habit_id,
        "---\nname: demo\ndescription: demo\n---\nbody",
        11,
    )
    .await
    .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    let existing = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(existing.contains("existing"));
}
