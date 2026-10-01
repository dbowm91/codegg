//! Application-layer repository Plan binding service (Eggplan M003).
//!
//! This is the **only** production owner of Eggplan repository state: it
//! holds every `RepositoryStore::open`, the CodeGG-mirror projection, the
//! subject-identity proof, repository-first cross-store ordering, terminal
//! evidence writeback, and guarded closure. Tools, the arbiter, the
//! scheduler, and the WorkOrder coordinator all go through it; none of them
//! touch an Eggplan store directly (enforced by
//! `scripts/check_work_plan_repository_binding.py`).
//!
//! Authority model for a bound plan:
//!
//! - the Eggplan repository Plan is canonical for intent, item lifecycle,
//!   evidence, and terminal closure;
//! - CodeGG keeps a durable runtime mirror used by the scheduler, agent,
//!   Todo, and Goal surfaces. The mirror is never an independent source of
//!   truth once bound;
//! - CodeGG remains owner of session/project/workspace identity and of the
//!   WorkOrder/session/scheduler/job/AgentRun/worktree execution surfaces.
//!
//! Cross-store consistency is **not** a single transaction. Eggplan's
//! file-backed store and CodeGG's SQLite catalog cannot commit atomically, so
//! every mutating bound path commits the repository `compare_and_swap` first
//! and the CodeGG mirror second. A crash between the two leaves the repository
//! ahead; the next load reconciles the mirror forward and never rolls the
//! repository backward.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codegg_core::identity::WorkspaceId;
use codegg_core::jobs::{
    ExecutionSubjectDisposition, ExecutionSubjectRevision, ExecutionSubjectState, JobId, JobRecord,
    JobStore, SqliteJobStore,
};
use codegg_core::project_storage::ProjectStorage;
use codegg_core::run_store::{FsRunStore, RunId, RunStore};
use codegg_core::work_order::coordinator::WorkspaceAction;
use codegg_core::work_plan::{
    HostEvidenceStatus, RepositoryBindingState, RepositoryBindingStore, RepositoryItemBinding,
    RepositoryPlanBinding, RepositoryWorkOrderBinding, WorkAcceptance, WorkAcceptanceDisposition,
    WorkEvidenceKind, WorkItem, WorkItemId, WorkItemPatch, WorkItemStatus, WorkPlan,
    WorkPlanCompletionAssessment, WorkPlanError, WorkPlanId, WorkPlanStatus, WorkPlanStore,
    MAX_ACCEPTANCE_CHARS, MAX_ACCEPTANCE_PER_ITEM, MAX_BLOCKER_CHARS, MAX_ITEM_DESCRIPTION_CHARS,
    MAX_NEXT_ACTION_CHARS, MAX_OBJECTIVE_CHARS,
};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRegistry};
use eggplan_codegg_compat::{
    project_repository_plan, RepositoryItemProjectionV1, RepositoryPlanProjectionV1,
};
use eggplan_core::{
    assess_plan, effective_observations, ArtifactRef, AssessmentStatus, ClosureCandidate,
    ClosureId, EvidenceKind, EvidenceObservation, EvidenceObservationId, EvidenceObservationInput,
    EvidenceProviderId, EvidenceStatus, Plan, PlanId, PlanItemId, PlanItemStatus, PlanStatus,
    ProviderDescriptor, ProviderPolicyEntry, ProviderRegistry, SubjectRevision, SubjectState,
};
use eggplan_repo::{PlanStore, RepoError, RepositoryStore};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Exact immutable Eggplan revision consumed by M003. One reviewed revision
/// carries both the M002 assessment bridge and the M003 reverse projection.
pub const EGGPLAN_PIN: &str = "3f7c603315131bb169bfdd2bb575531d228532b1";
/// Eggplan repository the pin belongs to.
pub const EGGPLAN_REPO: &str = "https://github.com/eggstack/eggplan.git";
/// Repository-local administrative state root, relative to the canonical
/// CodeGG workspace root. It is the only accepted state location; no caller
/// ever supplies a state path, URL, or parent traversal.
pub const EGGPLAN_STATE_DIR: &str = ".eggplan";
/// CodeGG run-record root, matching `ProductionWorkspaceServicesFactory`.
const CODEGG_RUN_DIR: [&str; 2] = [".codegg", "runs"];
/// Fixed CodeGG-host evidence provider identity for repository-bound
/// writeback. Trust is host code, never serialized plan text.
pub const EGGPLAN_PROVIDER_ID: &str = "epp_codegg_host";
const EGGPLAN_PROVIDER_CLASS: &str = "codegg-host-evidence-v1";
/// Namespace CodeGG uses for execution-subject repository identity (M002).
pub const CODEGG_SUBJECT_NAMESPACE_PREFIX: &str = "codegg-workspace:";
const MAX_DETAIL_CHARS: usize = 300;
const MAX_REASON_CHARS: usize = 500;
const MAX_TEXT_CHARS: usize = 200;
const MAX_ARTIFACT_REFS: usize = 16;
const MAX_OBSERVATION_ID_CHARS: usize = 96;

/// Outcome of one repository reconciliation pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// Repository state already matches the mirror.
    Synced,
    /// The repository advanced lifecycle/blocker/next-action; the mirror was
    /// advanced to match and the observed revision recorded.
    Reconciled { revision: u64 },
    /// The repository closed with a valid `ClosureRecord`; the mirror was
    /// terminalized as completed and the binding released.
    RepositoryClosed { revision: u64 },
    /// The repository cancelled; the mirror was terminalized and the binding
    /// released.
    RepositoryCancelled { revision: u64 },
}

/// Bounded binding failure. Every caller keeps the plan and must not treat it
/// as complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingError {
    pub code: &'static str,
    pub detail: String,
}

impl BindingError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for BindingError {}

impl From<WorkPlanError> for BindingError {
    fn from(error: WorkPlanError) -> Self {
        let code: &'static str = match error {
            WorkPlanError::Conflict { .. } => "binding_conflict",
            WorkPlanError::NotFound(_) => "binding_not_found",
            WorkPlanError::ScopeMismatch(_) => "binding_scope_mismatch",
            WorkPlanError::Terminal(_) => "binding_terminal",
            WorkPlanError::Validation(_) | WorkPlanError::Storage(_) => "binding_storage_error",
        };
        BindingError::new(code, error.to_string())
    }
}

fn repo_error(reason: &'static str, error: RepoError) -> BindingError {
    BindingError::new(reason, bounded(&error.to_string(), MAX_DETAIL_CHARS))
}

fn subject_error(reason: &'static str, error: eggplan_repo::GitSubjectError) -> BindingError {
    BindingError::new(reason, bounded(&error.to_string(), MAX_DETAIL_CHARS))
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn bounded_id(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect()
}

fn now_unix_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

// ── State root + repository store ownership ────────────────────────────

/// The single repository-local administrative root for a canonical CodeGG
/// workspace. Only this path is ever opened.
pub fn state_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(EGGPLAN_STATE_DIR)
}

/// The workspace's durable CodeGG run-record root.
pub fn run_store_root(workspace_root: &Path) -> PathBuf {
    let mut root = workspace_root.to_path_buf();
    for segment in CODEGG_RUN_DIR {
        root = root.join(segment);
    }
    root
}

/// Open the repository store for `workspace_root`.
///
/// The state root must already exist and validate. `open_read_only` is always
/// called first precisely because the mutating `open` would *create* and
/// initialize a missing store with a fresh `epr_*` identity; M003 never
/// auto-initializes. A missing root is `eggplan_state_root_missing`, not an
/// implicit initialization.
fn open_store(workspace_root: &Path, mutating: bool) -> Result<RepositoryStore, BindingError> {
    let root = state_root(workspace_root);
    if !root.is_dir() {
        return Err(BindingError::new(
            "eggplan_state_root_missing",
            "workspace has no repository-local Eggplan state root",
        ));
    }
    let validated = RepositoryStore::open_read_only(&root)
        .map_err(|error| repo_error("eggplan_state_root_invalid", error))?;
    if !mutating {
        return Ok(validated);
    }
    // The gate above proved the root exists and its config parses, so `open`
    // takes the existing-config path and never initializes one.
    RepositoryStore::open(&root).map_err(|error| repo_error("eggplan_store_open_failed", error))
}

// ── Identity proof (plan §5) ────────────────────────────────────────────

/// The proven CodeGG-workspace <-> Eggplan-repository tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityProof {
    pub codegg_repository_id: String,
    pub workspace_id: String,
    pub codegg_subject_namespace: String,
    pub eggplan_repository_id: String,
    pub revision: String,
    pub state: SubjectState,
    /// Eggplan-owner-normalized dirty digest for the proven state.
    pub normalized_dirty_digest: Option<String>,
    /// CodeGG-owner-normalized dirty digest for the same proven state.
    pub codegg_dirty_digest: Option<String>,
}

/// The proven CodeGG relation for one canonical session workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRelation {
    pub workspace_id: String,
    pub codegg_repository_id: String,
    pub workspace_root: PathBuf,
}

/// Normalize a bare-hex dirty digest into Eggplan's `sha256:<hex>` form.
/// Anything that is not exactly SHA-256 hex fails closed.
pub fn normalize_dirty_digest(hex: &str) -> Result<String, BindingError> {
    let is_hex = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if let Some(rest) = hex.strip_prefix("sha256:") {
        return if is_hex(rest) {
            Ok(hex.to_string())
        } else {
            Err(BindingError::new(
                "subject_invalid",
                "dirty digest is not sha256 hex",
            ))
        };
    }
    if is_hex(hex) {
        Ok(format!("sha256:{hex}"))
    } else {
        Err(BindingError::new(
            "subject_invalid",
            "dirty digest is not sha256 hex",
        ))
    }
}

/// Independently capture both sides of the identity proof and require them to
/// describe the same Git state.
///
/// - CodeGG side: governed `egggit` capture of the canonical workspace with
///   the Eggplan administrative root excluded, because an untracked
///   administrative directory is not source state and both owners must agree
///   about it.
/// - Eggplan side: `RepositoryStore::subject_source().capture()`.
///
/// Equality is required on the revision OID and on clean/dirty state. The
/// dirty *content* digests are each recorded in their own owner's canonical
/// form; the two owners use different canonical manifest encodings, so
/// claiming byte equality between them would assert an invariant that does
/// not exist. Disagreement on revision or dirtiness is
/// `repository_subject_mismatch`, never a warning.
pub async fn prove_identity(
    relation: &WorkspaceRelation,
    store: &RepositoryStore,
) -> Result<IdentityProof, BindingError> {
    let eggplan_subject = store
        .subject_source()
        .capture()
        .map_err(|error| subject_error("eggplan_subject_capture_failed", error))?;
    let codegg_subject = egggit::capture_git_source_subject_excluding(
        &relation.workspace_root,
        Path::new(EGGPLAN_STATE_DIR),
    )
    .await
    .map_err(|error| {
        BindingError::new(
            "repository_subject_capture_failed",
            format!("CodeGG subject capture failed: {error:?}"),
        )
    })?;
    let codegg_dirty = codegg_subject
        .dirty_digest
        .as_deref()
        .map(normalize_dirty_digest)
        .transpose()?;
    let codegg_state = if codegg_dirty.is_some() {
        SubjectState::Dirty
    } else {
        SubjectState::Clean
    };
    if codegg_subject.revision != eggplan_subject.revision {
        return Err(BindingError::new(
            "repository_subject_mismatch",
            "CodeGG and Eggplan captured different revisions",
        ));
    }
    if codegg_state != eggplan_subject.state {
        return Err(BindingError::new(
            "repository_subject_mismatch",
            "CodeGG and Eggplan disagree on clean/dirty state",
        ));
    }
    Ok(IdentityProof {
        codegg_repository_id: relation.codegg_repository_id.clone(),
        workspace_id: relation.workspace_id.clone(),
        codegg_subject_namespace: format!(
            "{CODEGG_SUBJECT_NAMESPACE_PREFIX}{}",
            relation.workspace_id
        ),
        eggplan_repository_id: store.repository_id().to_string(),
        revision: eggplan_subject.revision.clone(),
        state: eggplan_subject.state,
        normalized_dirty_digest: eggplan_subject.dirty_digest.clone(),
        codegg_dirty_digest: codegg_dirty,
    })
}

/// The single validated historical-subject translator (plan §5.8, §21).
///
/// A CodeGG historical subject is translated into the Eggplan repository
/// namespace only through the durable proven binding. The revision, dirty
/// state, and normalized dirty digest carry through unchanged; only the
/// repository identity is re-namespaced, and only after the binding proved
/// both identities describe the same Git state. No other site rewrites
/// `SubjectRevision.repository_id`.
pub fn translate_historical_subject(
    binding: &RepositoryPlanBinding,
    historical: &ExecutionSubjectRevision,
) -> Result<SubjectRevision, BindingError> {
    if !historical
        .repository_identity
        .starts_with(CODEGG_SUBJECT_NAMESPACE_PREFIX)
    {
        return Err(BindingError::new(
            "subject_namespace_unknown",
            "historical subject is not in a CodeGG namespace",
        ));
    }
    if binding.eggplan_repository_id.is_empty() || binding.codegg_repository_id.is_empty() {
        return Err(BindingError::new(
            "subject_identity_unproven",
            "binding has no proven identity tuple",
        ));
    }
    let (state, dirty) = match historical.state {
        ExecutionSubjectState::Clean => (SubjectState::Clean, None),
        ExecutionSubjectState::Dirty => (
            SubjectState::Dirty,
            Some(
                historical
                    .dirty_digest
                    .as_deref()
                    .ok_or_else(|| {
                        BindingError::new(
                            "subject_invalid",
                            "dirty historical subject has no dirty digest",
                        )
                    })
                    .and_then(normalize_dirty_digest)?,
            ),
        ),
    };
    let subject = SubjectRevision {
        subject_kind: "git".to_string(),
        repository_id: binding.eggplan_repository_id.clone(),
        revision: historical.revision.clone(),
        state,
        dirty_digest: dirty,
    };
    subject.validate().map_err(|error| {
        BindingError::new(
            "subject_invalid",
            format!("translated subject is invalid: {error}"),
        )
    })?;
    Ok(subject)
}

// ── Supported requirement profile (plan §9) ─────────────────────────────

/// Requirement kinds M003 defers because they need authority the host does
/// not have: standalone artifact and commit/revision resolution.
const DEFERRED_REQUIREMENT_KINDS: &[EvidenceKind] =
    &[EvidenceKind::Artifact, EvidenceKind::Revision];

/// Reject live binding when a repository requirement needs a provider other
/// than the fixed CodeGG host, a deferred evidence kind, or an execution
/// requirement with no authoritative verification binding.
///
/// Provider trust is explicit host policy: Eggplan has no
/// repository-global trusted-provider registry, and the presence of an
/// observation file is not trust.
pub fn validate_requirement_profile(
    projection: &RepositoryPlanProjectionV1,
) -> Result<(), BindingError> {
    let host = host_provider_id()?;
    for item in &projection.items {
        for criterion in &item.criteria {
            for requirement in &criterion.requirements {
                if let Some(provider) = &requirement.provider {
                    if provider != &host {
                        return Err(BindingError::new(
                            "unsupported_requirement_provider",
                            format!(
                                "requirement {:?} requires provider {}; CodeGG M003 binds only {EGGPLAN_PROVIDER_ID}",
                                requirement.description,
                                provider.as_str()
                            ),
                        ));
                    }
                }
                if DEFERRED_REQUIREMENT_KINDS.contains(&requirement.kind) {
                    return Err(BindingError::new(
                        "unsupported_requirement_kind",
                        format!(
                            "requirement {:?} needs {:?} authority, which M003 defers",
                            requirement.description, requirement.kind
                        ),
                    ));
                }
                if requirement.kind == EvidenceKind::HumanJudgment {
                    // Human judgment binds and surfaces
                    // `AwaitingUserJudgment`; CodeGG never manufactures the
                    // observation nor auto-closes it.
                    continue;
                }
                if requirement.expected_verification_digest.is_none() {
                    return Err(BindingError::new(
                        "unsupported_requirement_verification",
                        format!(
                            "requirement {:?} has no authoritative verification binding",
                            requirement.description
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn host_provider_id() -> Result<EvidenceProviderId, BindingError> {
    EvidenceProviderId::new(EGGPLAN_PROVIDER_ID)
        .map_err(|error| BindingError::new("provider_id_invalid", error.to_string()))
}

fn bound_provider_registry() -> Result<ProviderRegistry, BindingError> {
    let mut registry = ProviderRegistry::default();
    registry
        .register_trusted(
            ProviderDescriptor::new(
                host_provider_id()?,
                EGGPLAN_PROVIDER_CLASS,
                [
                    EvidenceKind::Test,
                    EvidenceKind::Command,
                    EvidenceKind::DelegatedRun,
                ],
            )
            .map_err(|error| BindingError::new("provider_invalid", error.to_string()))?,
        )
        .map_err(|error| BindingError::new("provider_invalid", error.to_string()))?;
    Ok(registry)
}

/// The provider-policy entries recorded in an Eggplan closure candidate.
fn bound_provider_policy() -> Vec<ProviderPolicyEntry> {
    vec![ProviderPolicyEntry {
        provider_id: EvidenceProviderId::new(EGGPLAN_PROVIDER_ID)
            .expect("fixed provider id is valid"),
        class: EGGPLAN_PROVIDER_CLASS.to_string(),
        allowed_kinds: [
            EvidenceKind::Test,
            EvidenceKind::Command,
            EvidenceKind::DelegatedRun,
        ]
        .into_iter()
        .collect(),
    }]
}

// ── Repository projection -> CodeGG mirror (plan §10) ───────────────────

fn map_plan_status(status: &PlanStatus) -> WorkPlanStatus {
    match status {
        PlanStatus::Active => WorkPlanStatus::Active,
        PlanStatus::Blocked => WorkPlanStatus::Blocked,
        PlanStatus::Closed => WorkPlanStatus::Completed,
        PlanStatus::Cancelled => WorkPlanStatus::Cancelled,
        PlanStatus::Draft => WorkPlanStatus::Active,
    }
}

fn map_work_item_status(status: WorkItemStatus) -> PlanItemStatus {
    match status {
        WorkItemStatus::Pending => PlanItemStatus::Pending,
        WorkItemStatus::Actionable => PlanItemStatus::Actionable,
        WorkItemStatus::InProgress => PlanItemStatus::InProgress,
        WorkItemStatus::Blocked => PlanItemStatus::Blocked,
        WorkItemStatus::Completed => PlanItemStatus::Completed,
        WorkItemStatus::Cancelled => PlanItemStatus::Cancelled,
    }
}

fn map_item_status(status: PlanItemStatus) -> WorkItemStatus {
    match status {
        PlanItemStatus::Pending => WorkItemStatus::Pending,
        PlanItemStatus::Actionable => WorkItemStatus::Actionable,
        PlanItemStatus::InProgress => WorkItemStatus::InProgress,
        PlanItemStatus::Blocked => WorkItemStatus::Blocked,
        PlanItemStatus::Completed => WorkItemStatus::Completed,
        PlanItemStatus::Cancelled => WorkItemStatus::Cancelled,
    }
}

/// Build the display-only CodeGG acceptance list for a mirrored item.
///
/// Criterion statements are preserved. `RequiresUserJudgment` appears only
/// where the repository criterion permits human judgment; everything else is
/// `Unmet`. `Satisfied` is never produced from a projection: acceptance
/// satisfaction is repository/evidence authority, and CodeGG acceptance is
/// display-only for a bound plan.
fn mirror_acceptance(item: &RepositoryItemProjectionV1) -> Vec<WorkAcceptance> {
    item.criteria
        .iter()
        .take(MAX_ACCEPTANCE_PER_ITEM)
        .map(|criterion| WorkAcceptance {
            description: bounded(&criterion.statement, MAX_ACCEPTANCE_CHARS),
            disposition: if criterion.human_judgment_allowed
                || criterion
                    .requirements
                    .iter()
                    .any(|requirement| requirement.kind == EvidenceKind::HumanJudgment)
            {
                WorkAcceptanceDisposition::RequiresUserJudgment
            } else {
                WorkAcceptanceDisposition::Unmet
            },
            note: Some("eggplan_repository_criterion".to_string()),
        })
        .collect()
}

// ── Bounded public status ───────────────────────────────────────────────

/// Bounded, host-safe binding status for projections. It never exposes an
/// absolute state-root path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryBindingStatus {
    pub state: &'static str,
    pub eggplan_repository_id: String,
    pub eggplan_plan_id: String,
    pub last_seen_plan_revision: i64,
}

/// Bounded outcome of one terminal evidence sync.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct EvidenceSyncReport {
    pub appended: u32,
    pub idempotent: u32,
}

/// Canonical repository assessment for a bound plan.
#[derive(Debug, Clone)]
pub struct BoundAssessment {
    pub assessment: eggplan_core::PlanAssessment,
    pub revision: u64,
    pub subject: SubjectRevision,
}

// ── Service ─────────────────────────────────────────────────────────────

/// The repository-binding service facade. It holds no repository handle:
/// handles are opened per operation inside the proven workspace scope and
/// dropped at the end of the operation.
#[derive(Clone)]
pub struct RepositoryBindingService {
    pool: SqlitePool,
    plans: WorkPlanStore,
    bindings: RepositoryBindingStore,
}

impl RepositoryBindingService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            plans: WorkPlanStore::new(pool.clone()),
            bindings: RepositoryBindingStore::new(pool.clone()),
            pool,
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn bindings(&self) -> &RepositoryBindingStore {
        &self.bindings
    }

    /// `true` when the v68 binding tables exist. An unmigrated catalog has no
    /// bindings, so read paths keep their pre-M003 behavior instead of
    /// failing.
    pub async fn binding_tables_available(&self) -> bool {
        RepositoryBindingStore::schema_available(&self.pool).await
    }

    /// Live binding for a CodeGG plan, or `None` when the plan is unbound.
    pub async fn load_binding(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<Option<RepositoryPlanBinding>, BindingError> {
        Ok(self.bindings.live_binding(work_plan_id).await?)
    }

    /// Durable reconciliation state of a plan's binding, including released
    /// and conflicted history. `None` when the plan was never bound.
    pub async fn binding_state(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<Option<RepositoryBindingState>, BindingError> {
        Ok(self
            .bindings
            .get_binding(work_plan_id)
            .await?
            .map(|binding| binding.binding_state))
    }

    /// Bounded public projection of the binding state.
    pub async fn binding_status(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<Option<RepositoryBindingStatus>, BindingError> {
        Ok(self
            .bindings
            .get_binding(work_plan_id)
            .await?
            .map(|binding| RepositoryBindingStatus {
                state: binding.binding_state.as_str(),
                eggplan_repository_id: binding.eggplan_repository_id,
                eggplan_plan_id: binding.eggplan_plan_id,
                last_seen_plan_revision: binding.last_seen_plan_revision,
            }))
    }

    // ── Explicit session binding (plan §8) ─────────────────────────

    /// Bind an existing, already-valid repository Plan to a session's CodeGG
    /// runtime mirror. This is a host/user-authorized operation; no
    /// model-selected planning tool reaches it.
    pub async fn bind_session_plan(
        &self,
        session_id: &str,
        eggplan_plan_id: &str,
    ) -> Result<RepositoryPlanBinding, BindingError> {
        let plan_id = PlanId::new(eggplan_plan_id)
            .map_err(|error| BindingError::new("eggplan_plan_id_invalid", error.to_string()))?;
        if self.plans.active_for_session(session_id).await?.is_some() {
            return Err(BindingError::new(
                "active_work_plan_exists",
                "session already has an active CodeGG work plan; resolve it explicitly first",
            ));
        }
        let relation = session_workspace_relation(&self.pool, session_id).await?;
        let store = open_store(&relation.workspace_root, false)?;
        let identity = prove_identity(&relation, &store).await?;
        let repository_plan = store.get(&plan_id).map_err(|error| {
            if matches!(error, RepoError::NotFound(_)) {
                BindingError::new("eggplan_plan_not_found", error.to_string())
            } else {
                repo_error("eggplan_plan_invalid", error)
            }
        })?;
        let projection =
            project_repository_plan(&repository_plan).map_err(|error| match error {
                eggplan_codegg_compat::RepositoryProjectionError::NonBindableLifecycle => {
                    BindingError::new(
                        "eggplan_plan_not_bindable",
                        "only Active or Blocked repository plans can be bound",
                    )
                }
                other => BindingError::new("eggplan_plan_invalid", format!("{other:?}")),
            })?;
        validate_requirement_profile(&projection)?;
        if self
            .bindings
            .live_binding_for_repository_plan(
                &relation.workspace_id,
                &identity.eggplan_repository_id,
                &binding_plan_key(&plan_id),
            )
            .await?
            .is_some()
        {
            return Err(BindingError::new(
                "eggplan_plan_already_bound",
                "another live CodeGG work plan already binds this repository plan",
            ));
        }
        self.create_mirror(
            session_id,
            &relation,
            &identity.eggplan_repository_id,
            &binding_plan_key(&plan_id),
            &projection,
        )
        .await
    }

    /// Create the CodeGG runtime mirror rows plus the durable binding and
    /// item map in one SQLite transaction.
    ///
    /// Legacy `create_active` is deliberately not used: it silently cancels
    /// another active plan, which would destroy runtime ownership of a
    /// repository Plan this binding has not claimed.
    async fn create_mirror(
        &self,
        session_id: &str,
        relation: &WorkspaceRelation,
        eggplan_repository_id: &str,
        eggplan_plan_id: &str,
        projection: &RepositoryPlanProjectionV1,
    ) -> Result<RepositoryPlanBinding, BindingError> {
        let project_id = self.session_project_id(session_id).await?;
        let work_plan_id = WorkPlanId::generate();
        let now = chrono::Utc::now();
        let mut item_ids: Vec<RepositoryItemBinding> = Vec::new();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        let status = map_plan_status(&projection.status);
        sqlx::query(
            "INSERT INTO work_plan (id, revision, session_id, project_id, objective, \
             objective_digest, origin_provenance, status, created_at, updated_at) \
             VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        )
        .bind(work_plan_id.as_str())
        .bind(session_id)
        .bind(&project_id)
        .bind(bounded(&projection.objective, MAX_OBJECTIVE_CHARS))
        .bind(codegg_core::work_plan::work_plan_origin_digest(
            &projection.objective,
        ))
        .bind(format!(
            "eggplan_repository:{}",
            bounded_id(eggplan_plan_id)
        ))
        .bind(status.as_str())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        for (position, item) in projection.items.iter().enumerate() {
            let work_item_id = WorkItemId::generate();
            let mapped_dependencies: Vec<String> = item
                .dependencies
                .iter()
                .filter_map(|dep| find_mapped_item(&item_ids, dep.as_str()))
                .map(|id| id.0)
                .collect();
            let mapped_parent = item
                .parent_item_id
                .as_ref()
                .and_then(|parent| find_mapped_item(&item_ids, parent.as_str()));
            let mut description = bounded(item.description.trim(), MAX_ITEM_DESCRIPTION_CHARS);
            if description.is_empty() {
                description = format!("repository item {}", item.item_id.as_str());
            }
            let acceptance = mirror_acceptance(item);
            let blocker = item
                .blocker
                .as_deref()
                .map(|value| bounded(value, MAX_BLOCKER_CHARS));
            let next_action = item
                .next_action
                .as_deref()
                .map(|value| bounded(value, MAX_NEXT_ACTION_CHARS));
            sqlx::query(
                "INSERT INTO work_item (id, plan_id, revision, position, parent_item_id, \
                 dependencies_json, status, description, acceptance_json, evidence_json, \
                 blocker, next_action, created_at, updated_at) \
                 VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7, ?8, '[]', ?9, ?10, ?11, ?11)",
            )
            .bind(work_item_id.as_str())
            .bind(work_plan_id.as_str())
            .bind(position as i64)
            .bind(mapped_parent.as_ref().map(|id| id.0.clone()))
            .bind(serde_json::to_string(&mapped_dependencies).unwrap_or_else(|_| "[]".to_string()))
            .bind(map_item_status(item.status).as_str())
            .bind(&description)
            .bind(serde_json::to_string(&acceptance).unwrap_or_else(|_| "[]".to_string()))
            .bind(blocker.as_deref())
            .bind(next_action.as_deref())
            .bind(now.timestamp_millis())
            .execute(&mut *tx)
            .await
            .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
            item_ids.push(RepositoryItemBinding {
                work_plan_id: work_plan_id.clone(),
                work_item_id,
                eggplan_item_id: item.item_id.as_str().to_string(),
            });
        }
        let binding = RepositoryPlanBinding {
            work_plan_id: work_plan_id.clone(),
            workspace_id: relation.workspace_id.clone(),
            codegg_repository_id: relation.codegg_repository_id.clone(),
            eggplan_repository_id: eggplan_repository_id.to_string(),
            eggplan_plan_id: eggplan_plan_id.to_string(),
            last_seen_plan_revision: projection.revision as i64,
            intent_digest: projection.intent_digest.clone(),
            projection_digest: projection.projection_digest.clone(),
            binding_state: RepositoryBindingState::Synced,
            created_at: now,
            updated_at: now,
            released_at: None,
        };
        for item in &item_ids {
            sqlx::query(
                "INSERT INTO work_plan_eggplan_item_binding \
                 (work_plan_id, work_item_id, eggplan_item_id) VALUES (?1, ?2, ?3)",
            )
            .bind(binding.work_plan_id.as_str())
            .bind(item.work_item_id.as_str())
            .bind(&item.eggplan_item_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        }
        let duplicate: Option<(String,)> = sqlx::query_as(
            "SELECT work_plan_id FROM work_plan_eggplan_binding \
             WHERE eggplan_repository_id = ?1 AND eggplan_plan_id = ?2 AND workspace_id = ?3 \
             AND binding_state != 'released' LIMIT 1",
        )
        .bind(&binding.eggplan_repository_id)
        .bind(&binding.eggplan_plan_id)
        .bind(&binding.workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        if duplicate.is_some() {
            return Err(BindingError::new(
                "eggplan_plan_already_bound",
                "another live CodeGG work plan already binds this repository plan",
            ));
        }
        sqlx::query(
            "INSERT INTO work_plan_eggplan_binding (work_plan_id, workspace_id, \
             codegg_repository_id, eggplan_repository_id, eggplan_plan_id, \
             last_seen_plan_revision, intent_digest, projection_digest, binding_state, \
             created_at, updated_at, released_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'synced', ?9, ?9, NULL)",
        )
        .bind(binding.work_plan_id.as_str())
        .bind(&binding.workspace_id)
        .bind(&binding.codegg_repository_id)
        .bind(&binding.eggplan_repository_id)
        .bind(&binding.eggplan_plan_id)
        .bind(binding.last_seen_plan_revision)
        .bind(&binding.intent_digest)
        .bind(&binding.projection_digest)
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        Ok(binding)
    }

    async fn session_project_id(&self, session_id: &str) -> Result<String, BindingError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            project_id: String,
        }
        let row: Option<Row> = sqlx::query_as("SELECT project_id FROM session WHERE id = ?1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| BindingError::new("binding_storage_error", error.to_string()))?;
        row.map(|row| row.project_id).ok_or_else(|| {
            BindingError::new(
                "session_not_found",
                "session row is required to mirror a repository plan",
            )
        })
    }

    // ── Structural drift and reconciliation (plan §11) ─────────────

    /// Reconcile one bound plan against its repository.
    ///
    /// Called before every bound mutation and at every
    /// assessment-completion boundary. Repository state is always re-read;
    /// canonical state is never reconstructed from the CodeGG mirror.
    pub async fn reconcile_bound_plan(
        &self,
        binding: &RepositoryPlanBinding,
        workspace_root: &Path,
    ) -> Result<ReconcileOutcome, BindingError> {
        // Always reconcile from the durable row. A caller's in-memory copy can
        // be stale, and a stale identity tuple must never be trusted.
        let Some(binding) = self.load_binding(&binding.work_plan_id).await? else {
            return Ok(ReconcileOutcome::Synced);
        };
        let store = open_store(workspace_root, false)?;
        if store.repository_id() != binding.eggplan_repository_id {
            self.mark_conflict(&binding).await?;
            return Err(BindingError::new(
                "repository_identity_mismatch",
                "repository identity changed since binding",
            ));
        }
        let plan_id = plan_id_of(&binding)?;
        let repository_plan = match store.get(&plan_id) {
            Ok(plan) => plan,
            Err(RepoError::NotFound(_)) => {
                self.mark_conflict(&binding).await?;
                return Err(BindingError::new(
                    "repository_plan_missing",
                    "bound repository plan no longer exists",
                ));
            }
            Err(error) => {
                self.mark_conflict(&binding).await?;
                return Err(repo_error("repository_plan_corrupt", error));
            }
        };
        if repository_plan.revision < binding.last_seen_plan_revision as u64 {
            self.mark_conflict(&binding).await?;
            return Err(BindingError::new(
                "repository_revision_regressed",
                "repository revision moved backwards",
            ));
        }
        if repository_plan.revision == binding.last_seen_plan_revision as u64 {
            if binding.binding_state == RepositoryBindingState::Conflict {
                return Err(BindingError::new(
                    "repository_binding_conflict",
                    "binding is conflicted and requires explicit repair",
                ));
            }
            if binding.binding_state == RepositoryBindingState::NeedsReconcile {
                // Interrupted cross-store write: the repository already
                // matches, so clear the flag and report Synced.
                self.bindings
                    .record_reconcile(
                        &binding.work_plan_id,
                        binding.last_seen_plan_revision,
                        &binding.intent_digest,
                        &binding.projection_digest,
                        RepositoryBindingState::Synced,
                    )
                    .await?;
            }
            return Ok(ReconcileOutcome::Synced);
        }
        // Terminal repository lifecycles are reconciled before any projection.
        // The Eggplan reverse projection deliberately refuses a terminal Plan,
        // and a terminal Plan is not a structural-drift concern: the mirror is
        // terminalized, never rebuilt.
        match repository_plan.status {
            PlanStatus::Closed => {
                if store
                    .closure_record(&plan_id)
                    .map_err(|error| repo_error("repository_closure_invalid", error))?
                    .is_none()
                {
                    self.mark_conflict(&binding).await?;
                    return Err(BindingError::new(
                        "repository_closure_missing",
                        "closed repository plan has no valid closure record",
                    ));
                }
                self.terminalize_mirror(&binding, &repository_plan, WorkPlanStatus::Completed)
                    .await?;
                return Ok(ReconcileOutcome::RepositoryClosed {
                    revision: repository_plan.revision,
                });
            }
            PlanStatus::Cancelled => {
                self.terminalize_mirror(&binding, &repository_plan, WorkPlanStatus::Cancelled)
                    .await?;
                return Ok(ReconcileOutcome::RepositoryCancelled {
                    revision: repository_plan.revision,
                });
            }
            _ => {}
        }
        // Newer repository revision: a structural intent change fails closed
        // instead of silently rebuilding a live mirror whose objective,
        // dependencies, or criteria changed while jobs may be in flight.
        let projection = project_repository_plan(&repository_plan).map_err(|error| {
            BindingError::new(
                "repository_plan_invalid",
                format!("cannot project repository plan: {error:?}"),
            )
        })?;
        if projection.intent_digest != binding.intent_digest {
            self.mark_conflict(&binding).await?;
            return Err(BindingError::new(
                "repository_plan_structure_changed",
                "repository plan structure changed after binding",
            ));
        }
        match projection.status {
            PlanStatus::Closed => {
                if store
                    .closure_record(&plan_id)
                    .map_err(|error| repo_error("repository_closure_invalid", error))?
                    .is_none()
                {
                    self.mark_conflict(&binding).await?;
                    return Err(BindingError::new(
                        "repository_closure_missing",
                        "closed repository plan has no valid closure record",
                    ));
                }
                self.sync_mirror(&binding.work_plan_id, &projection).await?;
                self.bindings
                    .record_reconcile(
                        &binding.work_plan_id,
                        projection.revision as i64,
                        &projection.intent_digest,
                        &projection.projection_digest,
                        RepositoryBindingState::Released,
                    )
                    .await?;
                Ok(ReconcileOutcome::RepositoryClosed {
                    revision: projection.revision,
                })
            }
            PlanStatus::Cancelled => {
                self.sync_mirror(&binding.work_plan_id, &projection).await?;
                self.bindings
                    .record_reconcile(
                        &binding.work_plan_id,
                        projection.revision as i64,
                        &projection.intent_digest,
                        &projection.projection_digest,
                        RepositoryBindingState::Released,
                    )
                    .await?;
                Ok(ReconcileOutcome::RepositoryCancelled {
                    revision: projection.revision,
                })
            }
            _ => {
                self.sync_mirror(&binding.work_plan_id, &projection).await?;
                self.bindings
                    .record_reconcile(
                        &binding.work_plan_id,
                        projection.revision as i64,
                        &projection.intent_digest,
                        &projection.projection_digest,
                        RepositoryBindingState::Synced,
                    )
                    .await?;
                Ok(ReconcileOutcome::Reconciled {
                    revision: projection.revision,
                })
            }
        }
    }

    async fn mark_conflict(&self, binding: &RepositoryPlanBinding) -> Result<(), BindingError> {
        if binding.binding_state != RepositoryBindingState::Released {
            self.bindings.mark_conflict(&binding.work_plan_id).await?;
        }
        Ok(())
    }

    /// Advance the CodeGG mirror lifecycle/blocker/next-action to match the
    /// repository. Item identity never changes: the durable item map stays
    /// the translation basis across restarts.
    async fn sync_mirror(
        &self,
        work_plan_id: &WorkPlanId,
        projection: &RepositoryPlanProjectionV1,
    ) -> Result<(), BindingError> {
        let mappings = self.bindings.list_item_bindings(work_plan_id).await?;
        for mapping in &mappings {
            let Some(item) = projection
                .items
                .iter()
                .find(|candidate| candidate.item_id.as_str() == mapping.eggplan_item_id)
            else {
                continue;
            };
            self.apply_mirror_item(mapping.work_item_id.clone(), item)
                .await?;
        }
        if let Some(plan) = self.plans.get(work_plan_id).await? {
            let target = map_plan_status(&projection.status);
            if plan.status != target {
                let _ = self
                    .plans
                    .transition_plan(&plan.id, plan.revision, target)
                    .await;
            }
        }
        Ok(())
    }

    /// Mirror one item's lifecycle/blocker/next-action into CodeGG. The
    /// mirror is authoritative for display only; the repository remains the
    /// lifecycle owner, and `Completed` is never mirrored from a projection
    /// because item completion is evidence-gated through
    /// [`Self::update_bound_item`].
    async fn apply_mirror_item(
        &self,
        work_item_id: WorkItemId,
        item: &RepositoryItemProjectionV1,
    ) -> Result<(), BindingError> {
        let Some(current) = self.plans.get_item(&work_item_id).await? else {
            return Ok(());
        };
        if current.status.is_terminal() {
            return Ok(());
        }
        let status = map_item_status(item.status);
        let mirror_blocker = item
            .blocker
            .as_deref()
            .map(|value| bounded(value, MAX_BLOCKER_CHARS))
            .filter(|value| !value.is_empty());
        let mirror_next = item
            .next_action
            .as_deref()
            .map(|value| bounded(value, MAX_NEXT_ACTION_CHARS))
            .filter(|value| !value.is_empty());
        if current.status == status
            && current.blocker == mirror_blocker
            && current.next_action == mirror_next
        {
            return Ok(());
        }
        if current.status == status {
            let _ = self
                .plans
                .update_item(
                    &work_item_id,
                    current.revision,
                    WorkItemPatch {
                        blocker: Some(mirror_blocker),
                        next_action: Some(mirror_next),
                        ..Default::default()
                    },
                )
                .await;
            return Ok(());
        }
        if status == WorkItemStatus::Completed {
            // Completion arrives only through the evidence-gated mutation.
            return Ok(());
        }
        let blocker_arg = if status == WorkItemStatus::Blocked {
            mirror_blocker
        } else {
            None
        };
        let _ = self
            .plans
            .transition_item(
                &work_item_id,
                current.revision,
                status,
                blocker_arg,
                mirror_next,
            )
            .await;
        Ok(())
    }

    // ── Cross-store mutation ordering (plan §12) ───────────────────

    /// Repository-first bound item lifecycle/blocker/next-action update.
    ///
    /// Ordering is deliberate: validate the caller's CodeGG expected
    /// revision, reconcile, construct the next repository revision, commit the
    /// repository `compare_and_swap`, then update the CodeGG mirror and the
    /// observed revision. A crash after the repository CAS is recoverable by
    /// mirror reconciliation; the repository is never rolled back.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_bound_item(
        &self,
        work_item_id: &WorkItemId,
        expected_revision: i64,
        new_status: WorkItemStatus,
        blocker: Option<String>,
        next_action: Option<String>,
        workspace_root: &Path,
    ) -> Result<(WorkPlan, WorkItem), BindingError> {
        let mapping = self
            .bindings
            .item_binding_for_work_item(work_item_id)
            .await?
            .ok_or_else(|| {
                BindingError::new(
                    "item_not_bound",
                    "item does not belong to a bound repository plan",
                )
            })?;
        let binding = self
            .load_binding(&mapping.work_plan_id)
            .await?
            .ok_or_else(|| {
                BindingError::new("binding_not_found", "item binding has no live plan binding")
            })?;
        let current_item = self
            .plans
            .get_item(work_item_id)
            .await?
            .ok_or_else(|| BindingError::new("item_not_found", "work item row is missing"))?;
        if current_item.revision != expected_revision {
            return Err(BindingError::new(
                "item_revision_conflict",
                format!(
                    "expected revision {expected_revision}, found {}",
                    current_item.revision
                ),
            ));
        }
        if new_status == WorkItemStatus::Completed
            && current_item.status == WorkItemStatus::Completed
        {
            return Ok((
                self.plans
                    .get(&current_item.plan_id)
                    .await?
                    .ok_or_else(|| BindingError::new("binding_not_found", "mirror plan is gone"))?,
                current_item,
            ));
        }
        self.reconcile_bound_plan(&binding, workspace_root).await?;
        let binding = self
            .load_binding(&mapping.work_plan_id)
            .await?
            .ok_or_else(|| {
                BindingError::new("binding_not_found", "binding disappeared during mutation")
            })?;
        let store = open_store(workspace_root, true)?;
        if store.repository_id() != binding.eggplan_repository_id {
            self.mark_conflict(&binding).await?;
            return Err(BindingError::new(
                "repository_identity_mismatch",
                "repository identity changed since binding",
            ));
        }
        let plan_id = plan_id_of(&binding)?;
        let mut repository_plan = store
            .get(&plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        let eggplan_item_id = PlanItemId::new(&mapping.eggplan_item_id)
            .map_err(|error| BindingError::new("eggplan_item_id_invalid", error.to_string()))?;
        let index = repository_plan
            .items
            .iter()
            .position(|item| item.id == eggplan_item_id)
            .ok_or_else(|| {
                BindingError::new(
                    "repository_item_missing",
                    "bound item is absent from the repository plan",
                )
            })?;
        if new_status == WorkItemStatus::Completed {
            // A model's `status=completed` request is a proposal, not
            // evidence: sync terminal evidence and assess the canonical
            // repository item first.
            self.sync_terminal_evidence(&binding, workspace_root)
                .await?;
            self.assert_repository_item_complete(
                &store,
                &plan_id,
                &eggplan_item_id,
                workspace_root,
            )
            .await?;
        }
        {
            let item = &mut repository_plan.items[index];
            item.status = map_work_item_status(new_status);
            item.blocker = if new_status == WorkItemStatus::Blocked {
                Some(
                    blocker
                        .clone()
                        .or_else(|| item.blocker.clone())
                        .unwrap_or_else(|| "blocked".to_string()),
                )
            } else {
                None
            };
            item.next_action = next_action.clone().or_else(|| item.next_action.clone());
        }
        let repository_revision = repository_plan.revision;
        repository_plan.revision += 1;
        let committed = store
            .compare_and_swap(&plan_id, repository_revision, &repository_plan)
            .map_err(|error| match error {
                RepoError::Conflict { .. } => BindingError::new(
                    "repository_revision_conflict",
                    "repository plan advanced concurrently",
                ),
                RepoError::InvalidTransition => BindingError::new(
                    "unsupported_bound_transition",
                    "repository rejected the transition",
                ),
                other => repo_error("repository_update_failed", other),
            })?;
        let projection = project_repository_plan(&committed).map_err(|error| {
            BindingError::new(
                "repository_plan_invalid",
                format!("cannot project repository plan: {error:?}"),
            )
        })?;
        if projection.intent_digest != binding.intent_digest {
            // Unreachable after the pre-CAS reconcile; fail closed rather
            // than record a diverged binding.
            self.mark_conflict(&binding).await?;
            return Err(BindingError::new(
                "repository_plan_structure_changed",
                "repository plan structure changed during mutation",
            ));
        }
        // Mirror last: the CodeGG write is the recoverable side.
        let mirror_blocker = if new_status == WorkItemStatus::Blocked {
            blocker.clone()
        } else {
            None
        };
        let (plan, item) = self
            .plans
            .transition_item(
                work_item_id,
                expected_revision,
                new_status,
                mirror_blocker,
                next_action,
            )
            .await?;
        self.bindings
            .record_reconcile(
                &binding.work_plan_id,
                committed.revision as i64,
                &projection.intent_digest,
                &projection.projection_digest,
                RepositoryBindingState::Synced,
            )
            .await?;
        Ok((plan, item))
    }

    /// Require that the repository assessment reports the mapped item
    /// Complete. Missing, failed, stale, or unbound evidence fails closed.
    async fn assert_repository_item_complete(
        &self,
        store: &RepositoryStore,
        plan_id: &PlanId,
        item_id: &PlanItemId,
        workspace_root: &Path,
    ) -> Result<(), BindingError> {
        let plan = store
            .get(plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        let subject = store
            .subject_source()
            .capture()
            .map_err(|error| subject_error("repository_subject_capture_failed", error))?;
        let _ = workspace_root;
        let observations = store
            .list_observations(plan_id)
            .map_err(|error| repo_error("repository_evidence_unavailable", error))?;
        let supersessions = store
            .list_supersessions(plan_id)
            .map_err(|error| repo_error("repository_evidence_unavailable", error))?;
        let effective = effective_observations(&observations, &supersessions)
            .map_err(|reason| BindingError::new("repository_evidence_invalid", reason))?
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let providers = bound_provider_registry()?;
        let assessment = assess_plan(&plan, &subject, &effective, &providers);
        let Some(item_assessment) = assessment
            .items
            .iter()
            .find(|candidate| candidate.item_id == *item_id)
        else {
            return Err(BindingError::new(
                "repository_item_missing",
                "bound item is absent from the repository assessment",
            ));
        };
        // Completion is gated on the item's acceptance criteria, not on its
        // lifecycle: the item is still non-Completed in the repository until
        // this very CAS lands.
        if item_assessment.criteria.is_empty()
            || item_assessment
                .criteria
                .iter()
                .any(|criterion| criterion.status != AssessmentStatus::Complete)
        {
            return Err(BindingError::new(
                "repository_item_evidence_incomplete",
                "repository assessment does not report every criterion complete",
            ));
        }
        Ok(())
    }

    // ── Evidence writeback (plan §13) ───────────────────────────────

    /// Bounded terminal evidence sync over the mapped CodeGG items.
    ///
    /// Only durable terminal observations are persisted. Transient
    /// `InProgress` is never written under an observation id a later
    /// terminal body would reuse. Appends are idempotent by deterministic
    /// observation identity; the same id with different bytes is a hard error.
    pub async fn sync_terminal_evidence(
        &self,
        binding: &RepositoryPlanBinding,
        workspace_root: &Path,
    ) -> Result<EvidenceSyncReport, BindingError> {
        let plan = self
            .plans
            .get(&binding.work_plan_id)
            .await?
            .ok_or_else(|| BindingError::new("binding_not_found", "mirror plan is gone"))?;
        let items = self.plans.list_items(&plan.id).await?;
        let mappings = self.bindings.list_item_bindings(&plan.id).await?;
        if mappings.is_empty() {
            return Ok(EvidenceSyncReport::default());
        }
        let store = open_store(workspace_root, true)?;
        if store.repository_id() != binding.eggplan_repository_id {
            self.mark_conflict(binding).await?;
            return Err(BindingError::new(
                "repository_identity_mismatch",
                "repository identity changed since binding",
            ));
        }
        let plan_id = plan_id_of(binding)?;
        let resolved = crate::work_plan_evidence::assemble_resolved(&self.pool, &items)
            .await
            .map_err(|error| BindingError::new("evidence_resolution_failed", error))?;
        let job_store = SqliteJobStore::new(self.pool.clone());
        let mut report = EvidenceSyncReport::default();
        for entry in &resolved.entries {
            // Only refs attached to a mapped bound item are in scope; an
            // unmapped ref belongs to no repository Plan.
            if !mappings.iter().any(|mapping| {
                items.iter().any(|item| {
                    item.id == mapping.work_item_id
                        && item
                            .evidence
                            .iter()
                            .any(|evidence| evidence.ref_id == entry.ref_id)
                })
            }) {
                continue;
            }
            let status = match entry.status {
                HostEvidenceStatus::Passed => EvidenceStatus::Passed,
                HostEvidenceStatus::Failed => EvidenceStatus::Failed,
                HostEvidenceStatus::InProgress | HostEvidenceStatus::Unavailable => continue,
            };
            let observation = self
                .build_terminal_observation(binding, &job_store, entry, status, workspace_root)
                .await?;
            let existing = match store.get_observation(&plan_id, observation.id()) {
                Ok(record) => Some(record),
                Err(RepoError::ObservationNotFound(_)) => None,
                Err(error) => {
                    return Err(repo_error("repository_evidence_unavailable", error));
                }
            };
            match existing {
                Some(previous) => {
                    if previous.content_digest() != observation.content_digest() {
                        return Err(BindingError::new(
                            "evidence_observation_conflict",
                            "same observation id already exists with different content",
                        ));
                    }
                    report.idempotent += 1;
                }
                None => {
                    store.append_observation(&plan_id, &observation).map_err(
                        |error| match error {
                            RepoError::ObservationConflict(_) => BindingError::new(
                                "evidence_observation_conflict",
                                "same observation id already exists with different content",
                            ),
                            other => repo_error("repository_evidence_append_failed", other),
                        },
                    )?;
                    report.appended += 1;
                }
            }
        }
        Ok(report)
    }

    /// Build one schema-v2 terminal observation with authoritative
    /// verification binding, translated subject, and verified RunStore
    /// artifact provenance.
    async fn build_terminal_observation(
        &self,
        binding: &RepositoryPlanBinding,
        job_store: &SqliteJobStore,
        entry: &crate::work_plan_evidence::ResolvedWorkEvidence,
        status: EvidenceStatus,
        workspace_root: &Path,
    ) -> Result<EvidenceObservation, BindingError> {
        let provenance = entry.source_subject.as_ref().ok_or_else(|| {
            BindingError::new(
                "evidence_subject_unavailable",
                "terminal evidence has no historical subject",
            )
        })?;
        if provenance.disposition != ExecutionSubjectDisposition::Stable {
            return Err(BindingError::new(
                "evidence_subject_unstable",
                "terminal evidence subject is not stable",
            ));
        }
        if let Some(materialization) = provenance.materialization.as_ref() {
            if !materialization.complete {
                return Err(BindingError::new(
                    "evidence_materialization_incomplete",
                    "incomplete materialization is not exact-subject evidence",
                ));
            }
        }
        let historical = provenance
            .sealed
            .as_ref()
            .or(provenance.captured.as_ref())
            .ok_or_else(|| {
                BindingError::new(
                    "evidence_subject_unavailable",
                    "terminal evidence has no historical revision",
                )
            })?;
        let job_id = entry.native_job_id.as_deref().ok_or_else(|| {
            BindingError::new(
                "evidence_native_missing",
                "terminal evidence has no native job",
            )
        })?;
        let job: JobRecord = job_store
            .get_job(&JobId::new_unchecked(job_id.to_string()))
            .await
            .map_err(|error| BindingError::new("evidence_native_missing", error.to_string()))?
            .ok_or_else(|| {
                BindingError::new(
                    "evidence_native_missing",
                    "terminal evidence references a missing job",
                )
            })?;
        if !kind_matches_payload(entry.kind, &job) {
            return Err(BindingError::new(
                "evidence_kind_mismatch",
                "evidence ref kind does not match its native payload",
            ));
        }
        let digest = crate::work_plan_eggplan::verification_digest_for_job(&job)
            .map_err(|reason| BindingError::new("verification_unavailable", reason))?;
        let subject = translate_historical_subject(binding, historical)?;
        let attempts = job_store
            .list_attempts(&job.job_id)
            .await
            .map_err(|error| BindingError::new("evidence_native_missing", error.to_string()))?;
        let attempt = entry.native_attempt_id.as_deref().and_then(|attempt_id| {
            attempts
                .iter()
                .find(|candidate| candidate.attempt_id.as_str() == attempt_id)
        });
        let observed_at_unix_ms = attempt
            .and_then(|record| record.completed_at)
            .or(job.terminal_at)
            .map(|timestamp| timestamp.timestamp_millis().max(0) as u64)
            .ok_or_else(|| {
                BindingError::new(
                    "evidence_timestamp_missing",
                    "terminal evidence has no durable terminal timestamp",
                )
            })?;
        let mut metadata: BTreeMap<String, String> = BTreeMap::new();
        metadata.insert("codegg.ref_id".to_string(), bounded_id(&entry.ref_id));
        if let Some(attempt_id) = entry.native_attempt_id.as_deref() {
            metadata.insert("codegg.native_attempt".to_string(), bounded_id(attempt_id));
        }
        metadata.insert("codegg.native_job".to_string(), bounded_id(job_id));
        let artifacts =
            verified_artifact_refs(workspace_root, entry.native_run_id.as_deref()).await?;
        let invocation_ref = format!(
            "{}:{}",
            job_id,
            entry.native_attempt_id.as_deref().unwrap_or("")
        );
        EvidenceObservation::finalize(EvidenceObservationInput {
            id: bound_observation_id(
                entry.kind,
                &entry.ref_id,
                attempt.map(|record| record.sequence),
            ),
            provider_id: host_provider_id()?,
            kind: bound_evidence_kind(entry.kind),
            status,
            subject,
            observed_at_unix_ms,
            invocation_ref: Some(bounded_id(&invocation_ref)),
            verification_digest: Some(digest),
            result_metadata: metadata,
            artifacts,
        })
        .map_err(|error| BindingError::new("evidence_observation_invalid", format!("{error:?}")))
    }

    // ── Bound assessment (plan §14) ─────────────────────────────────

    /// Assess a bound plan from canonical repository state.
    ///
    /// The CodeGG mirror is never used to reconstruct acceptance semantics.
    /// Callers own evidence writeback; this reads repository state only.
    pub async fn assess_bound_plan(
        &self,
        binding: &RepositoryPlanBinding,
        workspace_root: &Path,
    ) -> Result<BoundAssessment, BindingError> {
        self.reconcile_bound_plan(binding, workspace_root).await?;
        let binding = self
            .load_binding(&binding.work_plan_id)
            .await?
            .ok_or_else(|| BindingError::new("binding_not_found", "binding is no longer live"))?;
        if binding.binding_state == RepositoryBindingState::Conflict {
            return Err(BindingError::new(
                "repository_binding_conflict",
                "binding is conflicted and requires explicit repair",
            ));
        }
        let store = open_store(workspace_root, false)?;
        if store.repository_id() != binding.eggplan_repository_id {
            return Err(BindingError::new(
                "repository_identity_mismatch",
                "repository identity changed since binding",
            ));
        }
        let plan_id = plan_id_of(&binding)?;
        let plan = store
            .get(&plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        let subject = store
            .subject_source()
            .capture()
            .map_err(|error| subject_error("repository_subject_capture_failed", error))?;
        let observations = store
            .list_observations(&plan_id)
            .map_err(|error| repo_error("repository_evidence_unavailable", error))?;
        let supersessions = store
            .list_supersessions(&plan_id)
            .map_err(|error| repo_error("repository_evidence_unavailable", error))?;
        let effective = effective_observations(&observations, &supersessions)
            .map_err(|reason| BindingError::new("repository_evidence_invalid", reason))?
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let providers = bound_provider_registry()?;
        Ok(BoundAssessment {
            assessment: assess_plan(&plan, &subject, &effective, &providers),
            revision: plan.revision,
            subject,
        })
    }

    // ── Guarded closure (plan §16) ──────────────────────────────────

    /// Close a bound plan only through Eggplan guarded finalization.
    ///
    /// The Eggplan finalizer owns its own S1/S2 capture; CodeGG never injects
    /// a subject as finalization authority. Only after the guarded closure
    /// succeeds is the CodeGG mirror terminalized and the binding released,
    /// so a crash in between is repaired by restart reconciliation.
    pub async fn finalize_bound_plan(
        &self,
        binding: &RepositoryPlanBinding,
        workspace_root: &Path,
    ) -> Result<bool, BindingError> {
        self.reconcile_bound_plan(binding, workspace_root).await?;
        let binding = self
            .load_binding(&binding.work_plan_id)
            .await?
            .ok_or_else(|| BindingError::new("binding_not_found", "binding is no longer live"))?;
        if binding.binding_state == RepositoryBindingState::Conflict {
            return Err(BindingError::new(
                "repository_binding_conflict",
                "binding is conflicted and requires explicit repair",
            ));
        }
        self.sync_terminal_evidence(&binding, workspace_root)
            .await?;
        let store = open_store(workspace_root, true)?;
        if store.repository_id() != binding.eggplan_repository_id {
            self.mark_conflict(&binding).await?;
            return Err(BindingError::new(
                "repository_identity_mismatch",
                "repository identity changed since binding",
            ));
        }
        let plan_id = plan_id_of(&binding)?;
        let plan = store
            .get(&plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        let subject = store
            .subject_source()
            .capture()
            .map_err(|error| subject_error("repository_subject_capture_failed", error))?;
        let observations = store
            .list_observations(&plan_id)
            .map_err(|error| repo_error("repository_evidence_unavailable", error))?;
        let supersessions = store
            .list_supersessions(&plan_id)
            .map_err(|error| repo_error("repository_evidence_unavailable", error))?;
        let effective = effective_observations(&observations, &supersessions)
            .map_err(|reason| BindingError::new("repository_evidence_invalid", reason))?
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let providers = bound_provider_registry()?;
        let assessment = assess_plan(&plan, &subject, &effective, &providers);
        if assessment.status != AssessmentStatus::Complete {
            return Err(BindingError::new(
                "repository_not_complete",
                "repository assessment is not complete",
            ));
        }
        let candidate = ClosureCandidate::build(
            &plan,
            subject,
            assessment,
            &observations,
            &supersessions,
            bound_provider_policy(),
            now_unix_ms(),
        )
        .map_err(|reason| BindingError::new("closure_candidate_invalid", reason))?;
        let closure_id = ClosureId::new(format!(
            "epcl_{}_{}",
            bounded_id(&binding.eggplan_plan_id),
            candidate.source_revision
        ))
        .map_err(|error| BindingError::new("closure_id_invalid", error.to_string()))?;
        let (closed, _record) = store
            .finalize_closure(&candidate, closure_id, now_unix_ms())
            .map_err(|error| match error {
                RepoError::ClosureSubjectStale | RepoError::ClosureSubjectDrift => {
                    BindingError::new(
                        "closure_subject_drift",
                        "repository subject changed during guarded finalization",
                    )
                }
                RepoError::InvalidUpdate => BindingError::new(
                    "closure_rejected",
                    "repository rejected the guarded closure candidate",
                ),
                other => repo_error("closure_failed", other),
            })?;
        self.terminalize_mirror(&binding, &closed, WorkPlanStatus::Completed)
            .await?;
        Ok(true)
    }

    /// Cancel a bound plan as a canonical repository lifecycle operation.
    ///
    /// Repository CAS to `Cancelled` first, then mirror and release. M003 does
    /// not expose a general "detach while active" operation: unbinding an
    /// Active repository Plan would leave ambiguous runtime ownership.
    pub async fn cancel_bound_plan(
        &self,
        binding: &RepositoryPlanBinding,
        workspace_root: &Path,
    ) -> Result<(), BindingError> {
        let store = open_store(workspace_root, true)?;
        if store.repository_id() != binding.eggplan_repository_id {
            self.mark_conflict(binding).await?;
            return Err(BindingError::new(
                "repository_identity_mismatch",
                "repository identity changed since binding",
            ));
        }
        let plan_id = plan_id_of(binding)?;
        let mut plan = store
            .get(&plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        if matches!(plan.status, PlanStatus::Closed | PlanStatus::Cancelled) {
            return Err(BindingError::new(
                "repository_plan_terminal",
                "repository plan is already terminal",
            ));
        }
        let expected = plan.revision;
        plan.revision += 1;
        plan.status = PlanStatus::Cancelled;
        let committed = store
            .compare_and_swap(&plan_id, expected, &plan)
            .map_err(|error| repo_error("repository_update_failed", error))?;
        self.terminalize_mirror(binding, &committed, WorkPlanStatus::Cancelled)
            .await?;
        Ok(())
    }

    async fn terminalize_mirror(
        &self,
        binding: &RepositoryPlanBinding,
        repository_plan: &Plan,
        target: WorkPlanStatus,
    ) -> Result<(), BindingError> {
        let projection_digest = project_repository_plan(repository_plan)
            .map(|value| value.projection_digest)
            .unwrap_or_else(|_| binding.projection_digest.clone());
        if let Some(mirror) = self.plans.get(&binding.work_plan_id).await? {
            if mirror.status != target {
                let _ = self
                    .plans
                    .transition_plan(&mirror.id, mirror.revision, target)
                    .await;
            }
        }
        self.bindings
            .record_reconcile(
                &binding.work_plan_id,
                repository_plan.revision as i64,
                &binding.intent_digest,
                &projection_digest,
                RepositoryBindingState::Released,
            )
            .await?;
        Ok(())
    }

    // ── WorkOrder binding (plan §18) ────────────────────────────────

    /// Record a host-authorized one-shot WorkOrder binding request.
    ///
    /// This is a validated request, never scheduler authority. `repeat_count`
    /// must be exactly one: one canonical repository Plan cannot back several
    /// occurrences.
    pub async fn bind_work_order_plan(
        &self,
        work_order_id: &str,
        repeat_count: u32,
        workspace_root: &Path,
    ) -> Result<RepositoryWorkOrderBinding, BindingError> {
        if repeat_count != 1 {
            return Err(BindingError::new(
                "work_order_repeat_unsupported",
                "repository plan binding supports exactly one occurrence",
            ));
        }
        let relation = self.relation_for_root(workspace_root).await?;
        let store = open_store(workspace_root, false)?;
        let identity = prove_identity(&relation, &store).await?;
        let plan_id = self.single_bindable_plan(&store)?;
        let plan = store
            .get(&plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        let projection = project_repository_plan(&plan).map_err(|error| {
            BindingError::new(
                "eggplan_plan_not_bindable",
                format!("repository plan is not bindable: {error:?}"),
            )
        })?;
        validate_requirement_profile(&projection)?;
        let request = RepositoryWorkOrderBinding {
            work_order_id: work_order_id.to_string(),
            codegg_repository_id: relation.codegg_repository_id.clone(),
            eggplan_repository_id: identity.eggplan_repository_id.clone(),
            eggplan_plan_id: plan_id.as_str().to_string(),
            intent_digest: projection.intent_digest.clone(),
            created_at: chrono::Utc::now(),
        };
        self.bindings.create_work_order_binding(&request).await?;
        Ok(request)
    }

    pub async fn work_order_binding(
        &self,
        work_order_id: &str,
    ) -> Result<Option<RepositoryWorkOrderBinding>, BindingError> {
        Ok(self.bindings.get_work_order_binding(work_order_id).await?)
    }

    /// Author a binding request for one specific repository plan.
    pub async fn bind_work_order_plan_id(
        &self,
        work_order_id: &str,
        repeat_count: u32,
        eggplan_plan_id: &str,
        workspace_root: &Path,
    ) -> Result<RepositoryWorkOrderBinding, BindingError> {
        if repeat_count != 1 {
            return Err(BindingError::new(
                "work_order_repeat_unsupported",
                "repository plan binding supports exactly one occurrence",
            ));
        }
        let plan_id = PlanId::new(eggplan_plan_id)
            .map_err(|error| BindingError::new("eggplan_plan_id_invalid", error.to_string()))?;
        let relation = self.relation_for_root(workspace_root).await?;
        let store = open_store(workspace_root, false)?;
        let identity = prove_identity(&relation, &store).await?;
        let plan = store.get(&plan_id).map_err(|error| {
            if matches!(error, RepoError::NotFound(_)) {
                BindingError::new("eggplan_plan_not_found", error.to_string())
            } else {
                repo_error("eggplan_plan_invalid", error)
            }
        })?;
        let projection = project_repository_plan(&plan).map_err(|error| {
            BindingError::new(
                "eggplan_plan_not_bindable",
                format!("repository plan is not bindable: {error:?}"),
            )
        })?;
        validate_requirement_profile(&projection)?;
        let request = RepositoryWorkOrderBinding {
            work_order_id: work_order_id.to_string(),
            codegg_repository_id: relation.codegg_repository_id.clone(),
            eggplan_repository_id: identity.eggplan_repository_id.clone(),
            eggplan_plan_id: plan_id.as_str().to_string(),
            intent_digest: projection.intent_digest.clone(),
            created_at: chrono::Utc::now(),
        };
        self.bindings.create_work_order_binding(&request).await?;
        Ok(request)
    }

    /// Re-validate a WorkOrder binding request against a concrete shared
    /// workspace and materialize the occurrence session's mirror.
    ///
    /// Called after the coordinator resolved the shared workspace and before
    /// the initial turn/job is submitted. Failure records occurrence
    /// Attention; it never launches unbound.
    pub async fn materialize_work_order_binding(
        &self,
        request: &RepositoryWorkOrderBinding,
        session_id: &str,
        workspace_root: &Path,
    ) -> Result<RepositoryPlanBinding, BindingError> {
        let relation = self.relation_for_root(workspace_root).await?;
        let store = open_store(workspace_root, false)?;
        let identity = prove_identity(&relation, &store).await?;
        if identity.eggplan_repository_id != request.eggplan_repository_id {
            return Err(BindingError::new(
                "work_order_repository_changed",
                "repository identity changed since the binding request was authored",
            ));
        }
        if !request.codegg_repository_id.is_empty()
            && relation.codegg_repository_id != request.codegg_repository_id
        {
            return Err(BindingError::new(
                "work_order_repository_changed",
                "CodeGG repository identity changed since the binding request was authored",
            ));
        }
        let plan_id = PlanId::new(&request.eggplan_plan_id)
            .map_err(|error| BindingError::new("eggplan_plan_id_invalid", error.to_string()))?;
        let plan = store
            .get(&plan_id)
            .map_err(|error| repo_error("repository_plan_unavailable", error))?;
        let projection = project_repository_plan(&plan).map_err(|error| {
            BindingError::new(
                "eggplan_plan_not_bindable",
                format!("repository plan is not bindable: {error:?}"),
            )
        })?;
        if projection.intent_digest != request.intent_digest {
            return Err(BindingError::new(
                "work_order_intent_changed",
                "repository plan structure changed since the binding request was authored",
            ));
        }
        validate_requirement_profile(&projection)?;
        self.create_mirror(
            session_id,
            &relation,
            &request.eggplan_repository_id,
            &request.eggplan_plan_id,
            &projection,
        )
        .await
    }

    /// Managed worktrees do not reliably carry the repository-local
    /// untracked `.eggplan` root, so a mutation-capable auto-isolated
    /// occurrence moves to Attention rather than copying repository state.
    pub fn require_shared_repository_state(action: &WorkspaceAction) -> Result<(), BindingError> {
        match action {
            WorkspaceAction::ShareReadOnly | WorkspaceAction::ShareSerialized => Ok(()),
            WorkspaceAction::UseManagedWorktree => Err(BindingError::new(
                "eggplan_binding_requires_shared_repository_state",
                "managed worktrees do not carry the repository-local Eggplan state root",
            )),
            WorkspaceAction::NeedsAttention { diagnostic, .. } => Err(BindingError::new(
                "eggplan_binding_requires_shared_repository_state",
                format!("workspace cannot be materialized: {diagnostic}"),
            )),
        }
    }

    fn single_bindable_plan(&self, store: &RepositoryStore) -> Result<PlanId, BindingError> {
        let mut bindable = Vec::new();
        for plan_id in store
            .list()
            .map_err(|error| repo_error("repository_plan_unavailable", error))?
        {
            if let Ok(plan) = store.get(&plan_id) {
                if project_repository_plan(&plan).is_ok() {
                    bindable.push(plan_id);
                }
            }
        }
        if bindable.len() != 1 {
            return Err(BindingError::new(
                "work_order_plan_ambiguous",
                "work order binding requires exactly one bindable repository plan",
            ));
        }
        Ok(bindable.remove(0))
    }

    async fn relation_for_root(
        &self,
        workspace_root: &Path,
    ) -> Result<WorkspaceRelation, BindingError> {
        let store = Arc::new(SqliteWorkspaceStore::new(self.pool.clone()));
        let registry = WorkspaceRegistry::load(store).await.map_err(|error| {
            BindingError::new("workspace_registry_unavailable", error.to_string())
        })?;
        let record = registry.resolve_root(workspace_root).await.ok_or_else(|| {
            BindingError::new(
                "workspace_not_registered",
                "workspace root is not a registered CodeGG workspace",
            )
        })?;
        let workspace_id = record.id.as_str().to_string();
        let storage = ProjectStorage::new(self.pool.clone());
        let workspace = storage
            .workspace_binding(&record.id)
            .await
            .map_err(|error| BindingError::new("workspace_binding_unavailable", error.to_string()))?
            .ok_or_else(|| {
                BindingError::new(
                    "workspace_binding_missing",
                    "workspace has no canonical binding record",
                )
            })?;
        let codegg_repository_id = workspace
            .repository_id
            .map(|id| id.as_str().to_string())
            .unwrap_or_default();
        Ok(WorkspaceRelation {
            workspace_id,
            codegg_repository_id,
            workspace_root: workspace_root.to_path_buf(),
        })
    }
}

// ── Free helpers ────────────────────────────────────────────────────────

/// Resolve the canonical session -> project/workspace/repository relation
/// through `ProjectStorage`. A session without a resolved workspace binding
/// has no bindable identity; the error is never softened into a guess.
pub async fn session_workspace_relation(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<WorkspaceRelation, BindingError> {
    let storage = ProjectStorage::new(pool.clone());
    let session = storage
        .session_binding(session_id)
        .await
        .map_err(|error| BindingError::new("session_binding_unavailable", error.to_string()))?
        .ok_or_else(|| {
            BindingError::new("session_binding_missing", "session has no project binding")
        })?;
    let workspace_id: WorkspaceId = session.workspace_id.ok_or_else(|| {
        BindingError::new(
            "session_binding_missing",
            "session is not bound to a canonical workspace",
        )
    })?;
    let workspace = storage
        .workspace_binding(&workspace_id)
        .await
        .map_err(|error| BindingError::new("workspace_binding_unavailable", error.to_string()))?
        .ok_or_else(|| {
            BindingError::new(
                "workspace_binding_missing",
                "workspace has no canonical binding record",
            )
        })?;
    let repository_id = workspace.repository_id.ok_or_else(|| {
        BindingError::new(
            "workspace_repository_unbound",
            "workspace is not bound to a CodeGG repository",
        )
    })?;
    let workspace_root = resolve_workspace_root(pool, &workspace_id).await?;
    Ok(WorkspaceRelation {
        workspace_id: workspace_id.as_str().to_string(),
        codegg_repository_id: repository_id.as_str().to_string(),
        workspace_root,
    })
}

/// Resolve a registered workspace root from the durable workspace registry.
pub async fn resolve_workspace_root(
    pool: &SqlitePool,
    workspace_id: &WorkspaceId,
) -> Result<PathBuf, BindingError> {
    let store = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    let registry = WorkspaceRegistry::load(store)
        .await
        .map_err(|error| BindingError::new("workspace_registry_unavailable", error.to_string()))?;
    let record = registry.resolve(workspace_id).await.ok_or_else(|| {
        BindingError::new(
            "workspace_not_registered",
            "workspace is not present in the durable registry",
        )
    })?;
    Ok(record.canonical_root.clone())
}

fn plan_id_of(binding: &RepositoryPlanBinding) -> Result<PlanId, BindingError> {
    PlanId::new(&binding.eggplan_plan_id)
        .map_err(|error| BindingError::new("eggplan_plan_id_invalid", error.to_string()))
}

fn binding_plan_key(plan_id: &PlanId) -> String {
    plan_id.as_str().to_string()
}

fn find_mapped_item(
    mappings: &[RepositoryItemBinding],
    eggplan_item_id: &str,
) -> Option<WorkItemId> {
    mappings
        .iter()
        .find(|mapping| mapping.eggplan_item_id == eggplan_item_id)
        .map(|mapping| mapping.work_item_id.clone())
}

fn kind_matches_payload(kind: WorkEvidenceKind, job: &JobRecord) -> bool {
    use codegg_core::jobs::JobPayload;
    matches!(
        (kind, &job.payload),
        (WorkEvidenceKind::TestJob, JobPayload::Test { .. })
            | (
                WorkEvidenceKind::SchedulerJob,
                JobPayload::ManagedArgv { .. }
                    | JobPayload::Shell { .. }
                    | JobPayload::Python { .. }
                    | JobPayload::Git { .. }
            )
            | (
                WorkEvidenceKind::DelegatedRun,
                JobPayload::SubagentRun { .. }
            )
            | (WorkEvidenceKind::AgentRun, JobPayload::SubagentRun { .. })
    )
}

fn bound_evidence_kind(kind: WorkEvidenceKind) -> EvidenceKind {
    match kind {
        WorkEvidenceKind::TestJob => EvidenceKind::Test,
        WorkEvidenceKind::SchedulerJob => EvidenceKind::Command,
        WorkEvidenceKind::DelegatedRun | WorkEvidenceKind::AgentRun => EvidenceKind::DelegatedRun,
        WorkEvidenceKind::Artifact => EvidenceKind::Artifact,
        WorkEvidenceKind::Commit => EvidenceKind::Revision,
    }
}

fn bound_observation_id(
    kind: WorkEvidenceKind,
    ref_id: &str,
    attempt_seq: Option<u32>,
) -> EvidenceObservationId {
    let base = format!(
        "epe_codegg-{}-{}-a{}",
        kind.as_str(),
        bounded_id(ref_id),
        attempt_seq.unwrap_or(0)
    );
    EvidenceObservationId::new(bounded(&base, MAX_OBSERVATION_ID_CHARS))
        .expect("observation id is valid")
}

/// Verified RunStore artifact provenance for one terminal run.
///
/// Artifact records are read back through `RunStore` and their SHA-256
/// digests re-verified before they become Eggplan `ArtifactRef` values. This
/// is artifact *provenance* attached to a terminal execution observation, not
/// standalone `EvidenceKind::Artifact` requirement authority, which stays
/// deferred.
async fn verified_artifact_refs(
    workspace_root: &Path,
    run_id: Option<&str>,
) -> Result<Vec<ArtifactRef>, BindingError> {
    let Some(run_id) = run_id else {
        return Ok(Vec::new());
    };
    let store = FsRunStore::new(run_store_root(workspace_root));
    let manifest = store
        .get_run(&RunId::new_unchecked(run_id.to_string()))
        .await
        .map_err(|error| BindingError::new("artifact_record_unavailable", error.to_string()))?
        .ok_or_else(|| {
            BindingError::new(
                "artifact_record_missing",
                "terminal run has no durable run record",
            )
        })?;
    let mut refs = Vec::new();
    for artifact in manifest.artifacts.iter().take(MAX_ARTIFACT_REFS) {
        let chunk = store
            .read_artifact(&artifact.artifact_id, None)
            .await
            .map_err(|error| BindingError::new("artifact_read_failed", error.to_string()))?;
        use sha2::Digest;
        if hex::encode(sha2::Sha256::digest(&chunk.data)) != artifact.sha256 {
            return Err(BindingError::new(
                "artifact_digest_mismatch",
                "run store artifact digest does not match its bytes",
            ));
        }
        refs.push(ArtifactRef {
            reference: format!(
                "{}:{}:{}",
                bounded_id(run_id),
                bounded_id(artifact.artifact_id.as_str()),
                artifact.byte_length
            ),
            digest: Some(format!("sha256:{}", artifact.sha256)),
            media_type: Some(bounded(&artifact.mime_type, 128)),
        });
    }
    Ok(refs)
}

/// Verify that a CodeGG mirror is a faithful projection of the canonical
/// repository Plan. Used by status reporting and qualification tests.
pub async fn mirror_matches_repository(
    service: &RepositoryBindingService,
    work_plan_id: &WorkPlanId,
    workspace_root: &Path,
) -> Result<bool, BindingError> {
    let Some(binding) = service.load_binding(work_plan_id).await? else {
        return Ok(false);
    };
    let store = open_store(workspace_root, false)?;
    let plan = store
        .get(&plan_id_of(&binding)?)
        .map_err(|error| repo_error("repository_plan_unavailable", error))?;
    let projection = project_repository_plan(&plan)
        .map_err(|error| BindingError::new("repository_plan_invalid", format!("{error:?}")))?;
    Ok(projection.intent_digest == binding.intent_digest
        && projection.projection_digest == binding.projection_digest
        && projection.revision as i64 == binding.last_seen_plan_revision)
}

/// Identity of the immutable Eggplan revision this build consumes.
pub fn eggplan_pin() -> &'static str {
    EGGPLAN_PIN
}

/// Stable wire name for a canonical Eggplan assessment status.
pub fn assessment_status_str(status: &AssessmentStatus) -> &'static str {
    match status {
        AssessmentStatus::Complete => "complete",
        AssessmentStatus::ActionableWorkRemaining => "actionable_work_remaining",
        AssessmentStatus::Blocked => "blocked",
        AssessmentStatus::EvidenceFailed => "evidence_failed",
        AssessmentStatus::EvidenceMissingOrUnavailable => "evidence_missing_or_unavailable",
        AssessmentStatus::InFlight => "in_flight",
        AssessmentStatus::AwaitingHumanJudgment => "awaiting_human_judgment",
        AssessmentStatus::Inconclusive => "inconclusive",
        AssessmentStatus::InvalidOrStale => "invalid_or_stale",
    }
}

/// Project the canonical repository assessment into the existing CodeGG
/// completion DTO.
///
/// The repository assessment is authoritative. The CodeGG mirror contributes
/// only presentation detail (which item to point at, which handle to name);
/// no acceptance satisfaction is reconstructed from it.
pub fn project_bound_completion(
    items: &[WorkItem],
    assessment: &eggplan_core::PlanAssessment,
) -> WorkPlanCompletionAssessment {
    let reasons: Vec<String> = assessment
        .reasons
        .iter()
        .map(|reason| bounded(&format!("{reason:?}"), MAX_REASON_CHARS))
        .collect();
    let suffix = if reasons.is_empty() {
        String::new()
    } else {
        format!(" [{}]", bounded(&reasons.join(","), 200))
    };
    match assessment.status {
        AssessmentStatus::Complete => WorkPlanCompletionAssessment::Complete {
            reason: bounded(
                &format!("repository plan assessment complete{suffix}"),
                MAX_REASON_CHARS,
            ),
        },
        AssessmentStatus::AwaitingHumanJudgment => {
            let mut details: Vec<String> = items
                .iter()
                .filter(|item| {
                    item.acceptance.iter().any(|criterion| {
                        criterion.disposition == WorkAcceptanceDisposition::RequiresUserJudgment
                    })
                })
                .take(4)
                .map(|item| {
                    bounded(
                        &format!(
                            "{}: awaiting user judgment",
                            bounded(item.description.trim(), MAX_TEXT_CHARS)
                        ),
                        MAX_TEXT_CHARS,
                    )
                })
                .collect();
            details.extend(reasons.into_iter().take(4));
            if details.is_empty() {
                details.push("remaining repository criteria require user judgment".to_string());
            }
            WorkPlanCompletionAssessment::AwaitingUserJudgment { reasons: details }
        }
        AssessmentStatus::Blocked
        | AssessmentStatus::EvidenceFailed
        | AssessmentStatus::EvidenceMissingOrUnavailable
        | AssessmentStatus::InvalidOrStale
        | AssessmentStatus::Inconclusive => {
            let blocked = items.iter().find(|item| {
                item.status == WorkItemStatus::Blocked
                    || item.dependencies.iter().any(|dep| {
                        items
                            .iter()
                            .any(|other| other.id == *dep && !other.status.is_terminal())
                    })
            });
            let reason = match assessment.status {
                AssessmentStatus::Blocked => blocked
                    .as_ref()
                    .and_then(|item| item.blocker.clone())
                    .unwrap_or_else(|| format!("repository plan is blocked{suffix}")),
                other => format!(
                    "repository assessment {}: {suffix}",
                    assessment_status_str(&other)
                ),
            };
            WorkPlanCompletionAssessment::Blocked {
                item_id: blocked.map(|item| item.id.clone()),
                blocker: bounded(&reason, MAX_REASON_CHARS),
            }
        }
        AssessmentStatus::InFlight => {
            let owned = items
                .iter()
                .find(|item| item.status == WorkItemStatus::InProgress);
            match owned {
                Some(item) => WorkPlanCompletionAssessment::InFlight {
                    item_id: Some(item.id.clone()),
                    handle_kind: "delegated_run".to_string(),
                    handle_id: bounded(
                        item.owner_run_id
                            .as_deref()
                            .or(item.owner_job_id.as_deref())
                            .unwrap_or("in_progress"),
                        MAX_TEXT_CHARS,
                    ),
                },
                None => WorkPlanCompletionAssessment::Blocked {
                    item_id: None,
                    blocker: bounded(
                        &format!("repository reports in-flight work without a mapped item{suffix}"),
                        MAX_REASON_CHARS,
                    ),
                },
            }
        }
        AssessmentStatus::ActionableWorkRemaining => {
            let current = items
                .iter()
                .find(|item| {
                    item.status == WorkItemStatus::InProgress
                        || item.status == WorkItemStatus::Actionable
                })
                .or_else(|| {
                    items.iter().find(|item| {
                        codegg_core::work_plan::actionable_items(items)
                            .iter()
                            .any(|candidate| candidate.id == item.id)
                    })
                });
            match current {
                Some(item) => {
                    let unmet = item
                        .acceptance
                        .iter()
                        .filter(|criterion| {
                            criterion.disposition == WorkAcceptanceDisposition::Unmet
                        })
                        .count();
                    WorkPlanCompletionAssessment::ActionableWorkRemaining {
                        current_item_id: item.id.clone(),
                        description: bounded(item.description.trim(), MAX_TEXT_CHARS),
                        unmet_count: unmet,
                        next_action: item
                            .next_action
                            .as_deref()
                            .map(|action| bounded(action.trim(), MAX_TEXT_CHARS))
                            .filter(|action| !action.is_empty()),
                    }
                }
                None => WorkPlanCompletionAssessment::Blocked {
                    item_id: None,
                    blocker: bounded(
                        &format!(
                            "repository requires continuation but no actionable item exists{suffix}"
                        ),
                        MAX_REASON_CHARS,
                    ),
                },
            }
        }
    }
}
