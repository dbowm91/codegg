use codegg::memory::habit::{
    HabitCandidateStatus, HabitId, HabitStore, WorkflowAction, WorkflowActionKind,
    WorkflowEffectClass, WorkflowOccurrence, WorkflowOutcome,
};
use codegg::memory::project_namespace;
use codegg::skills::promotion::{
    compute_content_digest, SkillPromotionStore, SkillProposalStatus, SkillProposalSubmission,
    SkillTargetScope,
};
use codegg::skills::publish::{
    SkillPublicationError, SkillPublicationRequest, SkillPublicationService,
};
use codegg::skills::{AssetDiscoveryConfig, AssetRegistry, SourceKind};
use std::fs;
use tempfile::tempdir;

fn ready_fixture() -> (tempfile::TempDir, String, HabitId, SkillPromotionStore) {
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
    let promotions =
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap();
    (dir, identity, habit_id, promotions)
}

fn proposal(
    identity: &str,
    habit_id: &HabitId,
    store: &SkillPromotionStore,
) -> codegg::skills::promotion::SkillProposal {
    let request = store
        .begin_request(identity, "session", habit_id, Vec::new(), Vec::new(), 10)
        .unwrap();
    store
        .submit(SkillProposalSubmission {
            project_identity: identity,
            session_id: "session",
            request_id: &request.id,
            habit_id,
            supplied_name: "demo",
            supplied_description: "A demo skill",
            skill_markdown:
                "---\nname: demo\ndescription: A demo skill\n---\n# Demo\n\nUse the workflow.",
            now: 11,
        })
        .unwrap()
}

fn request(
    proposal: &codegg::skills::promotion::SkillProposal,
    target_scope: SkillTargetScope,
) -> SkillPublicationRequest {
    SkillPublicationRequest {
        proposal_id: proposal.id.clone(),
        expected_revision: proposal.revision,
        expected_content_digest: proposal.content_digest.clone(),
        target_scope,
    }
}

#[test]
fn publication_lock_contents_are_preserved() {
    let (dir, identity, habit_id, store) = ready_fixture();
    let proposal = proposal(&identity, &habit_id, &store);
    let lock_root = dir.path().join("project/.codegg/skills");
    fs::create_dir_all(&lock_root).unwrap();
    let lock_path = lock_root.join(".codegg-skill-publish.lock");
    let marker = b"advisory-lock-owner";
    fs::write(&lock_path, marker).unwrap();

    let service = SkillPublicationService::new(
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap(),
    );
    service
        .publish(
            &identity,
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
    let (dir, identity, habit_id, store) = ready_fixture();
    let proposal = proposal(&identity, &habit_id, &store);
    let expected_digest = compute_content_digest(&proposal.skill_markdown);
    let service = SkillPublicationService::new(
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap(),
    );
    let result = service
        .publish(
            &identity,
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
    let (dir, identity, habit_id, store) = ready_fixture();
    let proposal = proposal(&identity, &habit_id, &store);
    let service = SkillPublicationService::new(
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap(),
    );
    service
        .publish(
            &identity,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap();
    let published = store
        .get_proposal(&identity, &proposal.id)
        .unwrap()
        .unwrap();
    let retry = service
        .publish(
            &identity,
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
    let (dir, identity, habit_id, store) = ready_fixture();
    let proposal = proposal(&identity, &habit_id, &store);
    let path = dir.path().join("project/.codegg/skills/demo/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &proposal.skill_markdown).unwrap();
    let before = fs::read(&path).unwrap();
    let service = SkillPublicationService::new(
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap(),
    );
    let result = service
        .reconcile(
            &identity,
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
    let (dir, identity, habit_id, store) = ready_fixture();
    let proposal = proposal(&identity, &habit_id, &store);
    let stale = request(&proposal, SkillTargetScope::Project);
    store
        .append_diagnostics(
            &identity,
            &proposal.id,
            vec![codegg::skills::promotion::BoundedDiagnostic {
                severity: codegg::skills::Severity::Warning,
                reason: "review note".to_string(),
                location: None,
            }],
        )
        .unwrap();
    let fresh = store
        .get_proposal(&identity, &proposal.id)
        .unwrap()
        .unwrap();
    let service = SkillPublicationService::new(
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap(),
    );
    let error = service
        .publish(
            &identity,
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
    let collision = service
        .publish(
            &identity,
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
    let (dir, identity, habit_id, store) = ready_fixture();
    let foreign = dir.path().join("project/.agents/skills/demo");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join("SKILL.md"),
        "---\nname: demo\ndescription: foreign\n---\nforeign",
    )
    .unwrap();
    let proposal = proposal(&identity, &habit_id, &store);
    let service = SkillPublicationService::new(
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap(),
    );
    let error = service
        .publish(
            &identity,
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

    store.mark_previewed(&identity, &proposal.id, 21).unwrap();
    let previewed = store
        .get_proposal(&identity, &proposal.id)
        .unwrap()
        .unwrap();

    let result = service
        .publish(
            &identity,
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
    let (dir, identity, habit_id, store) = ready_fixture();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let codegg = dir.path().join("project/.codegg");
    fs::create_dir_all(codegg.parent().unwrap()).unwrap();
    symlink(&outside, &codegg).unwrap();
    let proposal = proposal(&identity, &habit_id, &store);
    let service = SkillPublicationService::new(store);
    let error = service
        .publish(
            &identity,
            std::path::Path::new(&identity),
            &dir.path().join("config"),
            request(&proposal, SkillTargetScope::Project),
            20,
        )
        .unwrap_err();
    assert!(matches!(error, SkillPublicationError::UnsafeTarget(_)));
    assert!(!outside.join("skills/demo/SKILL.md").exists());
}
