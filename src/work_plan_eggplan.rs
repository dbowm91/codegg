//! Application-layer Eggplan assessment facade (Eggplan M002).
//!
//! Staged production adoption of Eggplan's generic plan/evidence assessment
//! for the supported Git-backed CodeGG WorkPlan path. CodeGG keeps storage,
//! scheduler, Goal/Todo/checkpoint, worktree, and agent-loop ownership;
//! Eggplan only decides completion from the exact current subject plus
//! bound execution evidence projected through the compatibility bridge.
//!
//! Engine selection is explicit and deterministic:
//!
//! - `EggplanGit`: Git-backed workspace, current subject captured, all
//!   non-human evidence kinds supported. Missing/stale/drifted subjects
//!   and missing verification stay fail-closed inside Eggplan assessment;
//!   they never fall back to legacy.
//! - `LegacyNonGit`: positively non-Git workspace (no `.git` entry).
//!   A Git capture failure is never classified as non-Git.
//! - `LegacyUnsupportedEvidence`: the plan contains `Artifact`/`Commit`
//!   refs, which M002 cannot bind. The whole assessment uses legacy.
//!
//! Only active/blocked plans enter live Eggplan assessment; terminal
//! records are history and are rejected with `plan_not_active`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codegg_core::jobs::{
    ExecutionSubjectDisposition, ExecutionSubjectRevision, ExecutionTarget, JobPayload, JobRecord,
    JobStore, SqliteJobStore,
};
use codegg_core::work_plan::{
    actionable_items, HostEvidenceStatus, WorkEvidenceKind, WorkItem, WorkItemStatus, WorkPlan,
    WorkPlanCompletionAssessment, WorkPlanStatus, WorkPlanStore,
};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRegistry};
use eggplan_codegg_compat::{
    assess_codegg_snapshot, AcceptanceDisposition, CodeggAcceptance, CodeggCompletionFamily,
    CodeggEvidenceKind, CodeggEvidenceRef, CodeggItemSnapshot, CodeggItemStatus,
    CodeggPlanSnapshot, CodeggPlanStatus, EvidenceResolver, ResolvedEvidence,
};
use eggplan_core::{
    digest_json, EvidenceKind, EvidenceObservation, EvidenceObservationId,
    EvidenceObservationInput, EvidenceProviderId, EvidenceStatus, ProviderDescriptor,
    ProviderRegistry, SubjectRevision, SubjectState, VerificationDigest,
};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Exact immutable Eggplan revision consumed by M002.
pub const EGGPLAN_PIN: &str = "0d4a6af7adc6f80f975aca1bfe9bae04e2eb27d8";
/// Eggplan repository the pin belongs to.
pub const EGGPLAN_REPO: &str = "https://github.com/eggstack/eggplan.git";
/// Fixed CodeGG-host evidence provider identity. Trust is host
/// code, never serialized WorkPlan evidence text.
pub const EGGPLAN_PROVIDER_ID: &str = "epp_codegg_host";
const EGGPLAN_PROVIDER_CLASS: &str = "codegg-host-evidence-v1";
/// Canonical verification-spec schema version.
const VERIFICATION_SPEC_SCHEMA: &str = "codegg-verification-spec-v1";
const MAX_TEXT_CHARS: usize = 200;
const MAX_REASON_CHARS: usize = 500;
const MAX_ID_CHARS: usize = 128;

/// Explicit assessment-engine selection (M002 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssessmentEngine {
    EggplanGit,
    LegacyNonGit,
    LegacyUnsupportedEvidence,
}

impl AssessmentEngine {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EggplanGit => "eggplan_git",
            Self::LegacyNonGit => "legacy_non_git",
            Self::LegacyUnsupportedEvidence => "legacy_unsupported_evidence",
        }
    }
}

/// CodeGG-facing assessment: the existing public DTO plus bounded
/// Eggplan diagnostics. No Eggplan storage or repository handles escape.
#[derive(Debug, Clone)]
pub struct EggplanBackedAssessment {
    pub assessment: WorkPlanCompletionAssessment,
    pub engine: AssessmentEngine,
    pub eggplan_completion_family: Option<String>,
    pub eggplan_reason_codes: Vec<String>,
    pub mapping_digest: Option<String>,
    /// Bounded S1 revision the assessment ran against (revalidate before
    /// any completion transition).
    pub subject_revision: Option<String>,
    /// Full S1 subject for S2 equality revalidation (revision, dirty
    /// state, and dirty digest must all match).
    pub subject: Option<SubjectRevision>,
}

/// Bounded adapter failure. The plan is always preserved; callers must
/// not treat the plan as complete.
#[derive(Debug, Clone)]
pub struct AssessmentAdapterError {
    pub code: &'static str,
    pub detail: String,
}

impl AssessmentAdapterError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for AssessmentAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for AssessmentAdapterError {}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Execution-semantic fields covered by one verification digest:
/// kind, variant, argv, cwd, scope, mode, timeout, and extra digests.
type VerificationFields = (
    &'static str,
    String,
    Vec<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u64>,
    BTreeMap<String, String>,
);

fn bounded_id(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect()
}

/// Normalize a bare-hex dirty digest to Eggplan's `sha256:<hex>` form.
/// Already-prefixed values pass through; anything else is rejected.
fn eggplan_dirty_digest(hex: &str) -> Result<String, String> {
    if let Some(rest) = hex.strip_prefix("sha256:") {
        if rest.len() == 64
            && rest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Ok(hex.to_string());
        }
        return Err("dirty digest is not sha256 hex".to_string());
    }
    if hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Ok(format!("sha256:{hex}"));
    }
    Err("dirty digest is not sha256 hex".to_string())
}

/// Project M001 native subject fields to an Eggplan `SubjectRevision`,
/// normalizing the dirty-digest form.
fn eggplan_subject_from_fields(
    subject_kind: String,
    repository_id: String,
    revision: String,
    state: &str,
    dirty_digest: Option<String>,
) -> Result<SubjectRevision, String> {
    let (state, dirty) = match (state, dirty_digest) {
        ("dirty", Some(hex)) => (SubjectState::Dirty, Some(eggplan_dirty_digest(&hex)?)),
        ("dirty", None) => return Err("dirty subject without a dirty digest".to_string()),
        (_, _) => (SubjectState::Clean, None),
    };
    Ok(SubjectRevision {
        subject_kind,
        repository_id,
        revision,
        state,
        dirty_digest: dirty,
    })
}

// ── Engine selection helpers ─────────────────────────────────────────────

/// Positively non-Git workspaces use the legacy engine. A `.git` entry
/// (file or directory) means Git-backed for M002; bare-repo workspaces
/// without one stay legacy.
fn is_git_backed(workspace_root: &Path) -> bool {
    workspace_root.join(".git").exists()
}

fn has_unsupported_evidence(items: &[WorkItem]) -> Option<WorkEvidenceKind> {
    items
        .iter()
        .flat_map(|item| item.evidence.iter())
        .map(|evidence| evidence.kind)
        .find(|kind| matches!(kind, WorkEvidenceKind::Artifact | WorkEvidenceKind::Commit))
}

/// Resolve the session working directory from durable session state.
/// Returns `None` when the session is unknown (caller falls back to an
/// explicit legacy engine, never to a fabricated root).
pub async fn session_workspace_root(pool: &SqlitePool, session_id: &str) -> Option<PathBuf> {
    #[derive(sqlx::FromRow)]
    struct Row {
        directory: String,
    }
    let row: Option<Row> = sqlx::query_as("SELECT directory FROM session WHERE id = ?1")
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .ok()?;
    let directory = row?.directory;
    if directory.is_empty() {
        return None;
    }
    Some(PathBuf::from(directory))
}

// ── Canonical verification specification (M002 §7) ───────────────────────

/// Versioned CodeGG-native canonical execution specification. Only
/// execution-semantic fields participate: variant, canonical argv (or
/// content digests for large/sensitive inputs), normalized cwd, scope,
/// mode, effective timeout, and target class. Credentials, secrets,
/// timestamps, lease/attempt ids, progress output, labels, and node
/// addresses never enter.
#[derive(Debug, Clone, Serialize)]
struct CodeggVerificationSpecV1 {
    schema: &'static str,
    kind: &'static str,
    variant: String,
    argv: Vec<String>,
    cwd: Option<String>,
    scope: Option<String>,
    mode: Option<String>,
    timeout_secs: Option<u64>,
    target_class: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    extra: BTreeMap<String, String>,
}

fn bounded_argv(argv: &[String]) -> Vec<String> {
    argv.iter()
        .take(64)
        .map(|arg| bounded_text(arg, 512))
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn target_class(target: &ExecutionTarget) -> String {
    match target {
        ExecutionTarget::Local => "local".to_string(),
        ExecutionTarget::EggworkNode { .. } => "eggwork".to_string(),
    }
}

fn job_timeout_secs(job: &JobRecord) -> Option<u64> {
    job.timeout.map(|timeout| timeout.as_secs())
}

/// Derive the canonical verification digest for a natively owned job.
/// Returns `Err(reason)` (`verification_unavailable`) when the durable
/// record cannot reconstruct exact execution semantics; callers must
/// not hash ref/job ids as a substitute.
fn verification_digest_for_job(job: &JobRecord) -> Result<VerificationDigest, String> {
    let target_class = target_class(&job.target);
    let job_timeout = job_timeout_secs(job);
    // Each arm yields the execution-semantic fields; the digest covers
    // exactly this tuple and nothing else.
    let (kind, variant, argv, cwd, scope, mode, timeout_secs, extra): VerificationFields =
        match &job.payload {
            JobPayload::Test {
                argv, cwd, scope, ..
            } => {
                if argv.is_empty() {
                    return Err("test job has no canonical argv".to_string());
                }
                (
                    "test_job",
                    "test".to_string(),
                    bounded_argv(argv),
                    cwd.as_deref().map(|cwd| bounded_text(cwd, 512)),
                    scope.as_deref().map(|scope| bounded_text(scope, 256)),
                    None,
                    job_timeout,
                    BTreeMap::new(),
                )
            }
            JobPayload::ManagedArgv { argv, cwd } => {
                if argv.is_empty() {
                    return Err("scheduler job has no canonical argv".to_string());
                }
                (
                    "scheduler_job",
                    "managed_argv".to_string(),
                    bounded_argv(argv),
                    cwd.as_deref().map(|cwd| bounded_text(cwd, 512)),
                    None,
                    None,
                    job_timeout,
                    BTreeMap::new(),
                )
            }
            JobPayload::Shell {
                argv: Some(argv),
                cwd,
                ..
            } => {
                if argv.is_empty() {
                    return Err("shell job has no canonical argv".to_string());
                }
                (
                    "scheduler_job",
                    "shell".to_string(),
                    bounded_argv(argv),
                    cwd.as_deref().map(|cwd| bounded_text(cwd, 512)),
                    None,
                    None,
                    job_timeout,
                    BTreeMap::new(),
                )
            }
            JobPayload::Shell { argv: None, .. } => {
                return Err("shell job without canonical argv fails closed".to_string());
            }
            JobPayload::Python {
                script_path,
                args,
                mode,
                source,
                source_hash,
                cwd,
                timeout_secs: payload_timeout,
            } => {
                // Script/source digest participates; the path string alone is
                // execution identity only when no inline source exists.
                let script_digest = match (source, source_hash) {
                    (Some(_), Some(hash)) => hash.clone(),
                    (Some(_), None) => {
                        return Err("python inline source lacks its required digest".to_string());
                    }
                    (None, _) => sha256_hex(script_path.as_bytes()),
                };
                let mut extra = BTreeMap::new();
                extra.insert(
                    "script_digest".to_string(),
                    bounded_text(&script_digest, 128),
                );
                (
                    "scheduler_job",
                    "python".to_string(),
                    bounded_argv(args),
                    cwd.as_deref().map(|cwd| bounded_text(cwd, 512)),
                    None,
                    Some(bounded_text(mode, 64)),
                    payload_timeout.or(job_timeout),
                    extra,
                )
            }
            JobPayload::Git { argv, cwd } => {
                if argv.is_empty() {
                    return Err("git job has no canonical argv".to_string());
                }
                (
                    "scheduler_job",
                    "git".to_string(),
                    bounded_argv(argv),
                    cwd.as_deref().map(|cwd| bounded_text(cwd, 512)),
                    None,
                    None,
                    job_timeout,
                    BTreeMap::new(),
                )
            }
            JobPayload::SubagentRun {
                prompt,
                agent,
                model,
                parent_id,
                denied_tools,
                allowed_paths,
                max_tool_calls,
                task_id,
                run_id,
                delegation_key,
                base_commit,
                ..
            } => {
                // Prompt content enters only as a digest; policy enters as a
                // digest of the sorted sets; durable identities are provenance.
                let mut denied = denied_tools.clone();
                denied.sort();
                let mut allowed = allowed_paths.clone();
                allowed.sort();
                let mut extra = BTreeMap::new();
                extra.insert("prompt_digest".to_string(), sha256_hex(prompt.as_bytes()));
                extra.insert(
                    "policy_digest".to_string(),
                    sha256_hex(format!("{denied:?}|{allowed:?}").as_bytes()),
                );
                extra.insert("agent".to_string(), bounded_text(agent, MAX_ID_CHARS));
                if let Some(model) = model {
                    extra.insert("model".to_string(), bounded_text(model, MAX_ID_CHARS));
                }
                if let Some(parent) = parent_id {
                    extra.insert("parent_id".to_string(), bounded_text(parent, MAX_ID_CHARS));
                }
                if let Some(max_calls) = max_tool_calls {
                    extra.insert("max_tool_calls".to_string(), max_calls.to_string());
                }
                extra.insert(
                    "task_id".to_string(),
                    bounded_text(task_id.as_ref(), MAX_ID_CHARS),
                );
                extra.insert(
                    "run_id".to_string(),
                    bounded_text(run_id.as_ref(), MAX_ID_CHARS),
                );
                extra.insert(
                    "delegation_key".to_string(),
                    bounded_text(delegation_key, MAX_ID_CHARS),
                );
                if let Some(base) = base_commit {
                    extra.insert("base_commit".to_string(), bounded_text(base, MAX_ID_CHARS));
                }
                (
                    "delegated_run",
                    "subagent_run".to_string(),
                    Vec::new(),
                    None,
                    None,
                    None,
                    job_timeout,
                    extra,
                )
            }
            _ => {
                return Err("payload variant cannot prove exact execution semantics".to_string());
            }
        };
    let spec = CodeggVerificationSpecV1 {
        schema: VERIFICATION_SPEC_SCHEMA,
        kind,
        variant,
        argv,
        cwd,
        scope,
        mode,
        timeout_secs,
        target_class,
        extra,
    };
    let digest = digest_json(&spec).map_err(|error| format!("cannot digest spec: {error}"))?;
    VerificationDigest::new(digest).map_err(|error| format!("bad digest: {error:?}"))
}

// ── Current-subject authority (M002 §4) ───────────────────────────────────

/// Capture the exact current Git subject and project it to an Eggplan
/// `SubjectRevision`. The repository id reuses M001's
/// `codegg-workspace:<id>` scheme resolved read-only from the workspace
/// registry so historical subjects compare equal for the same workspace.
/// Never HEAD-only when dirty: the dirty digest participates.
async fn capture_current_subject(
    pool: &SqlitePool,
    workspace_root: &Path,
) -> Result<(SubjectRevision, String), AssessmentAdapterError> {
    let subject = egggit::capture_git_source_subject(workspace_root)
        .await
        .map_err(|error| match error {
            egggit::SubjectCaptureError::NotGit => AssessmentAdapterError::new(
                "current_subject_not_git",
                "workspace is not a Git subject",
            ),
            other => AssessmentAdapterError::new(
                "current_subject_capture_failed",
                format!("current subject capture failed: {other:?}"),
            ),
        })?;
    let store = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    let registry = WorkspaceRegistry::load(store).await.map_err(|error| {
        AssessmentAdapterError::new(
            "workspace_identity_unavailable",
            format!("cannot load workspace registry: {error}"),
        )
    })?;
    let record = registry.resolve_root(workspace_root).await.ok_or_else(|| {
        AssessmentAdapterError::new(
            "workspace_identity_unavailable",
            "workspace root is not a registered CodeGG workspace",
        )
    })?;
    let repository_id = format!("codegg-workspace:{}", record.id.as_str());
    let revision = ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: codegg_core::jobs::ExecutionSubjectKind::Git,
        repository_identity: repository_id.clone(),
        revision: subject.revision.clone(),
        state: if subject.dirty_digest.is_some() {
            codegg_core::jobs::ExecutionSubjectState::Dirty
        } else {
            codegg_core::jobs::ExecutionSubjectState::Clean
        },
        dirty_digest: subject.dirty_digest.clone(),
    };
    let fields = revision.to_eggplan_fields();
    let subject_revision = eggplan_subject_from_fields(
        fields.subject_kind,
        fields.repository_id,
        fields.revision.clone(),
        &fields.state,
        fields.dirty_digest,
    )
    .map_err(|reason| AssessmentAdapterError::new("current_subject_invalid", reason))?;
    Ok((subject_revision, revision.revision))
}

/// Re-capture S2 immediately before a completion transition and require
/// S2 == S1 (revision and dirty digest). Bounded revalidation, not a
/// global filesystem transaction.
pub async fn current_subject_matches(
    workspace_root: &Path,
    expected: &SubjectRevision,
) -> Result<bool, AssessmentAdapterError> {
    let subject = egggit::capture_git_source_subject(workspace_root)
        .await
        .map_err(|error| {
            AssessmentAdapterError::new(
                "completion_revalidation_failed",
                format!("S2 capture failed: {error:?}"),
            )
        })?;
    let state_matches = matches!(
        (&expected.state, &subject.dirty_digest),
        (SubjectState::Clean, None) | (SubjectState::Dirty, Some(_))
    );
    if expected.revision != subject.revision || !state_matches {
        return Ok(false);
    }
    if let (Some(expected_dirty), Some(actual_dirty)) =
        (&expected.dirty_digest, &subject.dirty_digest)
    {
        if expected_dirty != actual_dirty {
            return Ok(false);
        }
    }
    Ok(true)
}

// ── Evidence adapter (M002 §8) ───────────────────────────────────────────

/// Host evidence resolver: converts `assemble_resolved` records plus
/// native job/attempt objects into bounded Eggplan observations with
/// exact subject and verification binding. Native payload data never
/// selects the provider id or widens allowed kinds.
struct HostEvidenceResolver {
    resolved: BTreeMap<String, crate::work_plan_evidence::ResolvedWorkEvidence>,
    jobs: BTreeMap<String, JobRecord>,
    attempts: BTreeMap<String, codegg_core::jobs::JobAttempt>,
}

impl HostEvidenceResolver {
    async fn load(
        pool: &SqlitePool,
        resolved: Vec<crate::work_plan_evidence::ResolvedWorkEvidence>,
    ) -> Result<Self, AssessmentAdapterError> {
        let store = SqliteJobStore::new(pool.clone());
        let mut jobs = BTreeMap::new();
        let mut attempts = BTreeMap::new();
        for entry in &resolved {
            if let Some(job_id) = entry.native_job_id.as_deref() {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id.to_string());
                if let Ok(Some(job)) = store.get_job(&id).await {
                    if let Ok(list) = store.list_attempts(&job.job_id).await {
                        for attempt in list {
                            attempts.insert(
                                format!(
                                    "{}:{}",
                                    attempt.job_id.as_str(),
                                    attempt.attempt_id.as_str()
                                ),
                                attempt,
                            );
                        }
                    }
                    jobs.insert(job_id.to_string(), job);
                }
            }
        }
        Ok(Self {
            resolved: resolved
                .into_iter()
                .map(|entry| (format!("{}:{}", entry.kind.as_str(), entry.ref_id), entry))
                .collect(),
            jobs,
            attempts,
        })
    }

    fn provider_id() -> EvidenceProviderId {
        EvidenceProviderId::new(EGGPLAN_PROVIDER_ID).expect("fixed provider id is valid")
    }

    fn observation_id(
        kind: WorkEvidenceKind,
        ref_id: &str,
        attempt_seq: Option<u32>,
    ) -> EvidenceObservationId {
        // Typed ids require the `epe_` prefix and ≤96 chars.
        let base = format!(
            "epe_codegg-{}-{}-a{}",
            kind.as_str(),
            bounded_id(ref_id),
            attempt_seq.unwrap_or(0)
        );
        EvidenceObservationId::new(bounded_text(&base, 96)).expect("observation id is valid")
    }

    fn eggplan_kind(kind: WorkEvidenceKind) -> EvidenceKind {
        match kind {
            WorkEvidenceKind::TestJob => EvidenceKind::Test,
            WorkEvidenceKind::SchedulerJob => EvidenceKind::Command,
            WorkEvidenceKind::DelegatedRun | WorkEvidenceKind::AgentRun => {
                EvidenceKind::DelegatedRun
            }
            WorkEvidenceKind::Artifact => EvidenceKind::Artifact,
            WorkEvidenceKind::Commit => EvidenceKind::Revision,
        }
    }

    fn eggplan_status(status: HostEvidenceStatus) -> Option<EvidenceStatus> {
        match status {
            HostEvidenceStatus::Passed => Some(EvidenceStatus::Passed),
            HostEvidenceStatus::Failed => Some(EvidenceStatus::Failed),
            HostEvidenceStatus::InProgress => Some(EvidenceStatus::InProgress),
            HostEvidenceStatus::Unavailable => None,
        }
    }
}

impl EvidenceResolver for HostEvidenceResolver {
    fn resolve(
        &mut self,
        reference: &CodeggEvidenceRef,
        _subject: &SubjectRevision,
    ) -> Result<ResolvedEvidence, String> {
        let codegg_kind = match reference.kind {
            CodeggEvidenceKind::TestJob => WorkEvidenceKind::TestJob,
            CodeggEvidenceKind::SchedulerJob => WorkEvidenceKind::SchedulerJob,
            CodeggEvidenceKind::DelegatedRun => WorkEvidenceKind::DelegatedRun,
            CodeggEvidenceKind::AgentRun => WorkEvidenceKind::AgentRun,
            CodeggEvidenceKind::Artifact | CodeggEvidenceKind::Commit => {
                return Err("unsupported evidence kind has no observation".to_string());
            }
        };
        let key = format!("{}:{}", codegg_kind.as_str(), reference.ref_id);
        let entry = self
            .resolved
            .get(&key)
            .ok_or_else(|| format!("evidence ref {} is unresolved", reference.ref_id))?;
        // Terminal native objects only: the verification digest must come
        // from the authoritative record actually used at execution.
        let job = self
            .jobs
            .get(entry.native_job_id.as_deref().unwrap_or(""))
            .ok_or_else(|| format!("evidence ref {} has no native job", reference.ref_id))?;
        // Evidence-kind/payload mismatch fails closed (a TestJob ref
        // pointing at a scheduler payload proves nothing).
        let kind_matches = matches!(
            (codegg_kind, &job.payload),
            (WorkEvidenceKind::TestJob, JobPayload::Test { .. })
                | (
                    WorkEvidenceKind::SchedulerJob,
                    JobPayload::ManagedArgv { .. }
                        | JobPayload::Shell { .. }
                        | JobPayload::Python { .. }
                        | JobPayload::Git { .. },
                )
                | (
                    WorkEvidenceKind::DelegatedRun,
                    JobPayload::SubagentRun { .. }
                )
                | (WorkEvidenceKind::AgentRun, JobPayload::SubagentRun { .. })
        );
        if !kind_matches {
            return Err(format!(
                "evidence ref {} kind does not match its native payload",
                reference.ref_id
            ));
        }
        let status = Self::eggplan_status(entry.status)
            .ok_or_else(|| format!("evidence ref {} is unavailable", reference.ref_id))?;
        // Stable historical source provenance is required for terminal
        // passing/failed execution evidence. InProgress display needs a
        // Stable subject too; anything else is not fabricable.
        let provenance = entry.source_subject.as_ref().ok_or_else(|| {
            format!(
                "evidence ref {} has no historical subject",
                reference.ref_id
            )
        })?;
        if provenance.disposition != ExecutionSubjectDisposition::Stable {
            return Err(format!(
                "evidence ref {} subject is not stable",
                reference.ref_id
            ));
        }
        // Incomplete materialization is not exact-subject evidence even
        // when the surrounding disposition reads Stable.
        if let Some(materialization) = provenance.materialization.as_ref() {
            if !materialization.complete {
                return Err(format!(
                    "evidence ref {} materialization is incomplete",
                    reference.ref_id
                ));
            }
        }
        let historical = provenance
            .sealed
            .as_ref()
            .or(provenance.captured.as_ref())
            .ok_or_else(|| {
                format!(
                    "evidence ref {} has no historical revision",
                    reference.ref_id
                )
            })?;
        let fields = historical.to_eggplan_fields();
        // Dirty digests are bare hex in durable provenance; the Eggplan
        // subject form requires `sha256:<hex>`.
        let observation_subject = eggplan_subject_from_fields(
            fields.subject_kind,
            fields.repository_id,
            fields.revision,
            &fields.state,
            fields.dirty_digest,
        )
        .map_err(|reason| {
            format!(
                "evidence ref {} subject invalid: {reason}",
                reference.ref_id
            )
        })?;
        let digest = verification_digest_for_job(job).map_err(|reason| {
            format!(
                "verification_unavailable for {}: {reason}",
                reference.ref_id
            )
        })?;
        // Deterministic observation identity from stable native identity +
        // attempt sequence + semantic kind. Timestamps come from durable
        // native records (never "now", so repeated reads look identical):
        // terminal evidence uses the terminal timestamp; InProgress
        // display uses the durable start/creation timestamp.
        let attempt_key = entry.native_attempt_id.as_deref().map(|attempt| {
            format!(
                "{}:{}",
                entry.native_job_id.as_deref().unwrap_or(""),
                attempt
            )
        });
        let attempt = attempt_key
            .as_deref()
            .and_then(|key| self.attempts.get(key));
        let attempt_seq = attempt.map(|record| record.sequence);
        let observed_at_unix_ms = match status {
            EvidenceStatus::Passed | EvidenceStatus::Failed => attempt
                .and_then(|record| record.completed_at)
                .or(job.terminal_at)
                .map(|timestamp| timestamp.timestamp_millis().max(0) as u64)
                .ok_or_else(|| {
                    format!(
                        "evidence ref {} has no terminal timestamp",
                        reference.ref_id
                    )
                })?,
            EvidenceStatus::InProgress => attempt
                .and_then(|record| record.started_at)
                .or_else(|| attempt.map(|record| record.created_at))
                .unwrap_or(job.created_at)
                .timestamp_millis()
                .max(0) as u64,
            _ => {
                return Err(format!(
                    "evidence ref {} has an unprojectable status",
                    reference.ref_id
                ));
            }
        };
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "codegg.ref_id".to_string(),
            bounded_text(&reference.ref_id, MAX_ID_CHARS),
        );
        if let Some(attempt_id) = entry.native_attempt_id.as_deref() {
            metadata.insert(
                "codegg.native_attempt".to_string(),
                bounded_text(attempt_id, MAX_ID_CHARS),
            );
        }
        let observation = EvidenceObservation::finalize(EvidenceObservationInput {
            id: Self::observation_id(codegg_kind, &reference.ref_id, attempt_seq),
            provider_id: Self::provider_id(),
            kind: Self::eggplan_kind(codegg_kind),
            status,
            subject: observation_subject,
            observed_at_unix_ms,
            invocation_ref: Some(bounded_text(
                &format!(
                    "{}:{}",
                    entry.native_job_id.as_deref().unwrap_or(""),
                    entry.native_attempt_id.as_deref().unwrap_or("")
                ),
                MAX_ID_CHARS,
            )),
            verification_digest: Some(digest.clone()),
            result_metadata: metadata,
            artifacts: Vec::new(),
        })
        .map_err(|error| format!("invalid observation: {error:?}"))?;
        Ok(ResolvedEvidence {
            observation,
            expected_verification: Some(digest),
        })
    }
}

fn provider_registry() -> Result<ProviderRegistry, AssessmentAdapterError> {
    let mut registry = ProviderRegistry::default();
    registry
        .register_trusted(
            ProviderDescriptor::new(
                EvidenceProviderId::new(EGGPLAN_PROVIDER_ID).expect("fixed provider id is valid"),
                EGGPLAN_PROVIDER_CLASS,
                [
                    EvidenceKind::Test,
                    EvidenceKind::Command,
                    EvidenceKind::DelegatedRun,
                ],
            )
            .map_err(|error| {
                AssessmentAdapterError::new(
                    "provider_registry_invalid",
                    format!("fixed provider descriptor invalid: {error:?}"),
                )
            })?,
        )
        .map_err(|error| {
            AssessmentAdapterError::new(
                "provider_registry_invalid",
                format!("cannot register fixed provider: {error:?}"),
            )
        })?;
    Ok(registry)
}

// ── Plan snapshot mapping (M002 §9) ─────────────────────────────────────────

fn map_plan_status(status: WorkPlanStatus) -> CodeggPlanStatus {
    match status {
        WorkPlanStatus::Active => CodeggPlanStatus::Active,
        WorkPlanStatus::Blocked => CodeggPlanStatus::Blocked,
        WorkPlanStatus::Completed => CodeggPlanStatus::Completed,
        WorkPlanStatus::Cancelled => CodeggPlanStatus::Cancelled,
    }
}

fn map_item_status(status: WorkItemStatus) -> CodeggItemStatus {
    match status {
        WorkItemStatus::Pending => CodeggItemStatus::Pending,
        WorkItemStatus::Actionable => CodeggItemStatus::Actionable,
        WorkItemStatus::InProgress => CodeggItemStatus::InProgress,
        WorkItemStatus::Blocked => CodeggItemStatus::Blocked,
        WorkItemStatus::Completed => CodeggItemStatus::Completed,
        WorkItemStatus::Cancelled => CodeggItemStatus::Cancelled,
    }
}

fn map_evidence_kind(kind: WorkEvidenceKind) -> CodeggEvidenceKind {
    match kind {
        WorkEvidenceKind::TestJob => CodeggEvidenceKind::TestJob,
        WorkEvidenceKind::DelegatedRun => CodeggEvidenceKind::DelegatedRun,
        WorkEvidenceKind::SchedulerJob => CodeggEvidenceKind::SchedulerJob,
        WorkEvidenceKind::AgentRun => CodeggEvidenceKind::AgentRun,
        WorkEvidenceKind::Artifact => CodeggEvidenceKind::Artifact,
        WorkEvidenceKind::Commit => CodeggEvidenceKind::Commit,
    }
}

/// Build the `CodeggPlanSnapshot` directly from current CodeGG values.
/// Source ids are preserved as source identity; owner run/job ids stay
/// provenance; serialized `Satisfied` is mapped faithfully (the bridge
/// records it as a loss, never as host evidence).
fn snapshot_for_assessment(plan: &WorkPlan, items: &[WorkItem]) -> CodeggPlanSnapshot {
    CodeggPlanSnapshot {
        plan_id: bounded_text(plan.id.as_str(), MAX_ID_CHARS),
        revision: plan.revision,
        status: map_plan_status(plan.status),
        objective: bounded_text(&plan.objective, 4000),
        current_phase: plan.current_phase.as_deref().map(|phase| bounded_text(phase, 512)),
        current_item_id: plan
            .current_item_id
            .as_ref()
            .map(|id| bounded_text(id.as_str(), MAX_ID_CHARS)),
        items: items
            .iter()
            .map(|item| CodeggItemSnapshot {
                item_id: bounded_text(item.id.as_str(), MAX_ID_CHARS),
                position: item.position,
                parent_item_id: item
                    .parent_item_id
                    .as_ref()
                    .map(|id| bounded_text(id.as_str(), MAX_ID_CHARS)),
                dependencies: item
                    .dependencies
                    .iter()
                    .take(32)
                    .map(|dep| bounded_text(dep.as_str(), MAX_ID_CHARS))
                    .collect(),
                status: map_item_status(item.status),
                description: bounded_text(&item.description, 4000),
                acceptance: item
                    .acceptance
                    .iter()
                    .take(16)
                    .map(|criterion| CodeggAcceptance {
                        description: bounded_text(&criterion.description, 2000),
                        disposition: match criterion.disposition {
                            codegg_core::work_plan::WorkAcceptanceDisposition::Unmet => {
                                AcceptanceDisposition::Unmet
                            }
                            codegg_core::work_plan::WorkAcceptanceDisposition::Satisfied => {
                                AcceptanceDisposition::Satisfied
                            }
                            codegg_core::work_plan::WorkAcceptanceDisposition::RequiresUserJudgment => {
                                AcceptanceDisposition::RequiresUserJudgment
                            }
                        },
                        note: criterion.note.as_deref().map(|note| bounded_text(note, 512)),
                    })
                    .collect(),
                evidence: item
                    .evidence
                    .iter()
                    .take(32)
                    .map(|evidence| CodeggEvidenceRef {
                        kind: map_evidence_kind(evidence.kind),
                        ref_id: bounded_text(evidence.ref_id.as_str(), MAX_ID_CHARS),
                        detail: evidence.detail.as_deref().map(|detail| bounded_text(detail, 512)),
                    })
                    .collect(),
                owner_run_id: item
                    .owner_run_id
                    .as_deref()
                    .map(|id| bounded_text(id, MAX_ID_CHARS)),
                owner_job_id: item
                    .owner_job_id
                    .as_deref()
                    .map(|id| bounded_text(id, MAX_ID_CHARS)),
                blocker: item.blocker.as_deref().map(|blocker| bounded_text(blocker, 500)),
                next_action: item
                    .next_action
                    .as_deref()
                    .map(|action| bounded_text(action, 500)),
            })
            .collect(),
    }
}

// ── Family projection to the CodeGG DTO (M002 §10) ──────────────────────────

/// Project the Eggplan completion family to the existing CodeGG DTO,
/// populating CodeGG-specific detail from authoritative source state.
/// Free-form Eggplan diagnostic text never becomes scheduler authority.
fn project_to_dto(
    items: &[WorkItem],
    resolved: &[crate::work_plan_evidence::ResolvedWorkEvidence],
    family: CodeggCompletionFamily,
    reason_codes: &[String],
) -> WorkPlanCompletionAssessment {
    let reason_suffix = if reason_codes.is_empty() {
        String::new()
    } else {
        format!(" [{}]", bounded_text(&reason_codes.join(","), 200))
    };
    match family {
        CodeggCompletionFamily::Complete => WorkPlanCompletionAssessment::Complete {
            reason: bounded_text(
                &format!("eggplan assessment complete{reason_suffix}"),
                MAX_REASON_CHARS,
            ),
        },
        CodeggCompletionFamily::ActionableWorkRemaining => {
            let current = actionable_items(items).into_iter().next();
            match current {
                Some(item) => {
                    let unmet = item
                        .acceptance
                        .iter()
                        .filter(|criterion| {
                            criterion.disposition
                                == codegg_core::work_plan::WorkAcceptanceDisposition::Unmet
                        })
                        .count();
                    WorkPlanCompletionAssessment::ActionableWorkRemaining {
                        current_item_id: item.id.clone(),
                        description: bounded_text(item.description.trim(), MAX_TEXT_CHARS),
                        unmet_count: unmet,
                        next_action: item
                            .next_action
                            .as_deref()
                            .map(|action| bounded_text(action.trim(), MAX_TEXT_CHARS))
                            .filter(|action| !action.is_empty()),
                    }
                }
                None => WorkPlanCompletionAssessment::Blocked {
                    item_id: None,
                    blocker: bounded_text(
                        &format!("eggplan requires continuation but no actionable item exists{reason_suffix}"),
                        MAX_REASON_CHARS,
                    ),
                },
            }
        }
        CodeggCompletionFamily::Blocked => {
            let blocked = items.iter().find(|item| {
                item.status == WorkItemStatus::Blocked
                    || item.dependencies.iter().any(|dep| {
                        items
                            .iter()
                            .any(|other| other.id == *dep && !other.status.is_terminal())
                    })
            });
            match blocked {
                Some(item) => WorkPlanCompletionAssessment::Blocked {
                    item_id: Some(item.id.clone()),
                    blocker: bounded_text(
                        item.blocker.as_deref().unwrap_or("item is blocked"),
                        MAX_REASON_CHARS,
                    ),
                },
                None => WorkPlanCompletionAssessment::Blocked {
                    item_id: None,
                    blocker: bounded_text(
                        &format!("eggplan assessment blocked{reason_suffix}"),
                        MAX_REASON_CHARS,
                    ),
                },
            }
        }
        CodeggCompletionFamily::AwaitingUserJudgment => {
            let mut reasons: Vec<String> = items
                .iter()
                .filter(|item| {
                    item.acceptance.iter().any(|criterion| {
                        criterion.disposition
                            == codegg_core::work_plan::WorkAcceptanceDisposition::RequiresUserJudgment
                    })
                })
                .take(4)
                .map(|item| {
                    bounded_text(
                        &format!("{}: awaiting user judgment", bounded_text(item.description.trim(), MAX_TEXT_CHARS)),
                        MAX_TEXT_CHARS,
                    )
                })
                .collect();
            reasons.extend(reason_codes.iter().take(4).cloned());
            if reasons.is_empty() {
                reasons.push("remaining criteria require user judgment".to_string());
            }
            WorkPlanCompletionAssessment::AwaitingUserJudgment { reasons }
        }
        CodeggCompletionFamily::InFlight => {
            let live = resolved
                .iter()
                .find(|entry| entry.status == HostEvidenceStatus::InProgress);
            match live {
                Some(entry) => WorkPlanCompletionAssessment::InFlight {
                    item_id: items
                        .iter()
                        .find(|item| {
                            item.evidence.iter().any(|evidence| {
                                evidence.kind == entry.kind && evidence.ref_id == entry.ref_id
                            })
                        })
                        .map(|item| item.id.clone()),
                    handle_kind: bounded_text(entry.kind.as_str(), 64),
                    handle_id: bounded_text(&entry.ref_id, MAX_TEXT_CHARS),
                },
                None => {
                    let owned = items
                        .iter()
                        .find(|item| item.owner_run_id.is_some() || item.owner_job_id.is_some());
                    match owned {
                        Some(item) => WorkPlanCompletionAssessment::InFlight {
                            item_id: Some(item.id.clone()),
                            handle_kind: "delegated_run".to_string(),
                            handle_id: bounded_text(
                                item.owner_run_id
                                    .as_deref()
                                    .or(item.owner_job_id.as_deref())
                                    .unwrap_or("unknown"),
                                MAX_TEXT_CHARS,
                            ),
                        },
                        None => WorkPlanCompletionAssessment::Blocked {
                            item_id: None,
                            blocker: bounded_text(
                                &format!("eggplan reports in-flight work without a resolvable handle{reason_suffix}"),
                                MAX_REASON_CHARS,
                            ),
                        },
                    }
                }
            }
        }
    }
}

// ── Facade (M002 §10) ──────────────────────────────────────────────────────

/// Assess one active/blocked plan through the Eggplan engine. The caller
/// supplies the canonical workspace root; current-subject capture,
/// verification binding, and provider trust are owned here.
pub async fn assess_work_plan_with_eggplan(
    pool: &SqlitePool,
    workspace_root: &Path,
    plan: &WorkPlan,
    items: &[WorkItem],
) -> Result<EggplanBackedAssessment, AssessmentAdapterError> {
    if matches!(
        plan.status,
        WorkPlanStatus::Completed | WorkPlanStatus::Cancelled
    ) {
        return Err(AssessmentAdapterError::new(
            "plan_not_active",
            "terminal plans are lifecycle history and are not re-assessed",
        ));
    }
    if !matches!(
        plan.status,
        WorkPlanStatus::Active | WorkPlanStatus::Blocked
    ) {
        return Err(AssessmentAdapterError::new(
            "plan_not_active",
            "only active/blocked plans enter live Eggplan assessment",
        ));
    }
    let (subject, subject_revision) = capture_current_subject(pool, workspace_root).await?;
    let snapshot = snapshot_for_assessment(plan, items);
    let resolved_snapshot = crate::work_plan_evidence::assemble_resolved(pool, items)
        .await
        .map_err(|error| AssessmentAdapterError::new("evidence_resolution_failed", error))?;
    let resolved_entries = resolved_snapshot.entries.clone();
    let mut resolver = HostEvidenceResolver::load(pool, resolved_snapshot.entries).await?;
    let providers = provider_registry()?;
    let bridged = assess_codegg_snapshot(&snapshot, subject.clone(), &mut resolver, &providers)
        .map_err(|error| AssessmentAdapterError::new("eggplan_assessment_failed", error))?;
    let family = bridged.completion_family.as_str().to_string();
    let assessment = project_to_dto(
        items,
        &resolved_entries,
        bridged.completion_family,
        &bridged.reason_codes,
    );
    Ok(EggplanBackedAssessment {
        assessment,
        engine: AssessmentEngine::EggplanGit,
        eggplan_completion_family: Some(family),
        eggplan_reason_codes: bridged.reason_codes,
        mapping_digest: Some(bridged.manifest.content_digest.clone()),
        subject_revision: Some(subject_revision),
        subject: Some(subject),
    })
}

/// Engine-selecting assessment for production call sites. Git-backed
/// supported-evidence paths use the Eggplan facade; non-Git and
/// unsupported-evidence paths use the explicit legacy compatibility
/// engine. The legacy core assessor is never called directly by
/// migrated sites.
pub async fn assess_with_engine(
    pool: &SqlitePool,
    workspace_root: Option<&Path>,
    plan: &WorkPlan,
    items: &[WorkItem],
) -> Result<EggplanBackedAssessment, AssessmentAdapterError> {
    let Some(root) = workspace_root else {
        return Ok(legacy_backed(pool, plan, items, AssessmentEngine::LegacyNonGit).await);
    };
    if !is_git_backed(root) {
        return Ok(legacy_backed(pool, plan, items, AssessmentEngine::LegacyNonGit).await);
    }
    if has_unsupported_evidence(items).is_some() {
        return Ok(legacy_backed(
            pool,
            plan,
            items,
            AssessmentEngine::LegacyUnsupportedEvidence,
        )
        .await);
    }
    match assess_work_plan_with_eggplan(pool, root, plan, items).await {
        Ok(backed) => Ok(backed),
        Err(error) if error.code == "plan_not_active" => {
            Ok(legacy_backed(pool, plan, items, AssessmentEngine::LegacyNonGit).await)
        }
        Err(error) => Err(error),
    }
}

async fn legacy_backed(
    pool: &SqlitePool,
    plan: &WorkPlan,
    items: &[WorkItem],
    engine: AssessmentEngine,
) -> EggplanBackedAssessment {
    // The legacy compatibility engine assesses over the real assembled
    // host-evidence snapshot, exactly as the pre-M002 arbiter did; only
    // the engine label is new.
    let evidence = crate::work_plan_evidence::assemble(pool, items)
        .await
        .unwrap_or_else(|_| empty_snapshot());
    EggplanBackedAssessment {
        assessment: codegg_core::work_plan::assess_work_plan(plan, items, &evidence),
        engine,
        eggplan_completion_family: None,
        eggplan_reason_codes: Vec::new(),
        mapping_digest: None,
        subject_revision: None,
        subject: None,
    }
}

fn empty_snapshot() -> codegg_core::work_plan::WorkPlanEvidenceSnapshot {
    codegg_core::work_plan::WorkPlanEvidenceSnapshot::empty()
}

/// Complete a plan only when the backed assessment allows it AND the
/// current subject still equals S1. Re-captures S2 immediately before
/// the status CAS; on drift (revision, dirty state, or dirty digest)
/// the plan is preserved and completion is refused with
/// `subject_changed_before_completion`.
pub async fn complete_plan_with_subject_revalidation(
    pool: &SqlitePool,
    workspace_root: &Path,
    plan: &WorkPlan,
    backed: &EggplanBackedAssessment,
) -> Result<bool, String> {
    if !backed.assessment.allows_completion() {
        return Ok(false);
    }
    if backed.engine != AssessmentEngine::EggplanGit {
        return Err("subject revalidation requires the EggplanGit engine".to_string());
    }
    let expected = backed.subject.as_ref().ok_or_else(|| {
        "subject revalidation requires the S1 subject from assessment".to_string()
    })?;
    let (current, _) = capture_current_subject(pool, workspace_root)
        .await
        .map_err(|error| format!("S2 capture failed: {error}"))?;
    if current != *expected {
        return Err("subject_changed_before_completion".to_string());
    }
    let store = WorkPlanStore::new(pool.clone());
    match store
        .transition_plan(&plan.id, plan.revision, WorkPlanStatus::Completed)
        .await
    {
        Ok(_) => Ok(true),
        Err(codegg_core::work_plan::WorkPlanError::Conflict { .. }) => Ok(false),
        Err(codegg_core::work_plan::WorkPlanError::Terminal(_)) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use codegg_core::jobs::{
        IdempotencyClass, JobKind, JobPriority, JobSource, JobState, ResourceRequest, RetryPolicy,
    };
    use codegg_core::work_plan::{
        WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceRef, WorkItemId, WorkPlanId,
    };
    use codegg_core::workspace::WorkspaceId;

    /// Canonical JSON of the verification spec (test-only introspection).
    #[cfg(test)]
    fn spec_json_for_job(job: &JobRecord) -> String {
        // Rebuild the spec through the same path the digest covers.
        let target_class = target_class(&job.target);
        let job_timeout = job_timeout_secs(job);
        let spec = match &job.payload {
            JobPayload::Test {
                argv, cwd, scope, ..
            } => CodeggVerificationSpecV1 {
                schema: VERIFICATION_SPEC_SCHEMA,
                kind: "test_job",
                variant: "test".to_string(),
                argv: bounded_argv(argv),
                cwd: cwd.as_deref().map(|cwd| bounded_text(cwd, 512)),
                scope: scope.as_deref().map(|scope| bounded_text(scope, 256)),
                mode: None,
                timeout_secs: job_timeout,
                target_class,
                extra: BTreeMap::new(),
            },
            _ => panic!("test helper covers Test payloads only"),
        };
        serde_json::to_string(&spec).unwrap()
    }

    fn test_job_record() -> JobRecord {
        let now = Utc::now();
        JobRecord {
            job_id: codegg_core::jobs::JobId::new_unchecked("j-verify-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-verify"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::Test {
                command: "cargo test".to_string(),
                argv: vec!["cargo".to_string(), "test".to_string(), "--lib".to_string()],
                cwd: None,
                scope: None,
                parent_run_id: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            state: JobState::Completed,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: ExecutionTarget::Local,
            labels: Default::default(),
        }
    }

    #[test]
    fn eggplan_pin_matches_manifest_and_lock() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains(&format!("rev = \"{EGGPLAN_PIN}\"")),
            "root Cargo.toml must pin the exact reviewed Eggplan revision"
        );
        // Only the pure assessment crates may appear as production
        // dependencies; repo/cli/projection/markdown/integrations are out.
        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("eggplan-") {
                assert!(
                    trimmed.starts_with("eggplan-core")
                        || trimmed.starts_with("eggplan-codegg-compat"),
                    "unexpected Eggplan dependency: {trimmed}"
                );
            }
        }
        let lock = std::fs::read_to_string(manifest_dir.join("Cargo.lock")).unwrap();
        let mut saw_core = false;
        let mut saw_compat = false;
        for package in lock.split("[[package]]") {
            if package.contains("name = \"eggplan-") {
                if package.contains("name = \"eggplan-core\"") {
                    saw_core = true;
                } else if package.contains("name = \"eggplan-codegg-compat\"") {
                    saw_compat = true;
                } else {
                    // eggplan-repo enters the lock only as a dev-dependency
                    // of the compat crate's own test suite; it must never
                    // be a production dependency of codegg (covered by the
                    // manifest assertion above).
                    assert!(
                        package.contains("name = \"eggplan-repo\""),
                        "unexpected Eggplan package in lock: {}",
                        package.lines().take(3).collect::<Vec<_>>().join(" ")
                    );
                }
                assert!(
                    package.contains(EGGPLAN_PIN),
                    "Eggplan lock entry must resolve to the pinned revision"
                );
            }
        }
        assert!(saw_core && saw_compat, "both pure crates must be locked");
    }

    #[test]
    fn provider_registry_is_fixed_and_bounded() {
        let registry = provider_registry().unwrap();
        let descriptor = registry
            .get(&EvidenceProviderId::new(EGGPLAN_PROVIDER_ID).unwrap())
            .expect("fixed provider registered");
        let kinds: Vec<EvidenceKind> = descriptor.allowed_kinds().iter().copied().collect();
        assert_eq!(
            kinds,
            vec![
                EvidenceKind::Command,
                EvidenceKind::Test,
                EvidenceKind::DelegatedRun
            ]
        );
    }

    #[test]
    fn verification_digest_is_deterministic_and_bound() {
        let job = test_job_record();
        let first = verification_digest_for_job(&job).unwrap();
        let second = verification_digest_for_job(&job).unwrap();
        assert_eq!(first, second);
        assert!(first.as_str().starts_with("sha256:"));
    }

    #[test]
    fn verification_argv_semantics_change_digest() {
        let base = verification_digest_for_job(&test_job_record()).unwrap();
        let mut changed = test_job_record();
        if let JobPayload::Test { argv, .. } = &mut changed.payload {
            argv.push("--release".to_string());
        }
        assert_ne!(verification_digest_for_job(&changed).unwrap(), base);
        // Display-only command text is not authoritative.
        let mut display = test_job_record();
        if let JobPayload::Test { command, .. } = &mut display.payload {
            *command = "a friendlier label".to_string();
        }
        assert_eq!(verification_digest_for_job(&display).unwrap(), base);
    }

    #[test]
    fn verification_target_and_timeout_participate() {
        let base = verification_digest_for_job(&test_job_record()).unwrap();
        let mut remote = test_job_record();
        remote.target = ExecutionTarget::EggworkNode {
            node_id: "n".to_string(),
        };
        assert_ne!(verification_digest_for_job(&remote).unwrap(), base);
        let mut timed = test_job_record();
        timed.timeout = Some(std::time::Duration::from_secs(600));
        assert_ne!(verification_digest_for_job(&timed).unwrap(), base);
    }

    #[test]
    fn verification_prompt_content_never_in_spec() {
        let job = test_job_record();
        let json = spec_json_for_job(&job);
        assert!(json.contains("cargo"));
        // Timestamps/attempt/lease/node data must not enter the spec.
        assert!(!json.contains("terminal_at"));
        assert!(!json.contains("attempt"));
        assert!(!json.contains("lease"));
    }

    #[test]
    fn verification_unavailable_cases_fail_closed() {
        // Shell without canonical argv.
        let mut shell = test_job_record();
        shell.payload = JobPayload::Shell {
            command: "echo hi".to_string(),
            argv: None,
            cwd: None,
        };
        assert!(verification_digest_for_job(&shell).is_err());
        // Python inline source without its required digest.
        let mut python = test_job_record();
        python.payload = JobPayload::Python {
            script_path: "run.py".to_string(),
            args: Vec::new(),
            mode: "file".to_string(),
            source: Some("print(1)".to_string()),
            source_hash: None,
            cwd: None,
            timeout_secs: None,
        };
        assert!(verification_digest_for_job(&python).is_err());
        // Legacy Subagent payloads stay verification-unavailable.
        let mut legacy = test_job_record();
        legacy.payload = JobPayload::Subagent {
            prompt: "p".to_string(),
            agent: "a".to_string(),
            model: None,
            parent_id: None,
            denied_tools: Vec::new(),
            allowed_paths: Vec::new(),
            max_tool_calls: None,
        };
        assert!(verification_digest_for_job(&legacy).is_err());
        // Research payloads prove no execution semantics.
        let mut research = test_job_record();
        research.payload = JobPayload::Research {
            query: "q".to_string(),
            max_depth: None,
        };
        assert!(verification_digest_for_job(&research).is_err());
    }

    #[test]
    fn subagent_run_policy_digest_stable_under_reorder() {
        let payload = |denied: Vec<String>, allowed: Vec<String>| JobPayload::SubagentRun {
            prompt: "do the thing".to_string(),
            agent: "worker".to_string(),
            model: Some("m".to_string()),
            parent_id: None,
            denied_tools: denied,
            allowed_paths: allowed,
            max_tool_calls: Some(8),
            task_id: codegg_core::identity::AgentTaskId::parse("task-1").unwrap(),
            run_id: codegg_core::identity::AgentRunId::parse("run-1").unwrap(),
            delegation_key: "dk".to_string(),
            base_commit: None,
        };
        let mut first = test_job_record();
        first.payload = payload(
            vec!["b".to_string(), "a".to_string()],
            vec!["x".to_string()],
        );
        let mut second = test_job_record();
        second.payload = payload(
            vec!["a".to_string(), "b".to_string()],
            vec!["x".to_string()],
        );
        assert_eq!(
            verification_digest_for_job(&first).unwrap(),
            verification_digest_for_job(&second).unwrap()
        );
    }

    #[test]
    fn dirty_digest_normalization() {
        assert_eq!(
            eggplan_dirty_digest(&"a".repeat(64)).unwrap(),
            format!("sha256:{}", "a".repeat(64))
        );
        assert_eq!(
            eggplan_dirty_digest(&format!("sha256:{}", "b".repeat(64))).unwrap(),
            format!("sha256:{}", "b".repeat(64))
        );
        assert!(eggplan_dirty_digest("abc").is_err());
        assert!(eggplan_dirty_digest(&format!("sha256:{}", "G".repeat(64))).is_err());
    }

    fn plan_fixture(status: WorkPlanStatus) -> WorkPlan {
        let now = Utc::now();
        WorkPlan {
            id: WorkPlanId("wp-egg".to_string()),
            revision: 0,
            session_id: "sess-egg".to_string(),
            project_id: "proj-egg".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "objective".to_string(),
            objective_digest: "sha256:x".to_string(),
            origin_provenance: "turn:t".to_string(),
            status,
            current_phase: None,
            current_item_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    fn item_fixture(status: WorkItemStatus) -> WorkItem {
        let now = Utc::now();
        WorkItem {
            id: WorkItemId("wi-egg-1".to_string()),
            plan_id: WorkPlanId("wp-egg".to_string()),
            revision: 0,
            position: 0,
            parent_item_id: None,
            dependencies: Vec::new(),
            status,
            description: "work".to_string(),
            acceptance: vec![WorkAcceptance {
                description: "criterion".to_string(),
                disposition: WorkAcceptanceDisposition::Unmet,
                note: None,
            }],
            evidence: Vec::new(),
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: None,
            next_action: Some("next".to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    fn lazy_pool() -> SqlitePool {
        SqlitePool::connect_lazy("sqlite::memory:").expect("lazy pool")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn engine_selection_without_root_is_legacy_non_git() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let backed = assess_with_engine(
            &lazy_pool(),
            None,
            &plan,
            &[item_fixture(WorkItemStatus::Pending)],
        )
        .await
        .unwrap();
        assert_eq!(backed.engine, AssessmentEngine::LegacyNonGit);
        assert!(backed.eggplan_completion_family.is_none());
        assert!(backed.assessment.requires_continuation());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn engine_selection_without_git_is_legacy_non_git() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan_fixture(WorkPlanStatus::Active);
        let backed = assess_with_engine(
            &lazy_pool(),
            Some(dir.path()),
            &plan,
            &[item_fixture(WorkItemStatus::Pending)],
        )
        .await
        .unwrap();
        assert_eq!(backed.engine, AssessmentEngine::LegacyNonGit);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn engine_selection_with_unsupported_evidence_is_explicit_legacy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut item = item_fixture(WorkItemStatus::Pending);
        item.evidence = vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::Artifact,
            ref_id: "artifact-1".to_string(),
            detail: None,
        }];
        let backed = assess_with_engine(&lazy_pool(), Some(dir.path()), &plan, &[item])
            .await
            .unwrap();
        assert_eq!(backed.engine, AssessmentEngine::LegacyUnsupportedEvidence);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn terminal_plans_are_not_reassessed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let plan = plan_fixture(WorkPlanStatus::Completed);
        let backed = assess_with_engine(&lazy_pool(), Some(dir.path()), &plan, &[])
            .await
            .unwrap();
        assert_eq!(backed.engine, AssessmentEngine::LegacyNonGit);
        assert!(backed.assessment.allows_completion());
    }

    /// Static source guard (M002 WP1/WP6): production call sites reach
    /// the legacy core assessor only through the engine-selection
    /// facade or the explicit legacy arbiter wrappers. Direct
    /// `assess_work_plan(` calls anywhere else fail this test.
    #[test]
    fn production_assessment_goes_through_engine_selection() {
        const ALLOWLIST: &[&str] = &[
            // Explicit legacy arbiter wrappers (compatibility API for
            // pure/test callers and the Legacy engines selected above).
            "src/work_plan_arbiter.rs",
            // The facade itself (legacy engines) plus this guard.
            "src/work_plan_eggplan.rs",
        ];
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations = Vec::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let relative = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if ALLOWLIST.contains(&relative.as_str()) {
                    continue;
                }
                let content = std::fs::read_to_string(&path).unwrap();
                for (index, line) in content.lines().enumerate() {
                    if line.contains("assess_work_plan(")
                        && !line.contains("assess_work_plan_with_eggplan(")
                    {
                        violations.push(format!("{}:{}: {line}", relative, index + 1));
                    }
                }
            }
        }
        assert!(
            violations.is_empty(),
            "direct legacy assessor calls outside the facade/legacy wrappers:\n{}",
            violations.join("\n")
        );
    }
}
