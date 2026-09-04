use codegg::memory::habit::{
    HabitId, HabitStore, WorkflowAction, WorkflowActionKind, WorkflowEffectClass,
    WorkflowOccurrence, WorkflowOutcome,
};
use codegg::memory::project_namespace;
use codegg::skills::promotion::{
    compute_content_digest, SkillPromotionStore, SkillProposalStatus, SkillProposalSubmission,
    SkillTargetScope,
};
use codegg::skills::{validate_portable_document, AssetDiscoveryConfig, AssetRegistry, SourceKind};
use std::fs;
use tempfile::tempdir;

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

fn fixture() -> (tempfile::TempDir, String, HabitId, SkillPromotionStore) {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let identity = project.to_string_lossy().to_string();
    let habits = HabitStore::with_root(dir.path().join("memory")).unwrap();
    let habit_id = make_ready_habit(&identity, &habits);
    let promotions =
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap();
    (dir, identity, habit_id, promotions)
}

#[test]
fn promotion_lock_contents_are_preserved() {
    let (dir, identity, habit_id, store) = fixture();
    let namespace = project_namespace(&identity);
    let suffix = namespace.strip_prefix("project/").unwrap();
    let lock_path = dir
        .path()
        .join("promotions")
        .join(format!("{suffix}.json.lock"));
    let marker = b"advisory-lock-owner";
    fs::write(&lock_path, marker).unwrap();

    store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();

    assert_eq!(fs::read(lock_path).unwrap(), marker);
}

#[test]
fn submission_without_explicit_request_is_denied() {
    let (_dir, identity, habit_id, store) = fixture();
    let request_id = codegg::skills::promotion::PromotionRequestId::new();
    let result = store.submit(SkillProposalSubmission {
        project_identity: &identity,
        session_id: "session-1",
        request_id: &request_id,
        habit_id: &habit_id,
        supplied_name: "demo",
        supplied_description: "demo",
        skill_markdown: "---\nname: demo\ndescription: demo\n---\nbody",
        now: 10,
    });
    assert!(result.is_err());
}

#[test]
fn request_is_revision_bound_and_consumed_after_one_submission() {
    let (_dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            vec!["existing".to_string()],
            Vec::new(),
            10,
        )
        .unwrap();
    assert_eq!(request.context.target_scope_hint, SkillTargetScope::Project);
    let markdown = "---\nname: demo\ndescription: demo\n---\nbody";
    let proposal = store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now: 11,
        })
        .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    assert_eq!(proposal.revision, 1);
    assert_eq!(proposal.content_digest, compute_content_digest(markdown));
    assert!(store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now: 12,
        })
        .is_err());
}

#[test]
fn stale_candidate_revision_is_rejected() {
    let (dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
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
    assert!(store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: "---\nname: demo\ndescription: demo\n---\nbody",
            now: 12,
        })
        .is_err());
}

#[test]
fn proposal_restrictions_reject_allowed_tools_without_skill_root_changes() {
    let (dir, identity, habit_id, store) = fixture();
    let before = AssetRegistry::build(
        &AssetDiscoveryConfig::default(),
        std::path::Path::new(&identity),
        &[],
    );
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let proposal = store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: "---\nname: demo\ndescription: demo\nallowed-tools: [bash]\n---\nbody",
            now: 11,
        })
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
    let file = dir.path().join("SKILL.md");
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

#[test]
fn wrong_session_project_or_habit_is_denied() {
    let (_dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let markdown = "---\nname: demo\ndescription: demo\n---\nbody";
    // Wrong session.
    assert!(store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-2",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now: 11,
        })
        .is_err());
    // Wrong habit.
    let other = codegg::memory::habit::HabitId::parse("other-habit").unwrap();
    assert!(store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &other,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now: 11,
        })
        .is_err());
    // Wrong project identity (different namespace).
    let other_project = format!("{identity}-other");
    assert!(store
        .submit(SkillProposalSubmission {
            project_identity: &other_project,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now: 11,
        })
        .is_err());
}

#[test]
fn expired_request_cannot_be_replayed() {
    let (_dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let markdown = "---\nname: demo\ndescription: demo\n---\nbody";
    let ttl = codegg::skills::promotion::REQUEST_TTL_MS;
    assert!(store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: markdown,
            now: 10 + ttl + 1,
        })
        .is_err());
}

#[test]
fn non_ready_habit_cannot_begin_request() {
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
    let store =
        SkillPromotionStore::with_roots(dir.path().join("promotions"), dir.path().join("memory"))
            .unwrap();
    assert!(store
        .begin_request(
            &identity,
            "session-1",
            &candidate.id,
            Vec::new(),
            Vec::new(),
            10
        )
        .is_err());
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

#[test]
fn proposal_restrictions_reject_scripts_and_sidecars_but_allow_prose() {
    let (_dir, identity, habit_id, store) = fixture();
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
        let request = store
            .begin_request(&identity, "session-1", &habit_id, Vec::new(), Vec::new(), 10)
            .unwrap();
        let proposal = store
            .submit(SkillProposalSubmission {
                project_identity: &identity,
                session_id: "session-1",
                request_id: &request.id,
                habit_id: &habit_id,
                supplied_name: "demo",
                supplied_description: "demo",
                skill_markdown: markdown,
                now: 11,
            })
            .unwrap();
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

#[test]
fn rejection_lifecycle_is_monotonic_and_habit_stays_ready() {
    let (dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let proposal = store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: "---\nname: demo\ndescription: demo\n---\nbody",
            now: 11,
        })
        .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    // M002 never marks the habit Promoted; that is reserved for M003.
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
    // First reject succeeds, second is a no-op.
    assert!(store.reject_proposal(&identity, &proposal.id, 12).unwrap());
    assert!(!store.reject_proposal(&identity, &proposal.id, 13).unwrap());
    let after = store
        .get_proposal(&identity, &proposal.id)
        .unwrap()
        .unwrap();
    assert_eq!(after.status, SkillProposalStatus::Rejected);
    assert!(after.revision > proposal.revision);
}

#[test]
fn proposal_scope_is_host_enum_not_path() {
    let (_dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let proposal = store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: "---\nname: demo\ndescription: demo\n---\nbody",
            now: 11,
        })
        .unwrap();
    assert_eq!(proposal.target_scope, SkillTargetScope::Project);
    assert_eq!(proposal.project_namespace, project_namespace(&identity));
    assert!(!proposal.project_namespace.contains(".."));
}

#[test]
fn malformed_persisted_proposal_fails_safely() {
    let (dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let proposal = store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: "---\nname: demo\ndescription: demo\n---\nbody",
            now: 11,
        })
        .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    // Corrupt the backing file; reads must fail closed, not return partial state.
    let namespace = project_namespace(&identity);
    let suffix = namespace.strip_prefix("project/").unwrap();
    let path = dir.path().join("promotions").join(format!("{suffix}.json"));
    fs::write(&path, "{ not json").unwrap();
    assert!(store.get_proposal(&identity, &proposal.id).is_err());
    assert!(store.list_proposals(&identity, 32).is_err());
}

#[test]
fn promotion_context_is_structural_and_prompt_hides_session() {
    let (_dir, identity, habit_id, store) = fixture();
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            vec!["existing-skill".to_string()],
            vec![codegg::skills::promotion::BoundedMemoryRef {
                id: "mem-1".to_string(),
                summary: "prefers focused tests".to_string(),
            }],
            10,
        )
        .unwrap();
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

#[test]
fn collision_diagnostic_is_advisory_without_overwrite() {
    let (dir, identity, habit_id, store) = fixture();
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
    // Submitting a same-name proposal records diagnostics via the tool path
    // but never overwrites the existing file.
    let request = store
        .begin_request(
            &identity,
            "session-1",
            &habit_id,
            Vec::new(),
            Vec::new(),
            10,
        )
        .unwrap();
    let proposal = store
        .submit(SkillProposalSubmission {
            project_identity: &identity,
            session_id: "session-1",
            request_id: &request.id,
            habit_id: &habit_id,
            supplied_name: "demo",
            supplied_description: "demo",
            skill_markdown: "---\nname: demo\ndescription: demo\n---\nbody",
            now: 11,
        })
        .unwrap();
    assert_eq!(proposal.status, SkillProposalStatus::Validated);
    let existing = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(existing.contains("existing"));
}
