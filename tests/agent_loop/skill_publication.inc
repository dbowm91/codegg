//! M007-I (work package C): the daemon-owned publication.
//!
//! The publication is daemon-owned and **store-free**: the caller holds the
//! exact proposal revision it approved, and the service performs the
//! filesystem effects and returns the proposal the record must become.
//!
//! These tests exercise the service directly, because the service is where
//! the filesystem authority lives. The daemon's own handler — the one that
//! owns the record and the C004 gate — is exercised in
//! `tests/core_daemon/m007i_daemon_promotion.inc`.

use codegg::memory::habit::{
    HabitCandidateStatus, HabitId, HabitStore, WorkflowAction, WorkflowActionKind,
    WorkflowEffectClass, WorkflowOccurrence, WorkflowOutcome,
};
use codegg::memory::project_namespace;
use codegg::skills::promotion::{
    build_validated_proposal, HabitPromotionContext, PromotionDraftRequest, PromotionRequestId,
    SkillProposal, SkillProposalStatus, SkillProposalSubmission, SkillTargetScope,
};
use codegg::skills::publish::{
    SkillPublicationError, SkillPublicationRequest, SkillPublicationService,
};
use codegg::skills::{AssetDiscoveryConfig, AssetRegistry, SourceKind};
use std::fs;
use tempfile::tempdir;

const DEMO_MARKDOWN: &str =
    "---\nname: demo\ndescription: A demo skill\n---\n# Demo\n\nUse the workflow.";

/// A ready habit candidate and the proposal the workflow produced from it.
fn ready_fixture() -> (tempfile::TempDir, String, HabitId, SkillProposal) {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(dir.path().join("config")).unwrap();
    let identity = project.to_string_lossy().to_string();
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    let namespace = project_namespace(&identity);
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
    for (session, turn) in [("s1", "t1"), ("s1", "t2"), ("s2", "t3")] {
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
    let habit_id = habits.load(&identity).unwrap().remove(0).id;
    let proposal = validated_proposal(&habit_id, &identity);
    (dir, identity, habit_id, proposal)
}

/// The proposal the workflow produces from a ready habit.
///
/// The workflow is a pure function of the staged request and the model's
/// draft, so the fixture builds the request the daemon would have staged.
fn validated_proposal(habit_id: &HabitId, workspace_id: &str) -> SkillProposal {
    let request_id = PromotionRequestId::new();
    let request = PromotionDraftRequest {
        id: request_id.clone(),
        session_id: "session".to_owned(),
        workspace_id: workspace_id.to_owned(),
        habit_id: habit_id.clone(),
        habit_fingerprint: "fingerprint".to_owned(),
        candidate_revision: 3,
        context: HabitPromotionContext {
            habit_id: habit_id.clone(),
            workflow_fingerprint: "fingerprint".to_owned(),
            action_skeleton: Vec::new(),
            successful_occurrences: 3,
            distinct_sessions: 2,
            related_memories: Vec::new(),
            existing_skill_names: Vec::new(),
            target_scope_hint: SkillTargetScope::Project,
        },
        created_at: 10,
        expires_at: 10 + 15 * 60 * 1000,
        consumed: false,
    };
    build_validated_proposal(
        &request,
        &SkillProposalSubmission {
            request_id: &request_id,
            habit_id,
            supplied_name: "demo",
            supplied_description: "A demo skill",
            skill_markdown: DEMO_MARKDOWN,
            now: 11,
        },
    )
    .unwrap()
}

fn request(proposal: &SkillProposal, target_scope: SkillTargetScope) -> SkillPublicationRequest {
    SkillPublicationRequest {
        proposal_id: proposal.id.clone(),
        expected_revision: proposal.revision,
        expected_content_digest: proposal.content_digest.clone(),
        target_scope,
    }
}

fn service(dir: &tempfile::TempDir) -> SkillPublicationService {
    SkillPublicationService::new(dir.path().join("memory"))
}

#[test]
fn publication_lock_contents_are_preserved() {
    let (dir, identity, _habit_id, proposal) = ready_fixture();
    let lock_root = dir.path().join("project/.codegg/skills");
    fs::create_dir_all(&lock_root).unwrap();
    let lock_path = lock_root.join(".codegg-skill-publish.lock");
    let marker = b"advisory-lock-owner";
    fs::write(&lock_path, marker).unwrap();

    service(&dir)
        .publish(
            &identity,
            &proposal,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap();

    assert_eq!(fs::read(lock_path).unwrap(), marker);
}

#[test]
fn project_publication_is_atomic_provenance_bound_and_promotes_habit() {
    let (dir, identity, habit_id, proposal) = ready_fixture();
    let expected_digest =
        codegg::skills::promotion::compute_content_digest(&proposal.skill_markdown);
    let result = service(&dir)
        .publish(
            &identity,
            &proposal,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap();

    let path = dir.path().join("project/.codegg/skills/demo/SKILL.md");
    assert_eq!(fs::read_to_string(&path).unwrap(), proposal.skill_markdown);
    assert_eq!(result.content_digest, expected_digest);
    assert_eq!(result.proposal.status, SkillProposalStatus::Published);
    assert_eq!(
        result.proposal.publication.as_ref().unwrap().relative_path,
        "demo/SKILL.md"
    );
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    let candidate = habits
        .load(&identity)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == habit_id)
        .unwrap();
    assert_eq!(candidate.status, HabitCandidateStatus::Promoted);

    let registry = AssetRegistry::build(
        &AssetDiscoveryConfig::default(),
        std::path::Path::new(&identity),
        &[],
    );
    assert_eq!(
        registry.get("demo").unwrap().source_kind,
        SourceKind::CodeGGProject
    );
}

#[test]
fn publication_retry_is_idempotent_after_metadata_commit() {
    let (dir, identity, _habit_id, proposal) = ready_fixture();
    let first = service(&dir)
        .publish(
            &identity,
            &proposal,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap();
    // The record the daemon holds after the first publication.
    let published = first.proposal;
    let retry = service(&dir)
        .publish(
            &identity,
            &published,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&published, SkillTargetScope::Project),
            21,
        )
        .unwrap();
    assert!(retry.idempotent);
    assert_eq!(retry.proposal.revision, published.revision);
}

#[test]
fn reconcile_completes_metadata_without_rewriting_exact_file() {
    let (dir, identity, _habit_id, proposal) = ready_fixture();
    let path = dir.path().join("project/.codegg/skills/demo/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &proposal.skill_markdown).unwrap();
    let before = fs::read(&path).unwrap();
    let result = service(&dir)
        .reconcile(
            &identity,
            &proposal,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap();
    assert!(result.reconciled);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(result.proposal.status, SkillProposalStatus::Published);
}

#[test]
fn stale_revision_is_rejected_and_different_content_is_never_overwritten() {
    let (dir, identity, _habit_id, proposal) = ready_fixture();
    let stale = request(&proposal, SkillTargetScope::Project);
    // The record moved underneath the approval: a review note bumped the
    // revision, so the captured approval is stale.
    let mut fresh = proposal.clone();
    fresh.revision = proposal.revision + 1;
    let error = service(&dir)
        .publish(
            &identity,
            &fresh,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            stale,
            20,
        )
        .unwrap_err();
    assert!(matches!(error, SkillPublicationError::StaleApproval));

    let path = dir.path().join("project/.codegg/skills/demo/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "---\nname: demo\ndescription: another\n---\nnot the proposal",
    )
    .unwrap();
    let collision = service(&dir)
        .publish(
            &identity,
            &fresh,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&fresh, SkillTargetScope::Project),
            21,
        )
        .unwrap_err();
    assert!(matches!(
        collision,
        SkillPublicationError::SkillAlreadyExists(_)
    ));
    assert!(fs::read_to_string(path).unwrap().contains("another"));
}

#[test]
fn global_scope_is_host_derived_and_foreign_precedence_requires_preview() {
    let (dir, identity, _habit_id, proposal) = ready_fixture();
    let foreign = dir.path().join("project/.agents/skills/demo");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join("SKILL.md"),
        "---\nname: demo\ndescription: foreign\n---\nforeign",
    )
    .unwrap();
    let error = service(&dir)
        .publish(
            &identity,
            &proposal,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        SkillPublicationError::PrecedenceChangedSincePreview(_)
    ));
    assert!(!dir
        .path()
        .join("project/.codegg/skills/demo/SKILL.md")
        .exists());

    // The human previewed the proposal's current revision.
    let mut previewed = proposal.clone();
    previewed.previewed_revision = Some(previewed.revision);

    let result = service(&dir)
        .publish(
            &identity,
            &previewed,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&previewed, SkillTargetScope::Global),
            22,
        )
        .unwrap();
    assert_eq!(
        result.shadowed_by.as_deref(),
        Some("demo from AgentsProject")
    );
    assert!(dir
        .path()
        .join("config/codegg/skills/demo/SKILL.md")
        .exists());
    assert!(!dir
        .path()
        .join("project/.agents/skills/demo/SKILL.md")
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn symlinked_owned_root_is_rejected() {
    use std::os::unix::fs::symlink;
    let (dir, identity, _habit_id, proposal) = ready_fixture();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let codegg = dir.path().join("project/.codegg");
    fs::create_dir_all(codegg.parent().unwrap()).unwrap();
    symlink(&outside, &codegg).unwrap();
    let error = service(&dir)
        .publish(
            &identity,
            &proposal,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap_err();
    assert!(matches!(error, SkillPublicationError::UnsafeTarget(_)));
    assert!(!outside.join("skills/demo/SKILL.md").exists());
}
