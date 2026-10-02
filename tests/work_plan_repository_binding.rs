//! Eggplan M003 qualification: repository Plan binding, cross-store
//! reconciliation, terminal evidence writeback, and guarded closure.
//!
//! The tests here own the *repository* side (they initialize `.eggplan` the
//! way a real Eggplan user would) and then drive CodeGG through the single
//! approved binding service. They assert both the positive contract and the
//! fail-closed edges: identity mismatch, structural drift, missing state
//! root, duplicate binding, and unauthorized requirement profiles.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use codegg::work_plan_repository_binding::{ReconcileOutcome, RepositoryBindingService};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionSubjectDisposition,
    ExecutionSubjectKind, ExecutionSubjectProvenance, ExecutionSubjectRevision,
    ExecutionSubjectSealKind, ExecutionSubjectState, IdempotencyClass, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy, SqliteJobStore,
};
use codegg_core::project_catalog::{ProjectCatalog, RegisterLocalProject};
use codegg_core::project_storage::ProjectStorage;
use codegg_core::repository_lineage::classify_remote_urls;
use codegg_core::work_plan::{
    RepositoryBindingState, WorkEvidenceKind, WorkItemId, WorkItemStatus, WorkPlanId,
    WorkPlanStatus, WorkPlanStore,
};
use codegg_core::workspace::{
    SqliteWorkspaceStore, WorkspaceId as CoreWorkspaceId, WorkspaceRecord, WorkspaceRegistry,
};
use eggplan_core::{
    AcceptanceCriterion, CriterionId, EvidenceCardinality, EvidenceKind, EvidenceRequirement, Plan,
    PlanId, PlanItem, PlanItemId, PlanItemStatus, PlanStatus, SubjectPolicy, VerificationDigest,
};
use eggplan_repo::{PlanStore, RepositoryStore};
use sqlx::SqlitePool;

// ── Fixtures ────────────────────────────────────────────────────────────

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_repo() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["config", "user.name", "M003 Test"]);
    run_git(
        dir.path(),
        &["config", "user.email", "m003@test.example.com"],
    );
    std::fs::write(dir.path().join("base.txt"), b"base\n").expect("seed file");
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "base"]);
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .expect("rev-parse")
            .stdout,
    )
    .expect("utf8");
    (dir, head.trim().to_string())
}

fn head_oid(root: &Path) -> String {
    String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .expect("rev-parse")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_string()
}

/// Initialize a real Eggplan repository state root. This is the repository
/// owner's own action; CodeGG only ever opens an already-initialized root.
fn init_state_root(root: &Path) -> RepositoryStore {
    RepositoryStore::open(root.join(".eggplan")).expect("initialize eggplan repository")
}

fn requirement(description: &str) -> EvidenceRequirement {
    EvidenceRequirement {
        description: description.to_string(),
        kind: EvidenceKind::Test,
        provider: None,
        subject_policy: SubjectPolicy::Exact,
        cardinality: EvidenceCardinality::Any,
        min_count: 1,
        allow_human_judgment: false,
        expected_verification_digest: Some(placeholder_verification_digest()),
    }
}

/// A syntactically valid digest used for requirement construction. Tests
/// that need a *matching* digest pin the host-derived one instead.
fn placeholder_verification_digest() -> VerificationDigest {
    VerificationDigest::new(format!("sha256:{}", "ab".repeat(32))).expect("digest")
}

fn criterion(statement: &str, requirements: Vec<EvidenceRequirement>) -> AcceptanceCriterion {
    AcceptanceCriterion {
        id: CriterionId::generate(),
        statement: statement.to_string(),
        human_judgment_allowed: false,
        requirements,
    }
}

/// Create a repository plan the way the Eggplan owner does: Draft at
/// revision 0, then one CAS into `Active`.
fn create_active_repository_plan(store: &RepositoryStore, plan: &mut Plan) {
    let mut draft = plan.clone();
    draft.status = PlanStatus::Draft;
    store.create(&draft).expect("create draft");
    let mut active = draft.clone();
    active.revision += 1;
    active.status = PlanStatus::Active;
    store
        .compare_and_swap(&draft.id, draft.revision, &active)
        .expect("activate");
    plan.revision = active.revision;
    plan.status = PlanStatus::Active;
}

fn one_item_plan(objective: &str) -> Plan {
    Plan {
        // Schema v2 is required for a requirement to carry an authoritative
        // verification binding, which the M003 evidence path requires.
        schema_version: 2,
        id: PlanId::generate(),
        revision: 0,
        objective: objective.to_string(),
        status: PlanStatus::Active,
        provenance: BTreeMap::new(),
        items: vec![PlanItem {
            id: PlanItemId::generate(),
            position: 0,
            parent: None,
            dependencies: Vec::new(),
            status: PlanItemStatus::Actionable,
            description: "run the suite".to_string(),
            criteria: vec![criterion(
                "suite passes",
                vec![requirement("test evidence")],
            )],
            blocker: None,
            next_action: Some("run it".to_string()),
        }],
        subject: None,
    }
}

use std::collections::BTreeMap;

struct Fixture {
    _dir: tempfile::TempDir,
    pool: SqlitePool,
    root: PathBuf,
    project_id: codegg_core::identity::ProjectId,
    workspace_id: CoreWorkspaceId,
    session_id: String,
    store: RepositoryStore,
}

impl Fixture {
    fn workspace_root(&self) -> &Path {
        &self.root
    }
    fn service(&self) -> RepositoryBindingService {
        RepositoryBindingService::new(self.pool.clone())
    }
}

async fn fixture() -> Fixture {
    let (dir, _head) = git_repo();
    let root = dir.path().to_path_buf();
    let store = init_state_root(&root);
    let pool = common::pool::isolated_pool().await;

    // Durable CodeGG identity: workspace registry + project/repository
    // binding + session binding, exactly as production resolves them.
    let registry_store = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    let registry = WorkspaceRegistry::load(registry_store)
        .await
        .expect("registry");
    let record = registry
        .get_or_register(&root)
        .await
        .expect("register workspace");
    let now = chrono::Utc::now();
    let storage = ProjectStorage::new(pool.clone());
    let evidence = classify_remote_urls(["https://example.test/m003"]);
    storage
        .reconcile_workspace(
            &WorkspaceRecord {
                id: record.id.clone(),
                canonical_root: record.canonical_root.clone(),
                display_name: record.display_name.clone(),
                created_at: now,
                last_opened_at: now,
                archived_at: None,
            },
            &evidence,
            "m003-test",
        )
        .await
        .expect("reconcile workspace");
    let catalog = ProjectCatalog::new(pool.clone());
    let registered = catalog
        .register_local_project(
            RegisterLocalProject {
                display_name: "M003".to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &record.id,
            "m003-test",
        )
        .await
        .expect("register project");

    let session_id = "sess-m003-1".to_string();
    create_session(&pool, &session_id, &registered.project_id, &root).await;
    storage
        .bind_session(&session_id, &registered.project_id, &record.id, "m003-test")
        .await
        .expect("bind session");

    Fixture {
        _dir: dir,
        pool,
        root,
        project_id: registered.project_id,
        workspace_id: record.id.clone(),
        session_id,
        store,
    }
}

// ── Binding and identity ────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn valid_same_workspace_bind_creates_mirror_and_map() {
    let f = fixture().await;
    let mut plan = one_item_plan("ship the binding");
    create_active_repository_plan(&f.store, &mut plan);

    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    assert_eq!(binding.eggplan_plan_id, plan.id.as_str());
    assert!(binding.eggplan_repository_id.starts_with("epr_"));
    assert_eq!(binding.binding_state, RepositoryBindingState::Synced);
    assert_eq!(binding.last_seen_plan_revision, plan.revision as i64);

    let store = WorkPlanStore::new(f.pool.clone());
    let mirror = store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("mirror row");
    assert_eq!(mirror.objective, plan.objective);
    assert_eq!(mirror.status, WorkPlanStatus::Active);
    let items = store.list_items(&mirror.id).await.expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, WorkItemStatus::Actionable);
    // The CodeGG mirror never reuses Eggplan identities as storage authority.
    assert!(!items[0].id.as_str().starts_with("epi_"));
    let map = service
        .bindings()
        .list_item_bindings(&binding.work_plan_id)
        .await
        .expect("item map");
    assert_eq!(map.len(), 1);
    assert_eq!(map[0].work_item_id, items[0].id);
    assert!(map[0].eggplan_item_id.starts_with("epi_"));
}

#[tokio::test(flavor = "current_thread")]
async fn dirty_same_subject_binds_and_mirror_acceptance_never_satisfies() {
    let f = fixture().await;
    let mut plan = one_item_plan("dirty bind");
    create_active_repository_plan(&f.store, &mut plan);
    std::fs::write(f.root.join("untracked.txt"), b"x\n").expect("dirty the tree");

    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("dirty same-subject bind still proves identity");

    let store = WorkPlanStore::new(f.pool.clone());
    let items = store
        .list_items(&binding.work_plan_id)
        .await
        .expect("items");
    assert!(!items[0].acceptance.is_empty());
    for criterion in &items[0].acceptance {
        assert_eq!(
            criterion.disposition,
            codegg_core::work_plan::WorkAcceptanceDisposition::Unmet,
            "a projection never fabricates Satisfied"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn repository_subject_mismatch_is_not_a_warning() {
    let f = fixture().await;
    let mut plan = one_item_plan("drifting subject");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    // A new commit between the two independent captures makes the two owners
    // disagree. Reconcile must fail closed, never warn.
    std::fs::write(f.root.join("base.txt"), b"changed\n").expect("edit");
    run_git(&f.root, &["add", "."]);
    run_git(&f.root, &["commit", "-m", "advance"]);
    run_git(&f.root, &["commit", "--allow-empty", "-m", "advance2"]);

    // The live tree moved, so the stored revision is behind. Advance the
    // binding observation artificially to force the identity check to run
    // against a repository whose own capture is stable, then corrupt the
    // proven revision: identity proof then refuses.
    sqlx::query(
        "UPDATE work_plan_eggplan_binding SET eggplan_repository_id = 'epr_forged' WHERE work_plan_id = ?1",
    )
    .bind(binding.work_plan_id.as_str())
    .execute(&f.pool)
    .await
    .expect("tamper");

    let error = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect_err("identity mismatch fails closed");
    assert_eq!(error.code, "repository_identity_mismatch");
}

#[tokio::test(flavor = "current_thread")]
async fn missing_state_root_is_rejected_without_auto_initialization() {
    let f = fixture().await;
    std::fs::remove_dir_all(f.root.join(".eggplan")).expect("remove state root");
    let service = f.service();
    let error = service
        .bind_session_plan(&f.session_id, "ep_missing0000000000000000000000")
        .await
        .expect_err("no implicit initialization");
    assert_eq!(error.code, "eggplan_state_root_missing");
    assert!(
        !f.root.join(".eggplan").exists(),
        "CodeGG must never create the repository state root"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn non_bindable_lifecycles_are_rejected() {
    let f = fixture().await;
    let service = f.service();

    // Draft requires explicit repository activation first.
    let mut draft = one_item_plan("draft is not bindable");
    draft.status = PlanStatus::Draft;
    f.store.create(&draft).expect("create draft");
    let error = service
        .bind_session_plan(&f.session_id, draft.id.as_str())
        .await
        .expect_err("draft cannot bind");
    assert_eq!(error.code, "eggplan_plan_not_bindable");

    // Cancelled is historical and can never be live-bound.
    let mut cancelled = one_item_plan("cancelled cannot bind");
    create_active_repository_plan(&f.store, &mut cancelled);
    let mut terminal = cancelled.clone();
    terminal.revision += 1;
    terminal.status = PlanStatus::Cancelled;
    f.store
        .compare_and_swap(&cancelled.id, cancelled.revision, &terminal)
        .expect("cancel");
    let error = service
        .bind_session_plan(&f.session_id, terminal.id.as_str())
        .await
        .expect_err("cancelled cannot bind");
    assert_eq!(error.code, "eggplan_plan_not_bindable");
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_live_binding_is_rejected() {
    let f = fixture().await;
    let mut plan = one_item_plan("single owner");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("first bind");

    let second_session = "sess-m003-2".to_string();
    create_session(&f.pool, &second_session, &f.project_id, f.workspace_root()).await;
    codegg_core::project_storage::ProjectStorage::new(f.pool.clone())
        .bind_session(&second_session, &f.project_id, &f.workspace_id, "m003-test")
        .await
        .expect("bind second session");

    let error = service
        .bind_session_plan(&second_session, plan.id.as_str())
        .await
        .expect_err("one live binding per repository plan");
    assert_eq!(error.code, "eggplan_plan_already_bound");
}

#[tokio::test(flavor = "current_thread")]
async fn active_codegg_plan_prerequisite_is_enforced() {
    let f = fixture().await;
    let mut plan = one_item_plan("needs empty session");
    create_active_repository_plan(&f.store, &mut plan);
    let store = WorkPlanStore::new(f.pool.clone());
    store
        .create_active(codegg_core::work_plan::NewWorkPlan {
            session_id: f.session_id.clone(),
            project_id: f.project_id.as_str().to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "existing plan".to_string(),
            origin_provenance: "test".to_string(),
            current_phase: None,
        })
        .await
        .expect("existing plan");
    let service = f.service();
    let error = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect_err("must not silently cancel another active plan");
    assert_eq!(error.code, "active_work_plan_exists");
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_requirement_profile_is_rejected() {
    let f = fixture().await;
    // Explicit different provider.
    let mut foreign = one_item_plan("foreign provider");
    foreign.items[0].criteria[0].requirements[0].provider =
        Some(eggplan_core::EvidenceProviderId::new("epp_someone_else").expect("id"));
    create_active_repository_plan(&f.store, &mut foreign);
    let service = f.service();
    let error = service
        .bind_session_plan(&f.session_id, foreign.id.as_str())
        .await
        .expect_err("explicit foreign provider");
    assert_eq!(error.code, "unsupported_requirement_provider");

    // Standalone artifact authority stays deferred.
    let mut artifact = one_item_plan("artifact authority");
    artifact.items[0].criteria[0].requirements[0].kind = EvidenceKind::Artifact;
    create_active_repository_plan(&f.store, &mut artifact);
    let error = service
        .bind_session_plan(&f.session_id, artifact.id.as_str())
        .await
        .expect_err("artifact requirement is deferred");
    assert_eq!(error.code, "unsupported_requirement_kind");

    // Commit/revision authority stays deferred.
    let mut revision = one_item_plan("commit authority");
    revision.items[0].criteria[0].requirements[0].kind = EvidenceKind::Revision;
    create_active_repository_plan(&f.store, &mut revision);
    let error = service
        .bind_session_plan(&f.session_id, revision.id.as_str())
        .await
        .expect_err("commit requirement is deferred");
    assert_eq!(error.code, "unsupported_requirement_kind");
}

#[tokio::test(flavor = "current_thread")]
async fn binding_survives_restart() {
    let f = fixture().await;
    let mut plan = one_item_plan("durable mirror");
    create_active_repository_plan(&f.store, &mut plan);
    let binding = f
        .service()
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    // A fresh service handle models a daemon restart: the durable rows are
    // the authority, not the in-memory handle.
    let restarted = RepositoryBindingService::new(f.pool.clone());
    let reloaded = restarted
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .expect("live binding");
    assert_eq!(
        reloaded.eggplan_repository_id,
        binding.eggplan_repository_id
    );
    assert_eq!(reloaded.intent_digest, binding.intent_digest);
    let items = restarted
        .bindings()
        .list_item_bindings(&binding.work_plan_id)
        .await
        .expect("item map");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].eggplan_item_id,
        mapping_for(&f, &binding.work_plan_id).await
    );
}

async fn mapping_for(f: &Fixture, work_plan_id: &WorkPlanId) -> String {
    f.service()
        .bindings()
        .list_item_bindings(work_plan_id)
        .await
        .expect("item map")
        .remove(0)
        .eggplan_item_id
}

// ── Cross-store lifecycle ───────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_only_external_change_reconciles() {
    let f = fixture().await;
    let mut plan = one_item_plan("lifecycle drift");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    // The repository owner blocks the item out of band. Structure is
    // unchanged, so the mirror reconciles forward.
    let mut next = f.store.get(&plan.id).expect("get");
    next.revision += 1;
    next.items[0].status = PlanItemStatus::Blocked;
    next.items[0].blocker = Some("waiting on review".to_string());
    f.store
        .compare_and_swap(&plan.id, next.revision - 1, &next)
        .expect("lifecycle CAS");

    let outcome = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect("reconcile");
    assert!(matches!(outcome, ReconcileOutcome::Reconciled { .. }));

    let store = WorkPlanStore::new(f.pool.clone());
    let items = store
        .list_items(&binding.work_plan_id)
        .await
        .expect("items");
    assert_eq!(items[0].status, WorkItemStatus::Blocked);
    assert_eq!(items[0].blocker.as_deref(), Some("waiting on review"));
    let reloaded = service
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .expect("live");
    assert_eq!(reloaded.binding_state, RepositoryBindingState::Synced);
    assert_eq!(reloaded.last_seen_plan_revision, next.revision as i64);
}

#[tokio::test(flavor = "current_thread")]
async fn structural_external_change_conflicts() {
    let f = fixture().await;
    let mut plan = one_item_plan("structural drift");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    let mut next = f.store.get(&plan.id).expect("get");
    next.revision += 1;
    next.items[0].description = "different structural intent".to_string();
    f.store
        .compare_and_swap(&plan.id, next.revision - 1, &next)
        .expect("structural CAS");

    let error = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect_err("structural change fails closed");
    assert_eq!(error.code, "repository_plan_structure_changed");
    assert_eq!(
        service
            .binding_state(&binding.work_plan_id)
            .await
            .expect("state"),
        Some(RepositoryBindingState::Conflict)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn missing_repository_plan_conflicts() {
    let f = fixture().await;
    let mut plan = one_item_plan("vanishing plan");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    std::fs::remove_dir_all(f.root.join(".eggplan").join("plans").join(plan.id.as_str()))
        .expect("remove plan directory");

    let error = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect_err("missing repository plan is a conflict");
    assert_eq!(error.code, "repository_plan_missing");
    assert_eq!(
        service
            .binding_state(&binding.work_plan_id)
            .await
            .expect("state"),
        Some(RepositoryBindingState::Conflict)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_mirror_write_reconciles_forward() {
    let f = fixture().await;
    let mut plan = one_item_plan("crash between stores");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    // Simulate a crash after the repository CAS but before the mirror CAS:
    // the repository advanced, the binding still records the old revision.
    let mut next = f.store.get(&plan.id).expect("get");
    next.revision += 1;
    next.items[0].status = PlanItemStatus::InProgress;
    f.store
        .compare_and_swap(&plan.id, next.revision - 1, &next)
        .expect("repository CAS");
    service
        .bindings()
        .mark_needs_reconcile(&binding.work_plan_id)
        .await
        .expect("flag interrupted write");

    let outcome = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect("reconcile forward");
    assert!(matches!(outcome, ReconcileOutcome::Reconciled { .. }));
    let store = WorkPlanStore::new(f.pool.clone());
    let items = store
        .list_items(&binding.work_plan_id)
        .await
        .expect("items");
    assert_eq!(items[0].status, WorkItemStatus::InProgress);
    assert_eq!(
        service
            .binding_state(&binding.work_plan_id)
            .await
            .expect("state"),
        Some(RepositoryBindingState::Synced)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bound_item_mutation_writes_repository_first() {
    let f = fixture().await;
    let mut plan = one_item_plan("repository first");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;

    let repository_plan = f.store.get(&plan.id).expect("get");
    let repository_revision = repository_plan.revision;
    let (plan_row, item_row) = service
        .update_bound_item(
            &work_item_id,
            0,
            WorkItemStatus::InProgress,
            None,
            Some("run the suite".to_string()),
            f.workspace_root(),
        )
        .await
        .expect("bound mutation");

    // The repository committed first and advanced by exactly one.
    let after = f.store.get(&plan.id).expect("get");
    assert_eq!(after.revision, repository_revision + 1);
    assert_eq!(after.items[0].status, PlanItemStatus::InProgress);
    // The mirror then caught up, and the binding recorded the new revision.
    assert_eq!(item_row.status, WorkItemStatus::InProgress);
    assert_eq!(plan_row.id, binding.work_plan_id);
    let reloaded = service
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .expect("live");
    assert_eq!(reloaded.last_seen_plan_revision, after.revision as i64);
}

#[tokio::test(flavor = "current_thread")]
async fn stale_codegg_item_revision_is_refused_before_repository_write() {
    let f = fixture().await;
    let mut plan = one_item_plan("stale revision");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    let repository_revision = f.store.get(&plan.id).expect("get").revision;

    let error = service
        .update_bound_item(
            &work_item_id,
            99,
            WorkItemStatus::InProgress,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect_err("stale revision");
    assert_eq!(error.code, "item_revision_conflict");
    assert_eq!(
        f.store.get(&plan.id).expect("get").revision,
        repository_revision,
        "the repository is untouched when the caller revision is stale"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn completion_requires_repository_evidence() {
    let f = fixture().await;
    let mut plan = one_item_plan("evidence gated completion");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;

    service
        .update_bound_item(
            &work_item_id,
            0,
            WorkItemStatus::InProgress,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("start work");
    let repository_revision = f.store.get(&plan.id).expect("get").revision;
    let error = service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::Completed,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect_err("a model completion proposal is not evidence");
    assert_eq!(error.code, "repository_item_evidence_incomplete");
    assert_eq!(
        f.store.get(&plan.id).expect("get").revision,
        repository_revision,
        "a refused completion never advances the repository"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_is_repository_first() {
    let f = fixture().await;
    let mut plan = one_item_plan("cancel through the repository");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    service
        .cancel_bound_plan(&binding, f.workspace_root())
        .await
        .expect("cancel");

    assert_eq!(
        f.store.get(&plan.id).expect("get").status,
        PlanStatus::Cancelled
    );
    let store = WorkPlanStore::new(f.pool.clone());
    let mirror = store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    assert_eq!(mirror.status, WorkPlanStatus::Cancelled);
    assert!(
        service
            .load_binding(&binding.work_plan_id)
            .await
            .expect("load")
            .is_none(),
        "a cancelled repository plan releases the binding"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn repository_cancelled_externally_reconciles_and_releases() {
    let f = fixture().await;
    let mut plan = one_item_plan("external cancel");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    let mut next = f.store.get(&plan.id).expect("get");
    next.revision += 1;
    next.status = PlanStatus::Cancelled;
    f.store
        .compare_and_swap(&plan.id, next.revision - 1, &next)
        .expect("cancel CAS");

    let outcome = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect("reconcile");
    assert!(matches!(
        outcome,
        ReconcileOutcome::RepositoryCancelled { .. }
    ));
    let store = WorkPlanStore::new(f.pool.clone());
    let mirror = store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    assert_eq!(mirror.status, WorkPlanStatus::Cancelled);
    assert!(service
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn closed_plan_without_valid_closure_is_never_complete() {
    let f = fixture().await;
    let mut plan = one_item_plan("closed without closure");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");

    // Force the plan file to Closed with no closure record. Loading a Closed
    // plan without a valid record is corruption, so the error surfaces as a
    // repository-state failure and the mirror never becomes Completed.
    let path = f
        .root
        .join(".eggplan")
        .join("plans")
        .join(plan.id.as_str())
        .join("plan.json");
    let raw = std::fs::read_to_string(&path).expect("plan file");
    let mutated = raw.replacen("\"status\":\"active\"", "\"status\":\"closed\"", 1);
    assert_ne!(raw, mutated, "plan status field must be present");
    std::fs::write(&path, mutated).expect("tamper");

    let store = RepositoryStore::open_read_only(f.root.join(".eggplan")).expect("read only");
    assert!(
        store.get(&plan.id).is_err(),
        "a Closed plan without a valid ClosureRecord must not load"
    );
    let error = service
        .reconcile_bound_plan(&binding, f.workspace_root())
        .await
        .expect_err("corrupt closed plan is a conflict");
    assert!(matches!(
        error.code,
        "repository_plan_corrupt" | "repository_plan_missing" | "repository_closure_invalid"
    ));
    let codegg_store = WorkPlanStore::new(f.pool.clone());
    let mirror = codegg_store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    assert_ne!(mirror.status, WorkPlanStatus::Completed);
}

// ── Evidence and closure ────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn terminal_evidence_writeback_is_idempotent_and_authoritative() {
    let f = fixture().await;
    // The native job exists first so the repository requirement can pin the
    // authoritative verification digest the host itself derives.
    let (job_id, digest) = insert_passing_test_job(&f).await;
    let mut plan = one_item_plan("evidence writeback");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);

    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(&f, &work_item_id, WorkEvidenceKind::TestJob, &job_id).await;

    let first = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect("first sync");
    assert_eq!(first.appended, 1);
    assert_eq!(first.idempotent, 0);

    let second = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect("retry is idempotent");
    assert_eq!(second.appended, 0);
    assert_eq!(second.idempotent, 1);

    let observations = f.store.list_observations(&plan.id).expect("observations");
    assert_eq!(observations.len(), 1);
    let observation = &observations[0];
    assert_eq!(
        observation.provider_id().as_str(),
        codegg::work_plan_repository_binding::EGGPLAN_PROVIDER_ID
    );
    assert_eq!(observation.kind(), EvidenceKind::Test);
    assert_eq!(observation.status(), eggplan_core::EvidenceStatus::Passed);
    // The observation subject is translated into the proven Eggplan
    // repository namespace, never a blind relabel.
    assert_eq!(
        observation.subject().repository_id,
        binding.eggplan_repository_id
    );
    assert_eq!(observation.subject().revision, head_oid(f.workspace_root()));
    assert!(
        observation.verification_digest().is_some(),
        "terminal evidence carries authoritative verification binding"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn drifted_or_unstable_evidence_is_never_written_back_as_pass() {
    let f = fixture().await;
    let (job_id, digest) = insert_test_job_with_disposition(
        &f,
        ExecutionSubjectDisposition::Drifted,
        head_oid(f.workspace_root()),
    )
    .await;
    let mut plan = one_item_plan("unstable evidence");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(&f, &work_item_id, WorkEvidenceKind::TestJob, &job_id).await;

    let error = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect_err("drifted subject is not authoritative evidence");
    assert_eq!(error.code, "evidence_subject_unstable");
    assert!(f.store.list_observations(&plan.id).expect("obs").is_empty());
}

// ── C001 dirty-subject provenance and bound evidence ─────────────────────

use codegg::execution_subject_capture::capture_attempt_revision;

/// Capture start/seal revisions through the production v2 helper at the
/// current dirty worktree state, asserting the C001 shape: native digest
/// plus the exact Eggplan-compatible digest.
async fn capture_stable_dirty_pair(
    f: &Fixture,
) -> (ExecutionSubjectRevision, ExecutionSubjectRevision) {
    let (start, start_reason) = capture_attempt_revision(f.workspace_root(), &f.workspace_id).await;
    let (seal, seal_reason) = capture_attempt_revision(f.workspace_root(), &f.workspace_id).await;
    assert_eq!(start_reason, None);
    assert_eq!(seal_reason, None);
    let (start, seal) = (start.expect("start capture"), seal.expect("seal capture"));
    assert_eq!(start.state, ExecutionSubjectState::Dirty);
    assert!(start.dirty_digest.is_some(), "native digest is captured");
    assert!(
        start.eggplan_dirty_digest.is_some(),
        "Eggplan-compatible digest is captured alongside the native one"
    );
    assert_ne!(
        start.dirty_digest, start.eggplan_dirty_digest,
        "the two owners use intentionally different encodings"
    );
    assert_eq!(start, seal, "an unmodified dirty tree seals Stable");
    (start, seal)
}

/// Persist one durable test job whose attempt provenance is the given
/// captured/sealed pair, returning the authoritative verification digest.
async fn insert_job_with_provenance(
    f: &Fixture,
    start: &ExecutionSubjectRevision,
    seal: &ExecutionSubjectRevision,
    disposition: ExecutionSubjectDisposition,
) -> (String, VerificationDigest) {
    let store = SqliteJobStore::new(f.pool.clone());
    let job = store
        .create_job(job_spec(test_payload()))
        .await
        .expect("create job");
    let attempt = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new())
        .await
        .expect("begin attempt");
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .expect("mark running");
    store
        .set_attempt_source_subject_started(
            &attempt.attempt_id,
            &ExecutionSubjectProvenance {
                schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
                captured: Some(start.clone()),
                sealed: None,
                disposition: ExecutionSubjectDisposition::Started,
                seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
                unavailable_reason: None,
                materialization: None,
            },
        )
        .await
        .expect("started subject");
    store
        .seal_attempt_source_subject(
            &attempt.attempt_id,
            &ExecutionSubjectProvenance {
                schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
                captured: Some(start.clone()),
                sealed: Some(seal.clone()),
                disposition,
                seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
                unavailable_reason: None,
                materialization: None,
            },
        )
        .await
        .expect("seal subject");
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id,
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .expect("finish attempt");
    let record = store
        .get_job(&job.job_id)
        .await
        .expect("load job")
        .expect("job row");
    let digest = codegg::work_plan_eggplan::verification_digest_for_job(&record)
        .expect("authoritative verification digest");
    (job.job_id.to_string(), digest)
}

#[tokio::test(flavor = "current_thread")]
async fn dirty_stable_execution_binds_evidence_and_guarded_closes() {
    let f = fixture().await;
    // A non-`.eggplan` source file makes the tree dirty for both owners.
    std::fs::write(f.root.join("base.txt"), b"base dirty\n").expect("dirty the tree");
    let (start, seal) = capture_stable_dirty_pair(&f).await;
    let (job_id, digest) =
        insert_job_with_provenance(&f, &start, &seal, ExecutionSubjectDisposition::Stable).await;

    let mut plan = one_item_plan("dirty evidence end to end");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("dirty same-subject bind still proves identity");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(&f, &work_item_id, WorkEvidenceKind::TestJob, &job_id).await;

    let report = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect("stable dirty evidence syncs");
    assert_eq!(report.appended, 1);

    // The observation carries the exact Eggplan subject: repository capture
    // equality, not a converted native digest.
    let expected_digest = f
        .store
        .subject_source()
        .capture()
        .expect("repository capture")
        .dirty_digest
        .expect("dirty repository capture");
    assert_eq!(
        seal.eggplan_dirty_digest.as_deref(),
        Some(expected_digest.as_str()),
        "execution-time digest equals the repository capture"
    );
    let observations = f.store.list_observations(&plan.id).expect("observations");
    assert_eq!(observations.len(), 1);
    let observation = &observations[0];
    assert_eq!(
        observation.subject().state,
        eggplan_core::SubjectState::Dirty
    );
    assert_eq!(
        observation.subject().dirty_digest.as_deref(),
        Some(expected_digest.as_str())
    );
    assert_eq!(
        observation.subject().repository_id,
        binding.eggplan_repository_id
    );

    // Dirty item completion and guarded closure succeed while the same dirty
    // state stays stable.
    service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::InProgress,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("start work");
    service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::Completed,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("evidence-gated dirty completion");
    service
        .finalize_bound_plan(&binding, f.workspace_root())
        .await
        .expect("guarded dirty closure");
    let closed = f.store.get(&plan.id).expect("get");
    assert_eq!(closed.status, PlanStatus::Closed);
    let mirror = WorkPlanStore::new(f.pool.clone())
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    assert_eq!(mirror.status, WorkPlanStatus::Completed);
    assert!(service
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn dirty_content_change_during_attempt_never_becomes_passing_evidence() {
    let f = fixture().await;
    std::fs::write(f.root.join("base.txt"), b"base dirty A\n").expect("dirty A");
    let (start_opt, _) = capture_attempt_revision(f.workspace_root(), &f.workspace_id).await;
    let start = start_opt.expect("start capture");
    // The dirty contents change between start and seal while HEAD and the
    // dirty classification stay the same.
    std::fs::write(f.root.join("base.txt"), b"base dirty B\n").expect("dirty B");
    let (seal_opt, _) = capture_attempt_revision(f.workspace_root(), &f.workspace_id).await;
    let seal = seal_opt.expect("seal capture");
    assert_ne!(start, seal, "changed dirty contents drift the provenance");

    let (job_id, digest) =
        insert_job_with_provenance(&f, &start, &seal, ExecutionSubjectDisposition::Drifted).await;
    let mut plan = one_item_plan("dirty drift");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(&f, &work_item_id, WorkEvidenceKind::TestJob, &job_id).await;

    let error = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect_err("drifted dirty subject is not authoritative evidence");
    assert_eq!(error.code, "evidence_subject_unstable");
    assert!(f.store.list_observations(&plan.id).expect("obs").is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_v1_dirty_provenance_fails_closed_without_backfill() {
    let f = fixture().await;
    // Historical v1: native digest only, no Eggplan-compatible projection.
    let legacy = ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION_V1,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: codegg_subject_identity(&f),
        revision: head_oid(f.workspace_root()),
        state: ExecutionSubjectState::Dirty,
        dirty_digest: Some("b".repeat(64)),
        eggplan_dirty_digest: None,
    };
    assert!(legacy.validate(), "v1 stays readable");
    let (job_id, digest) =
        insert_job_with_provenance(&f, &legacy, &legacy, ExecutionSubjectDisposition::Stable).await;
    let mut plan = one_item_plan("legacy dirty");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(&f, &work_item_id, WorkEvidenceKind::TestJob, &job_id).await;

    let error = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect_err("legacy dirty provenance fails closed");
    assert_eq!(error.code, "legacy_dirty_subject_missing_eggplan_digest");
    // No current-worktree backfill: nothing is written.
    assert!(f.store.list_observations(&plan.id).expect("obs").is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn in_flight_evidence_is_never_persisted_as_terminal() {
    let f = fixture().await;
    let mut plan = one_item_plan("in-flight evidence");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    let job_id = insert_in_flight_test_job(&f).await;
    attach_evidence(
        &f,
        &work_item_id,
        WorkEvidenceKind::TestJob,
        job_id.as_str(),
    )
    .await;

    let report = service
        .sync_terminal_evidence(&binding, f.workspace_root())
        .await
        .expect("sync");
    assert_eq!(report.appended, 0);
    assert_eq!(report.idempotent, 0);
    assert!(
        f.store.list_observations(&plan.id).expect("obs").is_empty(),
        "a transient InProgress body must never reserve a terminal id"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn guarded_closure_precedes_mirror_completion() {
    let f = fixture().await;
    // The native job exists first so the repository requirement can pin the
    // authoritative verification digest the host itself derives.
    let (job_id, digest) = insert_passing_test_job(&f).await;
    let mut plan = one_item_plan("guarded closure");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(
        &f,
        &work_item_id,
        WorkEvidenceKind::TestJob,
        job_id.as_str(),
    )
    .await;

    // A live plan cannot close.
    let error = service
        .finalize_bound_plan(&binding, f.workspace_root())
        .await
        .expect_err("live plan is not complete");
    assert_eq!(error.code, "repository_not_complete");
    assert_eq!(
        f.store.get(&plan.id).expect("get").status,
        PlanStatus::Active
    );

    // Drive the item through the normal repository transitions, then complete
    // it through the evidence-gated service.
    service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::InProgress,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("start work");
    service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::Completed,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("evidence-gated completion");

    service
        .finalize_bound_plan(&binding, f.workspace_root())
        .await
        .expect("guarded closure");

    // Eggplan closure happened first and produced a valid record.
    let closed = f.store.get(&plan.id).expect("get");
    assert_eq!(closed.status, PlanStatus::Closed);
    assert!(f
        .store
        .closure_record(&plan.id)
        .expect("closure record")
        .is_some());
    // Only then is the CodeGG mirror terminal and the binding released.
    let store = WorkPlanStore::new(f.pool.clone());
    let mirror = store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    assert_eq!(mirror.status, WorkPlanStatus::Completed);
    assert!(service
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn crash_after_repository_closure_reconciles_the_mirror() {
    let f = fixture().await;
    let (job_id, digest) = insert_passing_test_job(&f).await;
    let mut plan = one_item_plan("crash after closure");
    plan.items[0].criteria[0].requirements[0].expected_verification_digest = Some(digest);
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let work_item_id = sole_item_id(&f, &binding.work_plan_id).await;
    attach_evidence(
        &f,
        &work_item_id,
        WorkEvidenceKind::TestJob,
        job_id.as_str(),
    )
    .await;
    service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::InProgress,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("start work");
    service
        .update_bound_item(
            &work_item_id,
            current_item_revision(&f, &work_item_id).await,
            WorkItemStatus::Completed,
            None,
            None,
            f.workspace_root(),
        )
        .await
        .expect("evidence-gated completion");

    // Simulate the crash window: the repository closes but CodeGG never
    // terminalizes its mirror. Restart reconciliation must finish the job.
    let before = f.store.get(&plan.id).expect("get");
    let subject = f.store.subject_source().capture().expect("subject");
    let observations = f.store.list_observations(&plan.id).expect("obs");
    let supersessions = f.store.list_supersessions(&plan.id).expect("sup");
    let mut registry = eggplan_core::ProviderRegistry::default();
    registry
        .register_trusted(
            eggplan_core::ProviderDescriptor::new(
                eggplan_core::EvidenceProviderId::new(
                    codegg::work_plan_repository_binding::EGGPLAN_PROVIDER_ID,
                )
                .expect("id"),
                "codegg-host-evidence-v1",
                [
                    EvidenceKind::Test,
                    EvidenceKind::Command,
                    EvidenceKind::DelegatedRun,
                ],
            )
            .expect("descriptor"),
        )
        .expect("register");
    let effective = eggplan_core::effective_observations(&observations, &supersessions)
        .expect("effective")
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let assessment = eggplan_core::assess_plan(&before, &subject, &effective, &registry);
    assert_eq!(assessment.status, eggplan_core::AssessmentStatus::Complete);
    let candidate = eggplan_core::ClosureCandidate::build(
        &before,
        subject,
        assessment,
        &observations,
        &supersessions,
        vec![eggplan_core::ProviderPolicyEntry {
            provider_id: eggplan_core::EvidenceProviderId::new(
                codegg::work_plan_repository_binding::EGGPLAN_PROVIDER_ID,
            )
            .expect("id"),
            class: "codegg-host-evidence-v1".to_string(),
            allowed_kinds: [
                EvidenceKind::Test,
                EvidenceKind::Command,
                EvidenceKind::DelegatedRun,
            ]
            .into_iter()
            .collect(),
        }],
        1,
    )
    .expect("candidate");
    f.store
        .finalize_closure(
            &candidate,
            eggplan_core::ClosureId::new(format!("epcl_crash_{}", candidate.source_revision))
                .expect("id"),
            1,
        )
        .expect("repository closure only");
    assert_eq!(
        f.store.get(&plan.id).expect("get").status,
        PlanStatus::Closed
    );
    // The mirror is still live because CodeGG crashed before terminalizing.
    let store = WorkPlanStore::new(f.pool.clone());
    assert_ne!(
        store
            .get(&binding.work_plan_id)
            .await
            .expect("mirror")
            .expect("row")
            .status,
        WorkPlanStatus::Completed
    );

    let restarted = RepositoryBindingService::new(f.pool.clone());
    let reloaded = restarted
        .load_binding(&binding.work_plan_id)
        .await
        .expect("load")
        .expect("still live after the crash");
    let outcome = restarted
        .reconcile_bound_plan(&reloaded, f.workspace_root())
        .await
        .expect("restart reconciliation finishes the mirror");
    assert!(matches!(outcome, ReconcileOutcome::RepositoryClosed { .. }));
    let mirror = store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    assert_eq!(mirror.status, WorkPlanStatus::Completed);
}

// ── WorkOrder binding ───────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn work_order_binding_supports_only_one_shot_requests() {
    let f = fixture().await;
    let mut plan = one_item_plan("work order inheritance");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let error = service
        .bind_work_order_plan_id("wo_repeat", 3, plan.id.as_str(), f.workspace_root())
        .await
        .expect_err("repeat_count > 1 is rejected");
    assert_eq!(error.code, "work_order_repeat_unsupported");

    let request = service
        .bind_work_order_plan_id("wo_one", 1, plan.id.as_str(), f.workspace_root())
        .await
        .expect("one-shot request");
    assert_eq!(request.eggplan_plan_id, plan.id.as_str());
    assert!(request.eggplan_repository_id.starts_with("epr_"));
    let stored = service
        .work_order_binding("wo_one")
        .await
        .expect("load")
        .expect("row");
    assert_eq!(stored.intent_digest, request.intent_digest);
}

#[tokio::test(flavor = "current_thread")]
async fn work_order_managed_worktree_is_not_supported() {
    use codegg_core::work_order::coordinator::WorkspaceAction;
    let error = RepositoryBindingService::require_shared_repository_state(
        &WorkspaceAction::UseManagedWorktree,
    )
    .expect_err("managed worktrees do not carry the state root");
    assert_eq!(
        error.code,
        "eggplan_binding_requires_shared_repository_state"
    );
    assert!(RepositoryBindingService::require_shared_repository_state(
        &WorkspaceAction::ShareReadOnly
    )
    .is_ok());
    assert!(RepositoryBindingService::require_shared_repository_state(
        &WorkspaceAction::ShareSerialized
    )
    .is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn work_order_intent_change_before_occurrence_blocks_materialization() {
    let f = fixture().await;
    let mut plan = one_item_plan("intent drift before occurrence");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let request = service
        .bind_work_order_plan_id("wo_drift", 1, plan.id.as_str(), f.workspace_root())
        .await
        .expect("request");

    // The repository structure changes after authoring but before the
    // occurrence materializes.
    let mut next = f.store.get(&plan.id).expect("get");
    next.revision += 1;
    next.items[0].description = "changed intent".to_string();
    f.store
        .compare_and_swap(&plan.id, next.revision - 1, &next)
        .expect("structural CAS");

    let session_id = "sess-wo-1".to_string();
    create_session(&f.pool, &session_id, &f.project_id, f.workspace_root()).await;
    codegg_core::project_storage::ProjectStorage::new(f.pool.clone())
        .bind_session(&session_id, &f.project_id, &f.workspace_id, "m003-test")
        .await
        .expect("bind session");

    let error = service
        .materialize_work_order_binding(&request, &session_id, f.workspace_root())
        .await
        .expect_err("changed intent blocks materialization");
    assert_eq!(error.code, "work_order_intent_changed");
    let plans = WorkPlanStore::new(f.pool.clone())
        .active_for_session(&session_id)
        .await
        .expect("query");
    assert!(
        plans.is_none(),
        "a failed materialization never launches an unbound occurrence"
    );
}

// ── Engine selection regression ─────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn unbound_plans_keep_the_m002_git_engine() {
    let f = fixture().await;
    let store = WorkPlanStore::new(f.pool.clone());
    let plan = store
        .create_active(codegg_core::work_plan::NewWorkPlan {
            session_id: f.session_id.clone(),
            project_id: f.project_id.as_str().to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "unbound".to_string(),
            origin_provenance: "test".to_string(),
            current_phase: None,
        })
        .await
        .expect("plan");
    let (_plan_row, item) = store
        .add_item(
            &plan.id,
            codegg_core::work_plan::NewWorkItem {
                parent_item_id: None,
                dependencies: Vec::new(),
                status: WorkItemStatus::Actionable,
                description: "unbound item".to_string(),
                acceptance: vec![codegg_core::work_plan::WorkAcceptance {
                    description: "not yet".to_string(),
                    disposition: codegg_core::work_plan::WorkAcceptanceDisposition::Unmet,
                    note: None,
                }],
                evidence: Vec::new(),
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .expect("add item");
    let plan = store.get(&plan.id).await.expect("get").expect("row");
    let items = vec![item];
    let backed = codegg::work_plan_eggplan::assess_with_engine(
        &f.pool,
        Some(f.workspace_root()),
        &plan,
        &items,
    )
    .await
    .expect("unbound assessment still uses the M002 engine");
    assert_eq!(
        backed.engine,
        codegg::work_plan_eggplan::AssessmentEngine::EggplanGit
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bound_plans_select_the_repository_bound_engine() {
    let f = fixture().await;
    let mut plan = one_item_plan("bound engine");
    create_active_repository_plan(&f.store, &mut plan);
    let service = f.service();
    let binding = service
        .bind_session_plan(&f.session_id, plan.id.as_str())
        .await
        .expect("bind");
    let store = WorkPlanStore::new(f.pool.clone());
    let mirror = store
        .get(&binding.work_plan_id)
        .await
        .expect("mirror")
        .expect("row");
    let items = store.list_items(&mirror.id).await.expect("items");
    let backed = codegg::work_plan_eggplan::assess_with_engine(
        &f.pool,
        Some(f.workspace_root()),
        &mirror,
        &items,
    )
    .await
    .expect("bound assessment");
    assert_eq!(
        backed.engine,
        codegg::work_plan_eggplan::AssessmentEngine::EggplanRepositoryBound
    );
}

// ── Local helpers ───────────────────────────────────────────────────────

/// Create a durable session row through the production session store, then
/// let the caller bind it to the canonical workspace/project relation.
async fn create_session(
    pool: &SqlitePool,
    session_id: &str,
    project_id: &codegg_core::identity::ProjectId,
    root: &Path,
) {
    codegg_core::session::store::SessionStore::new(pool.clone())
        .create_with_id(
            session_id,
            codegg_core::session::CreateSession {
                project_id: project_id.as_str().to_string(),
                directory: root.to_string_lossy().to_string(),
                title: Some("m003".to_string()),
                parent_id: None,
                workspace_id: None,
                agent: Some("build".to_string()),
                model: None,
                tags: None,
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            },
        )
        .await
        .expect("create session");
}

async fn sole_item_id(f: &Fixture, work_plan_id: &WorkPlanId) -> WorkItemId {
    WorkPlanStore::new(f.pool.clone())
        .list_items(work_plan_id)
        .await
        .expect("items")
        .remove(0)
        .id
}

async fn current_item_revision(f: &Fixture, work_item_id: &WorkItemId) -> i64 {
    WorkPlanStore::new(f.pool.clone())
        .get_item(work_item_id)
        .await
        .expect("item")
        .expect("row")
        .revision
}

async fn attach_evidence(
    f: &Fixture,
    work_item_id: &WorkItemId,
    kind: WorkEvidenceKind,
    ref_id: &str,
) {
    let patch = codegg_core::work_plan::WorkItemPatch {
        evidence: Some(vec![codegg_core::work_plan::WorkEvidenceRef {
            kind,
            ref_id: ref_id.to_string(),
            detail: None,
        }]),
        ..Default::default()
    };
    let revision = current_item_revision(f, work_item_id).await;
    WorkPlanStore::new(f.pool.clone())
        .update_item(work_item_id, revision, patch)
        .await
        .expect("attach evidence ref");
}

fn stable_provenance(identity: &str, oid: &str) -> ExecutionSubjectProvenance {
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(clean_revision(identity, oid)),
        sealed: Some(clean_revision(identity, oid)),
        disposition: ExecutionSubjectDisposition::Stable,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

fn clean_revision(identity: &str, oid: &str) -> ExecutionSubjectRevision {
    ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: identity.to_string(),
        revision: oid.to_string(),
        state: ExecutionSubjectState::Clean,
        dirty_digest: None,
        eggplan_dirty_digest: None,
    }
}

fn codegg_subject_identity(f: &Fixture) -> String {
    format!(
        "{}{}",
        codegg::work_plan_repository_binding::CODEGG_SUBJECT_NAMESPACE_PREFIX,
        f.workspace_id.as_str()
    )
}

fn test_payload() -> JobPayload {
    JobPayload::Test {
        command: "cargo test".to_string(),
        argv: vec!["cargo".to_string(), "test".to_string()],
        cwd: None,
        scope: None,
        parent_run_id: None,
    }
}

fn job_spec(payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: f_workspace_id(),
        session_id: None,
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload,
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: Vec::new(),
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: Default::default(),
    }
}

fn f_workspace_id() -> codegg_core::identity::WorkspaceId {
    codegg_core::identity::WorkspaceId::new_unchecked("ws-m003")
}

/// Create and complete a durable test job, returning its id and the
/// authoritative verification digest the host derives from the native record.
async fn insert_passing_test_job(f: &Fixture) -> (String, VerificationDigest) {
    insert_test_job_with_disposition(
        f,
        ExecutionSubjectDisposition::Stable,
        head_oid(f.workspace_root()),
    )
    .await
}

async fn insert_test_job_with_disposition(
    f: &Fixture,
    disposition: ExecutionSubjectDisposition,
    revision: String,
) -> (String, VerificationDigest) {
    let store = SqliteJobStore::new(f.pool.clone());
    let job = store
        .create_job(job_spec(test_payload()))
        .await
        .expect("create job");
    let attempt = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new())
        .await
        .expect("begin attempt");
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .expect("mark running");
    store
        .set_attempt_source_subject_started(
            &attempt.attempt_id,
            &started_provenance(&codegg_subject_identity(f), &revision),
        )
        .await
        .expect("started subject");
    let identity = codegg_subject_identity(f);
    let sealed_provenance = if disposition == ExecutionSubjectDisposition::Drifted {
        // Drift is represented by captured != sealed; the host records both
        // facts and the assessment then refuses the subject.
        ExecutionSubjectProvenance {
            disposition,
            ..stable_provenance(&identity, &revision)
        }
        .with_sealed(&alternate_oid(&revision))
    } else {
        ExecutionSubjectProvenance {
            disposition,
            ..stable_provenance(&identity, &revision)
        }
    };
    store
        .seal_attempt_source_subject(&attempt.attempt_id, &sealed_provenance)
        .await
        .expect("seal subject");
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id,
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .expect("finish attempt");
    let record = store
        .get_job(&job.job_id)
        .await
        .expect("load job")
        .expect("job row");
    let digest = codegg::work_plan_eggplan::verification_digest_for_job(&record)
        .expect("authoritative verification digest");
    (job.job_id.to_string(), digest)
}

/// A distinct but well-formed object id, used to represent a subject that
/// moved between capture and seal.
fn alternate_oid(oid: &str) -> String {
    let flipped = if oid.ends_with('0') { '1' } else { '0' };
    format!("{}{flipped}", &oid[..oid.len() - 1])
}

/// Helper trait used only to build a drifted sealed subject in tests.
trait WithSealed {
    fn with_sealed(&self, oid: &str) -> ExecutionSubjectProvenance;
}

impl WithSealed for ExecutionSubjectProvenance {
    fn with_sealed(&self, oid: &str) -> ExecutionSubjectProvenance {
        let mut next = self.clone();
        next.sealed = Some(clean_revision(
            &self
                .captured
                .as_ref()
                .expect("captured")
                .repository_identity,
            oid,
        ));
        next
    }
}

fn started_provenance(identity: &str, oid: &str) -> ExecutionSubjectProvenance {
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(clean_revision(identity, oid)),
        sealed: None,
        disposition: ExecutionSubjectDisposition::Started,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

async fn insert_in_flight_test_job(f: &Fixture) -> String {
    let store = SqliteJobStore::new(f.pool.clone());
    let job = store
        .create_job(job_spec(test_payload()))
        .await
        .expect("create job");
    // Left running with no terminal state: the resolver reports InProgress,
    // which must never be persisted as a terminal observation.
    let attempt = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new())
        .await
        .expect("begin attempt");
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .expect("mark running");
    job.job_id.to_string()
}
