//! Eggplan-backed WorkPlan assessment adoption (staged M002 adoption).
//!
//! This application-layer module is the only production boundary that
//! consumes the pure Eggplan assessment crates (`eggplan-core` +
//! `eggplan-codegg-compat`, pinned immutable in the root `Cargo.toml`).
//! `codegg-core` stays Eggplan-free: its pure
//! [`assess_work_plan`](codegg_core::work_plan::assess_work_plan) remains
//! the compatibility API, the differential oracle, and the explicit legacy
//! engine for non-Git or currently unsupported evidence.
//!
//! Ownership (see `plans/.../003-staged-production-assessment-adoption.md`):
//!
//! - CodeGG owns WorkPlan/Job/AgentRun persistence, scheduler authority,
//!   Goal/Todo/checkpoint state, worktree leases, and capture of execution
//!   provenance. Nothing here moves storage, scheduler, Git, or worktree
//!   ownership into Eggplan, and no Eggplan repository state is created.
//! - Historical evidence subjects come only from durable M001 attempt
//!   provenance resolved through
//!   [`assemble_resolved`](crate::work_plan_evidence::assemble_resolved).
//!   The current worktree is never consulted to fill historical records.
//! - The current assessment subject is governed capture through
//!   [`crate::scheduler::assessment_subject`], rooted at the canonical
//!   workspace root resolved from the session/workspace catalog (never
//!   process CWD, never HEAD-only when dirty).
//! - Completion transitions revalidate the subject immediately before the
//!   plan-status CAS via [`revalidate_subject_for_completion`]: the
//!   assessment allows completion only when the re-captured subject still
//!   equals the assessed one.
//!
//! Engine selection is explicit and deterministic from structured host
//! state ([`AssessmentEngine`]). Missing, stale, drifted, or unbound
//! historical evidence stays fail-closed inside Eggplan assessment; it
//! never falls back to legacy satisfaction. Legacy engines return the
//! legacy result verbatim (zero delta by construction) with a diagnostic
//! naming the compatibility reason.

use codegg_core::jobs::{
    ExecutionSubjectDisposition, ExecutionSubjectRevision, ExecutionSubjectState, JobId,
    JobPayload, JobRecord, JobStore, SqliteJobStore,
};
use codegg_core::work_plan::{
    assess_work_plan, HostEvidenceStatus, WorkAcceptanceDisposition, WorkEvidenceKind, WorkItem,
    WorkItemStatus, WorkPlan, WorkPlanCompletionAssessment, WorkPlanStatus,
};
use eggplan_codegg_compat::{
    assess_codegg_snapshot, AcceptanceDisposition as CompatAcceptanceDisposition, CodeggAcceptance,
    CodeggAssessmentBridgeResult, CodeggCompletionFamily, CodeggEvidenceKind, CodeggEvidenceRef,
    CodeggItemSnapshot, CodeggItemStatus, CodeggPlanSnapshot, CodeggPlanStatus, EvidenceResolver,
    ResolvedEvidence,
};
use eggplan_core::{
    digest_json, verification_digest, EvidenceKind as EggplanEvidenceKind, EvidenceObservation,
    EvidenceObservationId, EvidenceObservationInput, EvidenceProviderId,
    EvidenceStatus as EggplanEvidenceStatus, ProviderDescriptor, ProviderRegistry, SubjectRevision,
    SubjectState as EggplanSubjectState, VerificationDigest,
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

// ── §3/§7/§14: pinned identities ────────────────────────────────────────────

/// Namespace passed to Eggplan's domain-separated `verification_digest`.
/// Provider namespaces are CodeGG-host configuration, never serialized
/// WorkPlan evidence text.
pub const VERIFICATION_NAMESPACE: &str = "codegg";

/// Version of the CodeGG-native canonical verification specification.
pub const VERIFICATION_SPEC_VERSION: u32 = 1;

/// Fixed trusted provider identity owned by CodeGG for host evidence.
/// Native payload data cannot select this ID or widen its allowed kinds.
pub const EGGPLAN_PROVIDER_ID: &str = "epp_codegg_host";

/// Provider class recorded on the fixed host descriptor.
pub const EGGPLAN_PROVIDER_CLASS: &str = "codegg-host";

/// Observation-ID domain separating M002 IDs from any other producer.
const OBSERVATION_ID_DOMAIN: &str = "codegg-eggplan-m002-observation-v1";

/// Bounded preview lengths mirror the legacy assessor so diagnostics carry
/// identifiers and short excerpts only, never full plan content.
const MAX_DETAIL_CHARS: usize = 200;
const MAX_ENGINE_DETAIL_CHARS: usize = 500;

// ── §6: explicit assessment-engine selection ────────────────────────────────

/// Which engine decided the assessment, and why.
///
/// The choice is deterministic from structured host state and observable in
/// `EggplanBackedAssessment::engine_detail`. `EggplanGit` is the only engine
/// that runs Eggplan assessment; every legacy engine returns the legacy
/// pure result verbatim (zero delta by construction) with a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssessmentEngine {
    /// Git-backed workspace, current subject captured, all non-human
    /// evidence kinds supported. Missing/stale/drifted/unbound historical
    /// evidence stays fail-closed inside Eggplan assessment.
    EggplanGit,
    /// The resolved workspace root exists but is positively not a Git
    /// subject (`NotGit` capture). Git capture errors never select this.
    LegacyNonGit,
    /// The plan cites a currently unsupported evidence kind
    /// (`Artifact`/`Commit`), or its shape cannot be represented in the
    /// pinned bridge input contract (empty item set, oversized item set,
    /// overlong source IDs, unmappable snapshot). Whole-assessment legacy.
    LegacyUnsupportedEvidence,
    /// No workspace identity is bound to the plan session, so no
    /// trustworthy current subject can be constructed. Whole-assessment
    /// legacy; preserves pre-M002 behavior for legacy sessions and tests.
    LegacyNoWorkspaceContext,
    /// Already-terminal (`Completed`/`Cancelled`) plan history. Terminal
    /// records are lifecycle history, never live-re-assessed.
    TerminalHistory,
}

impl AssessmentEngine {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EggplanGit => "eggplan_git",
            Self::LegacyNonGit => "legacy_non_git",
            Self::LegacyUnsupportedEvidence => "legacy_unsupported_evidence",
            Self::LegacyNoWorkspaceContext => "legacy_no_workspace_context",
            Self::TerminalHistory => "terminal_history",
        }
    }
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// Fail-closed adapter error. Callers must not mark the plan complete on
/// any of these; terminal-history and legacy-engine paths never produce
/// them (they return the legacy result instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssessmentAdapterError {
    /// The bound workspace is gone or unresolvable. Fail closed, no legacy
    /// fallback: the workspace existed but cannot be established now.
    NoWorkspaceContext(String),
    /// Git-backed workspace, but current capture failed (bounds, unsafe
    /// path, git failure). Fail closed; never route to `LegacyNonGit`.
    CurrentCaptureFailed(String),
    /// The pinned bridge rejected the mapping or assessment input.
    /// Indicates an adapter invariant violation, not unsatisfying evidence.
    BridgeRejected(String),
    /// Durable store read failure while resolving evidence or workspace.
    Store(String),
}

impl std::fmt::Display for AssessmentAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWorkspaceContext(detail) => {
                write!(f, "assessment workspace unavailable: {detail}")
            }
            Self::CurrentCaptureFailed(detail) => {
                write!(f, "current subject capture failed: {detail}")
            }
            Self::BridgeRejected(detail) => write!(f, "eggplan bridge rejected input: {detail}"),
            Self::Store(detail) => write!(f, "assessment store read failed: {detail}"),
        }
    }
}

impl std::error::Error for AssessmentAdapterError {}

impl From<AssessmentAdapterError> for String {
    fn from(error: AssessmentAdapterError) -> Self {
        error.to_string()
    }
}

fn bounded_detail(value: impl Into<String>) -> String {
    bounded_detail_n(value, MAX_DETAIL_CHARS)
}

fn bounded_detail_n(value: impl Into<String>, max_chars: usize) -> String {
    let value = value.into();
    value.chars().take(max_chars).collect()
}

// ── §10: CodeGG-facing assessment facade result ─────────────────────────────

/// Eggplan-backed assessment with its decision provenance.
///
/// `assessment` is the existing public DTO so arbiter/tool call sites keep
/// their result semantics. No Eggplan storage/repository handle escapes.
#[derive(Debug, Clone)]
pub struct EggplanBackedAssessment {
    /// The decision. For `EggplanGit` this is projected from the Eggplan
    /// completion family with detail populated from authoritative CodeGG
    /// plan/evidence state; for legacy engines it is the legacy result.
    pub assessment: WorkPlanCompletionAssessment,
    /// Which engine decided, per §6.
    pub engine: AssessmentEngine,
    /// Eggplan completion family (`None` unless `EggplanGit` ran).
    pub eggplan_completion_family: Option<String>,
    /// Sorted/deduped Eggplan reason codes (`None` unless `EggplanGit`).
    pub eggplan_reason_codes: Vec<String>,
    /// Mapping-manifest content digest (`None` unless `EggplanGit` ran).
    pub mapping_digest: Option<String>,
    /// Current subject S1 the assessment was judged against. Present only
    /// for `EggplanGit`; completion transitions must revalidate S2 == S1
    /// via [`revalidate_subject_for_completion`] before the status CAS.
    pub subject: Option<ExecutionSubjectRevision>,
    /// Bounded stable diagnostic naming the engine decision.
    pub engine_detail: String,
}

impl EggplanBackedAssessment {
    fn legacy(
        assessment: WorkPlanCompletionAssessment,
        engine: AssessmentEngine,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            assessment,
            engine,
            eggplan_completion_family: None,
            eggplan_reason_codes: Vec::new(),
            mapping_digest: None,
            subject: None,
            engine_detail: bounded_detail_n(detail, MAX_ENGINE_DETAIL_CHARS),
        }
    }
}

// ── §7: canonical verification specification ────────────────────────────────
//
// Every supported execution shape maps to a versioned canonical JSON payload
// hashed with Eggplan's domain-separated `verification_digest`. The payload
// carries execution semantics actually consumed by the scheduler executors
// (argv/cwd/policy/identity), never display strings, ref/job IDs as
// identity, credentials, timestamps, lease IDs, log output, labels, or node
// addresses.

/// Why a verification digest could not be derived. Terminal evidence
/// without a digest stays unbound/unavailable; it never falls back to a
/// ref-ID hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationUnavailable {
    /// Stable machine-readable reason (`unsupported_payload`,
    /// `missing_field`, `invalid_field`, `digest_bounds`).
    pub reason: &'static str,
    /// Bounded human diagnostic (no secrets; payload echoes are truncated).
    pub detail: String,
}

impl VerificationUnavailable {
    fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            reason: "unsupported_payload",
            detail: bounded_detail(detail.into()),
        }
    }

    fn missing(field: &'static str) -> Self {
        Self {
            reason: "missing_field",
            detail: field.to_string(),
        }
    }

    fn invalid(detail: impl Into<String>) -> Self {
        Self {
            reason: "invalid_field",
            detail: bounded_detail(detail.into()),
        }
    }
}

impl std::fmt::Display for VerificationUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "verification unavailable ({}): {}",
            self.reason, self.detail
        )
    }
}

impl std::error::Error for VerificationUnavailable {}

fn timeout_json(timeout: &Option<std::time::Duration>) -> serde_json::Value {
    match timeout {
        Some(duration) => {
            serde_json::json!({"secs": duration.as_secs(), "nanos": duration.subsec_nanos()})
        }
        None => serde_json::Value::Null,
    }
}

fn target_class(job: &JobRecord) -> &'static str {
    match &job.target {
        codegg_core::jobs::ExecutionTarget::Local => "local",
        // The node ID is transient routing state, never verification
        // identity: remote execution binds the input, not the node address.
        codegg_core::jobs::ExecutionTarget::EggworkNode { .. } => "eggwork",
    }
}

fn finalize_spec(
    payload: serde_json::Value,
) -> Result<VerificationDigest, VerificationUnavailable> {
    verification_digest(VERIFICATION_NAMESPACE, VERIFICATION_SPEC_VERSION, &payload).map_err(
        |detail| VerificationUnavailable {
            reason: "digest_bounds",
            detail: bounded_detail(detail),
        },
    )
}

/// Canonical spec for `TestJob` evidence: the durable `Test` execution
/// semantics (argv/cwd/scope/effective timeout/target). The display
/// `command` is not authoritative when argv exists, so it is excluded.
pub fn verification_digest_for_test(
    job: &JobRecord,
) -> Result<VerificationDigest, VerificationUnavailable> {
    let JobPayload::Test {
        argv, cwd, scope, ..
    } = &job.payload
    else {
        return Err(VerificationUnavailable::unsupported(
            "test_job evidence requires the durable Test payload",
        ));
    };
    if argv.is_empty() {
        return Err(VerificationUnavailable::missing("argv"));
    }
    finalize_spec(serde_json::json!({
        "spec_version": VERIFICATION_SPEC_VERSION,
        "kind": "test",
        "argv": argv,
        "cwd": cwd,
        "scope": scope,
        "timeout": timeout_json(&job.timeout),
        "target": target_class(job),
    }))
}

/// Canonical spec for `SchedulerJob` evidence over the executable payload
/// forms whose durable record reconstructs exact execution semantics:
/// `ManagedArgv`, canonical `Shell` with explicit argv, `Python` with a
/// durable source digest, and `Git` argv/cwd invocations.
///
/// A `Shell` without canonical argv fails closed even though the executor
/// would run `sh -c <command>`: the display command is not canonical
/// verification identity. A `Python` record without a durable source digest
/// fails closed: script-path content is not execution-time provenance.
pub fn verification_digest_for_scheduler_job(
    job: &JobRecord,
) -> Result<VerificationDigest, VerificationUnavailable> {
    match &job.payload {
        JobPayload::ManagedArgv { argv, cwd } => {
            if argv.is_empty() {
                return Err(VerificationUnavailable::missing("argv"));
            }
            finalize_spec(serde_json::json!({
                "spec_version": VERIFICATION_SPEC_VERSION,
                "kind": "managed_argv",
                "argv": argv,
                "cwd": cwd,
                "timeout": timeout_json(&job.timeout),
                "target": target_class(job),
            }))
        }
        JobPayload::Shell { argv, cwd, .. } => {
            let Some(argv) = argv else {
                return Err(VerificationUnavailable::missing("argv"));
            };
            if argv.is_empty() {
                return Err(VerificationUnavailable::missing("argv"));
            }
            finalize_spec(serde_json::json!({
                "spec_version": VERIFICATION_SPEC_VERSION,
                "kind": "shell",
                "argv": argv,
                "cwd": cwd,
                "timeout": timeout_json(&job.timeout),
                "target": target_class(job),
            }))
        }
        JobPayload::Python {
            script_path,
            args,
            mode,
            source,
            source_hash,
            cwd,
            timeout_secs,
        } => {
            let digest = match (source_hash, source) {
                (Some(hash), _) if is_bare_sha256_hex(hash) => format!("sha256:{hash}"),
                (Some(_), _) => {
                    return Err(VerificationUnavailable::invalid(
                        "python source_hash is not lowercase hex",
                    ));
                }
                (None, Some(source)) => {
                    format!("sha256:{:x}", Sha256::digest(source.as_bytes()))
                }
                (None, None) => {
                    return Err(VerificationUnavailable::missing("source_identity"));
                }
            };
            finalize_spec(serde_json::json!({
                "spec_version": VERIFICATION_SPEC_VERSION,
                "kind": "python",
                "script_path": script_path,
                "args": args,
                "mode": mode,
                "source_digest": digest,
                "cwd": cwd,
                "timeout_secs": timeout_secs,
                "record_timeout": timeout_json(&job.timeout),
                "target": target_class(job),
            }))
        }
        JobPayload::Git { argv, cwd } => {
            if argv.is_empty() {
                return Err(VerificationUnavailable::missing("argv"));
            }
            finalize_spec(serde_json::json!({
                "spec_version": VERIFICATION_SPEC_VERSION,
                "kind": "git",
                "argv": argv,
                "cwd": cwd,
                "timeout": timeout_json(&job.timeout),
                "target": target_class(job),
            }))
        }
        other => Err(VerificationUnavailable::unsupported(format!(
            "scheduler_job evidence requires an executable payload, found {}",
            payload_kind_name(other)
        ))),
    }
}

/// Canonical spec for delegated execution evidence (`DelegatedRun` refs and
/// positively linked `AgentRun` refs): the durable `SubagentRun`
/// specification. The prompt enters only as a SHA-256 digest; the persisted
/// Eggplan metadata never exposes prompt content.
///
/// Legacy `Subagent` payloads stay verification-unavailable: without the
/// durable task/run/delegation identity they cannot prove the same
/// specification.
pub fn verification_digest_for_delegated_run(
    job: &JobRecord,
) -> Result<VerificationDigest, VerificationUnavailable> {
    let JobPayload::SubagentRun {
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
    } = &job.payload
    else {
        return Err(VerificationUnavailable::unsupported(
            "delegated_run evidence requires the durable SubagentRun payload",
        ));
    };
    let mut denied = denied_tools.clone();
    denied.sort();
    let mut allowed = allowed_paths.clone();
    allowed.sort();
    finalize_spec(serde_json::json!({
        "spec_version": VERIFICATION_SPEC_VERSION,
        "kind": "delegated_run",
        "prompt_digest": format!("sha256:{:x}", Sha256::digest(prompt.as_bytes())),
        "agent": agent,
        "model": model,
        "parent_id": parent_id,
        "denied_tools": denied,
        "allowed_paths": allowed,
        "max_tool_calls": max_tool_calls,
        "task_id": task_id.as_str(),
        "run_id": run_id.as_str(),
        "delegation_key": delegation_key,
        "base_commit": base_commit,
    }))
}

/// Route one evidence kind to its authoritative native specification.
pub fn verification_digest_for_evidence(
    kind: WorkEvidenceKind,
    job: &JobRecord,
) -> Result<VerificationDigest, VerificationUnavailable> {
    match kind {
        WorkEvidenceKind::TestJob => verification_digest_for_test(job),
        WorkEvidenceKind::SchedulerJob => verification_digest_for_scheduler_job(job),
        WorkEvidenceKind::DelegatedRun | WorkEvidenceKind::AgentRun => {
            verification_digest_for_delegated_run(job)
        }
        WorkEvidenceKind::Artifact | WorkEvidenceKind::Commit => {
            Err(VerificationUnavailable::unsupported(
                "artifact/commit authority is out of scope for M002",
            ))
        }
    }
}

fn payload_kind_name(payload: &JobPayload) -> &'static str {
    match payload {
        JobPayload::AgentTurn { .. } => "agent_turn",
        JobPayload::Subagent { .. } => "subagent",
        JobPayload::SubagentRun { .. } => "subagent_run",
        JobPayload::Test { .. } => "test",
        JobPayload::ManagedArgv { .. } => "managed_argv",
        JobPayload::Shell { .. } => "shell",
        JobPayload::Python { .. } => "python",
        JobPayload::Git { .. } => "git",
        JobPayload::Research { .. } => "research",
        JobPayload::ToolProgram { .. } => "tool_program",
        JobPayload::Maintenance { .. } => "maintenance",
    }
}

fn is_bare_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// ── §4: canonical workspace resolution ──────────────────────────────────────

/// Canonical workspace context for assessment: the governed root plus the
/// workspace identity used for current-subject construction.
#[derive(Debug, Clone)]
pub struct AssessmentWorkspaceContext {
    pub workspace_root: PathBuf,
    pub workspace_id: codegg_core::workspace::WorkspaceId,
}

/// Resolve the canonical CodeGG workspace root for a plan.
///
/// The root comes from the workspace catalog via the plan session's bound
/// workspace identity, falling back to the session-recorded directory only
/// when the catalog entry is missing. Both are host-recorded; process CWD
/// and model input never participate.
///
/// Returns `Ok(None)` when the session binds no workspace identity at all:
/// without an identity no trustworthy current subject can be constructed,
/// so the caller stays on the legacy engine. Returns `Err` when an identity
/// exists but no usable root remains: the workspace is gone, and assessing
/// (let alone completing) against it must fail closed.
pub async fn resolve_assessment_workspace(
    pool: &SqlitePool,
    plan: &WorkPlan,
) -> Result<Option<AssessmentWorkspaceContext>, AssessmentAdapterError> {
    let sessions = codegg_core::session::store::SessionStore::new(pool.clone());
    let session = sessions
        .get(&plan.session_id)
        .await
        .map_err(|error| AssessmentAdapterError::Store(bounded_detail(error.to_string())))?;
    let Some(session) = session else {
        return Ok(None);
    };
    let Some(workspace_id) = session.workspace_id.clone() else {
        return Ok(None);
    };
    let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
    let workspaces = codegg_core::workspace::SqliteWorkspaceStore::new(pool.clone());
    if let Ok(Some(record)) =
        codegg_core::workspace::WorkspaceStore::load_by_id(&workspaces, workspace_id.as_str()).await
    {
        if record.canonical_root.is_dir() {
            return Ok(Some(AssessmentWorkspaceContext {
                workspace_root: record.canonical_root,
                workspace_id,
            }));
        }
    }
    // Catalog entry missing or root gone: the session-recorded directory is
    // the only remaining host-recorded locator. It must exist on disk;
    // otherwise the workspace is gone and assessment fails closed.
    let candidate = PathBuf::from(&session.directory);
    let canonical = std::fs::canonicalize(&candidate).map_err(|_| {
        AssessmentAdapterError::NoWorkspaceContext(format!(
            "workspace '{}' has no usable root",
            workspace_id.as_str()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(AssessmentAdapterError::NoWorkspaceContext(format!(
            "workspace '{}' has no usable root",
            workspace_id.as_str()
        )));
    }
    Ok(Some(AssessmentWorkspaceContext {
        workspace_root: canonical,
        workspace_id,
    }))
}

/// Capture the current assessment subject (S1) for a resolved workspace.
///
/// The scheduler-owned capture result maps onto facade semantics:
/// `Git` carries the revision; `NonGit` positively establishes a non-Git
/// workspace (legacy engine). Every other capture failure is fail-closed.
pub enum CurrentSubject {
    Git(ExecutionSubjectRevision),
    NonGit,
}

pub async fn capture_assessment_subject(
    ctx: &AssessmentWorkspaceContext,
) -> Result<CurrentSubject, AssessmentAdapterError> {
    use codegg_core::jobs::ExecutionSubjectUnavailableReason as Reason;
    match crate::scheduler::assessment_subject::capture_current_assessment_subject(
        &ctx.workspace_root,
        &ctx.workspace_id,
    )
    .await
    {
        Ok(revision) => Ok(CurrentSubject::Git(revision)),
        Err(Reason::NotGit) => Ok(CurrentSubject::NonGit),
        Err(reason) => Err(AssessmentAdapterError::CurrentCaptureFailed(format!(
            "{reason:?}"
        ))),
    }
}

// ── §8/§9: resolved evidence → Eggplan observation adapter ──────────────────
//
// Every observation carries the CURRENT subject S1 (the bridge requires
// exact subject equality); the durable historical subject decides only the
// observation status. Terminal evidence with a Stable sealed subject equal
// to S1 keeps its terminal status; every other terminal history (drifted,
// unavailable, legacy, stale, cross-workspace) becomes an `Unavailable`
// observation with its verification binding so assessment stays
// fail-closed without fabricating satisfaction. Evidence whose binding
// cannot be derived is excluded from the snapshot: schema-v2 execution
// observations require bindings, and an unbound requirement can never be
// satisfied, so exclusion only removes unsatisfiable requirements (never
// manufactures satisfaction). Missing store targets produce no requirement
// at all (legacy `Unavailable` equivalent: never satisfied).

/// One bindable evidence occurrence: host status plus the authoritative
/// verification binding, ready to project into an observation.
///
/// Public so differential tests and the snapshot builder can share the
/// exact production binding map.
#[derive(Debug, Clone)]
pub struct BoundEvidence {
    kind: WorkEvidenceKind,
    ref_id: String,
    status: HostEvidenceStatus,
    digest: VerificationDigest,
    job_id: String,
    attempt_id: Option<String>,
    observed_at_unix_ms: u64,
    stable_exact: bool,
}

/// Bind one resolved evidence entry to its authoritative native
/// specification. Returns `None` when no schema-valid observation can be
/// built (missing target, underivable binding); the caller then leaves the
/// ref out of the Eggplan snapshot so the requirement reads as missing.
async fn bind_resolved_evidence(
    store: &SqliteJobStore,
    entry: &crate::work_plan_evidence::ResolvedWorkEvidence,
    current: &SubjectRevision,
) -> Option<BoundEvidence> {
    if entry.status == HostEvidenceStatus::Unavailable {
        return None;
    }
    let job_id = entry.native_job_id.as_deref()?;
    let job = store
        .get_job(&JobId::new_unchecked(job_id.to_string()))
        .await
        .ok()??;
    let digest = verification_digest_for_evidence(entry.kind, &job).ok()?;
    let attempts = store.list_attempts(&job.job_id).await.unwrap_or_default();
    let attempt = entry
        .native_attempt_id
        .as_deref()
        .and_then(|id| {
            attempts.iter().find(|a| {
                a.attempt_id == codegg_core::jobs::AttemptId::new_unchecked(id.to_string())
            })
        })
        .or_else(|| attempts.iter().max_by_key(|a| a.sequence));
    let stable_exact = matches!(
        (&entry.source_subject_disposition, &entry.source_subject),
        (ExecutionSubjectDisposition::Stable, Some(provenance))
        if sealed_matches_current(provenance, current)
    );
    let observed_at_unix_ms = match entry.status {
        HostEvidenceStatus::Passed | HostEvidenceStatus::Failed => attempt
            .and_then(|a| a.completed_at)
            .or(job.terminal_at)
            .map(|ts| ts.timestamp_millis().max(0) as u64)
            .unwrap_or(0),
        HostEvidenceStatus::InProgress => attempt
            .and_then(|a| a.started_at)
            .map(|ts| ts.timestamp_millis().max(0) as u64)
            .unwrap_or(0),
        HostEvidenceStatus::Unavailable => 0,
    };
    Some(BoundEvidence {
        kind: entry.kind,
        ref_id: entry.ref_id.clone(),
        status: entry.status,
        digest,
        job_id: job_id.to_string(),
        attempt_id: attempt.map(|a| a.attempt_id.to_string()),
        observed_at_unix_ms,
        stable_exact,
    })
}

/// True when the sealed historical subject is exactly the current one:
/// same repository identity, revision, state, and dirty digest.
fn sealed_matches_current(
    provenance: &codegg_core::jobs::ExecutionSubjectProvenance,
    current: &SubjectRevision,
) -> bool {
    let Some(sealed) = provenance.sealed.as_ref() else {
        return false;
    };
    let state_matches = matches!(
        (&sealed.state, current.state),
        (ExecutionSubjectState::Clean, EggplanSubjectState::Clean)
            | (ExecutionSubjectState::Dirty, EggplanSubjectState::Dirty)
    );
    sealed.repository_identity == current.repository_id
        && sealed.revision == current.revision
        && state_matches
        && digests_equal(
            sealed.dirty_digest.as_deref(),
            current.dirty_digest.as_deref(),
        )
}

/// Deterministic observation ID from stable native identity: evidence kind,
/// ref, job, and attempt. No randomness, no wall-clock, no per-process
/// state; repeated assessments derive the same ID. The resolver suffixes
/// repeat occurrences of one ref (`_2`, …) so shared evidence keeps its
/// requirement in every citing item while IDs stay unique and stable.
fn observation_base_id(
    kind: WorkEvidenceKind,
    ref_id: &str,
    job_id: &str,
    attempt_id: Option<&str>,
) -> Result<String, String> {
    let digest = digest_json(&(
        OBSERVATION_ID_DOMAIN,
        kind.as_str(),
        ref_id,
        job_id,
        attempt_id.unwrap_or(""),
    ))
    .map_err(|error| error.to_string())?;
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| "unexpected digest format".to_string())?;
    Ok(format!("epe_{}", &hex[..32]))
}

/// The host evidence resolver handed to the bridge. Pre-bound templates
/// are keyed by `(kind, ref_id)`; repeat occurrences receive deterministic
/// suffixed IDs so every citing item keeps its requirement.
struct SnapshotEvidenceResolver {
    subject: SubjectRevision,
    templates: HashMap<(String, String), BoundEvidence>,
    occurrences: HashMap<(String, String), usize>,
}

impl SnapshotEvidenceResolver {
    fn observation_for(&mut self, bound: &BoundEvidence) -> Result<EvidenceObservation, String> {
        let key = (bound.kind.as_str().to_string(), bound.ref_id.clone());
        let occurrence = self.occurrences.entry(key).or_insert(0);
        *occurrence += 1;
        let base = observation_base_id(
            bound.kind,
            &bound.ref_id,
            &bound.job_id,
            bound.attempt_id.as_deref(),
        )?;
        let id = if *occurrence == 1 {
            base
        } else {
            format!("{base}_{occurrence}")
        };
        // Terminal history without Stable exact-subject proof becomes an
        // Unavailable observation: Eggplan reads it as missing evidence,
        // never as satisfaction. In-flight stays In-flight (it cannot
        // satisfy completion either way).
        let status = match bound.status {
            HostEvidenceStatus::Passed | HostEvidenceStatus::Failed if bound.stable_exact => {
                match bound.status {
                    HostEvidenceStatus::Passed => EggplanEvidenceStatus::Passed,
                    _ => EggplanEvidenceStatus::Failed,
                }
            }
            HostEvidenceStatus::InProgress => EggplanEvidenceStatus::InProgress,
            _ => EggplanEvidenceStatus::Unavailable,
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("codegg_kind".to_string(), bound.kind.as_str().to_string());
        metadata.insert(
            "codegg_ref".to_string(),
            bounded_detail(bound.ref_id.clone()),
        );
        metadata.insert(
            "codegg_job".to_string(),
            bounded_detail(bound.job_id.clone()),
        );
        if let Some(attempt) = bound.attempt_id.as_deref() {
            if !attempt.is_empty() {
                metadata.insert("codegg_attempt".to_string(), bounded_detail(attempt));
            }
        }
        EvidenceObservation::finalize(EvidenceObservationInput {
            id: EvidenceObservationId::new(id).map_err(|error| error.to_string())?,
            provider_id: EvidenceProviderId::new(EGGPLAN_PROVIDER_ID.to_string())
                .map_err(|error| error.to_string())?,
            kind: match bound.kind {
                WorkEvidenceKind::TestJob => EggplanEvidenceKind::Test,
                WorkEvidenceKind::SchedulerJob => EggplanEvidenceKind::Command,
                WorkEvidenceKind::DelegatedRun | WorkEvidenceKind::AgentRun => {
                    EggplanEvidenceKind::DelegatedRun
                }
                WorkEvidenceKind::Artifact | WorkEvidenceKind::Commit => {
                    return Err("artifact/commit must not reach the Eggplan adapter".to_string());
                }
            },
            status,
            subject: self.subject.clone(),
            observed_at_unix_ms: bound.observed_at_unix_ms,
            invocation_ref: Some(bounded_detail(bound.ref_id.clone())),
            verification_digest: Some(bound.digest.clone()),
            result_metadata: metadata,
            artifacts: Vec::new(),
        })
        .map_err(|error| error.to_string())
    }
}

impl EvidenceResolver for SnapshotEvidenceResolver {
    fn resolve(
        &mut self,
        reference: &CodeggEvidenceRef,
        subject: &SubjectRevision,
    ) -> Result<ResolvedEvidence, String> {
        if subject != &self.subject {
            return Err("resolver called with a non-current subject".to_string());
        }
        let kind = match reference.kind {
            CodeggEvidenceKind::TestJob => WorkEvidenceKind::TestJob,
            CodeggEvidenceKind::DelegatedRun => WorkEvidenceKind::DelegatedRun,
            CodeggEvidenceKind::SchedulerJob => WorkEvidenceKind::SchedulerJob,
            CodeggEvidenceKind::AgentRun => WorkEvidenceKind::AgentRun,
            CodeggEvidenceKind::Artifact | CodeggEvidenceKind::Commit => {
                return Err("unsupported evidence kind in Eggplan snapshot".to_string());
            }
        };
        let key = (kind.as_str().to_string(), reference.ref_id.clone());
        let bound = self
            .templates
            .get(&key)
            .cloned()
            .ok_or_else(|| "resolver has no binding for snapshot reference".to_string())?;
        let observation = self.observation_for(&bound)?;
        Ok(ResolvedEvidence {
            observation,
            expected_verification: Some(bound.digest),
        })
    }
}

// ── §9: plan snapshot mapping ───────────────────────────────────────────────
//
// The snapshot projects current CodeGG values into the bridge input. Two
// bounded normalizations preserve CodeGG semantics across the model gap
// (Eggplan's human-judgment criterion is evidence-independent, CodeGG's is
// not):
//
// - N1: an item mixing `RequiresUserJudgment` with any non-human
//   disposition projects its human acceptances as `Unmet`. CodeGG gives
//   non-human dispositions decisive force (`Unmet` blocks judgment-only;
//   `Satisfied` satisfies outright); without normalization Eggplan's human
//   criterion would dominate and allow completion CodeGG would refuse.
// - N2: a human-only item with any non-`Passed` evidence ref projects its
//   human acceptances as `Unmet`. CodeGG judgment-only requires clean
//   evidence; failed/in-flight/dangling evidence must keep the item open.
//
// The mapping manifest records the ORIGINAL source dispositions, so the
// normalization is auditable; the snapshot carries the projection.

/// Why a plan shape cannot enter the bridge input contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRejected {
    pub reason: &'static str,
    pub detail: String,
}

impl std::fmt::Display for SnapshotRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.reason, self.detail)
    }
}

impl std::error::Error for SnapshotRejected {}

/// Bridge source-ID bound (`check_source_id` in the compat crate): IDs must
/// be non-empty, NUL-free, and at most 128 chars. CodeGG's own scope bound
/// is wider (256), so overlong IDs are rejected here with a legacy-engine
/// diagnostic instead of a bridge error.
fn check_bridge_id(value: &str, field: &'static str) -> Result<(), SnapshotRejected> {
    if value.is_empty() || value.contains('\0') || value.chars().count() > 128 {
        return Err(SnapshotRejected {
            reason: "source_id_exceeds_bridge_bound",
            detail: format!("{field} is empty, contains NUL, or exceeds 128 chars"),
        });
    }
    Ok(())
}

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

fn map_evidence_kind(kind: WorkEvidenceKind) -> Option<CodeggEvidenceKind> {
    match kind {
        WorkEvidenceKind::TestJob => Some(CodeggEvidenceKind::TestJob),
        WorkEvidenceKind::DelegatedRun => Some(CodeggEvidenceKind::DelegatedRun),
        WorkEvidenceKind::SchedulerJob => Some(CodeggEvidenceKind::SchedulerJob),
        WorkEvidenceKind::AgentRun => Some(CodeggEvidenceKind::AgentRun),
        WorkEvidenceKind::Artifact | WorkEvidenceKind::Commit => None,
    }
}

/// Project one item's acceptances with the N1/N2 normalizations applied.
/// Returns the projected acceptances plus whether normalization fired.
fn project_acceptances(
    item: &WorkItem,
    statuses: &HashMap<(String, String), HostEvidenceStatus>,
) -> Vec<CodeggAcceptance> {
    let has_human = item.acceptance.iter().any(|a| {
        matches!(
            a.disposition,
            WorkAcceptanceDisposition::RequiresUserJudgment
        )
    });
    let normalize = if !has_human {
        false
    } else if item.acceptance.iter().any(|a| {
        !matches!(
            a.disposition,
            WorkAcceptanceDisposition::RequiresUserJudgment
        )
    }) {
        // N1: a non-human disposition dominates judgment-only semantics.
        true
    } else {
        // N2: judgment-only requires clean evidence.
        item.evidence.iter().any(|evidence| {
            statuses
                .get(&(evidence.kind.as_str().to_string(), evidence.ref_id.clone()))
                .is_some_and(|status| *status != HostEvidenceStatus::Passed)
        })
    };
    item.acceptance
        .iter()
        .map(|acceptance| {
            let disposition = match acceptance.disposition {
                WorkAcceptanceDisposition::Unmet => CompatAcceptanceDisposition::Unmet,
                WorkAcceptanceDisposition::Satisfied => CompatAcceptanceDisposition::Satisfied,
                WorkAcceptanceDisposition::RequiresUserJudgment if normalize => {
                    CompatAcceptanceDisposition::Unmet
                }
                WorkAcceptanceDisposition::RequiresUserJudgment => {
                    CompatAcceptanceDisposition::RequiresUserJudgment
                }
            };
            CodeggAcceptance {
                description: acceptance.description.clone(),
                disposition,
                note: acceptance.note.clone(),
            }
        })
        .collect()
}

/// Build the bridge snapshot for one plan. Only bindable refs (present in
/// `bound`) become requirements; unbindable refs read as missing evidence.
/// Returns the snapshot plus whether any item acceptance was normalized.
pub fn build_plan_snapshot(
    plan: &WorkPlan,
    items: &[WorkItem],
    resolved: &crate::work_plan_evidence::ResolvedWorkEvidenceSnapshot,
    bound: &HashMap<(String, String), BoundEvidence>,
) -> Result<(CodeggPlanSnapshot, bool), SnapshotRejected> {
    if items.is_empty() {
        return Err(SnapshotRejected {
            reason: "empty_item_set",
            detail: "the bridge input contract requires at least one item".to_string(),
        });
    }
    if items.len() > 64 {
        return Err(SnapshotRejected {
            reason: "item_count_exceeds_bridge_bound",
            detail: format!("{} items exceed the 64-item bridge bound", items.len()),
        });
    }
    if plan.revision < 0 {
        return Err(SnapshotRejected {
            reason: "negative_plan_revision",
            detail: "plan revision must be non-negative".to_string(),
        });
    }
    check_bridge_id(plan.id.as_str(), "plan ID")?;
    let statuses: HashMap<(String, String), HostEvidenceStatus> = resolved
        .entries
        .iter()
        .map(|entry| {
            (
                (entry.kind.as_str().to_string(), entry.ref_id.clone()),
                entry.status,
            )
        })
        .collect();
    let mut normalized_any = false;
    let mut snapshot_items = Vec::with_capacity(items.len());
    for item in items {
        check_bridge_id(item.id.as_str(), "item ID")?;
        if item.acceptance.len() > 16 {
            return Err(SnapshotRejected {
                reason: "acceptance_count_exceeds_bridge_bound",
                detail: format!("item {} exceeds 16 acceptances", item.id.as_str()),
            });
        }
        for reference in item
            .parent_item_id
            .iter()
            .map(|id| id.as_str())
            .chain(item.dependencies.iter().map(|id| id.as_str()))
        {
            check_bridge_id(reference, "parent or dependency ID")?;
        }
        if let Some(owner) = item.owner_run_id.as_deref() {
            check_bridge_id(owner, "owner run ID")?;
        }
        if let Some(owner) = item.owner_job_id.as_deref() {
            check_bridge_id(owner, "owner job ID")?;
        }
        let acceptances = project_acceptances(item, &statuses);
        normalized_any |=
            acceptances
                .iter()
                .zip(item.acceptance.iter())
                .any(|(projected, source)| {
                    matches!(
                        source.disposition,
                        WorkAcceptanceDisposition::RequiresUserJudgment
                    ) && matches!(projected.disposition, CompatAcceptanceDisposition::Unmet)
                });
        let mut evidence = Vec::new();
        for reference in &item.evidence {
            let Some(kind) = map_evidence_kind(reference.kind) else {
                continue;
            };
            check_bridge_id(&reference.ref_id, "evidence reference")?;
            if reference
                .detail
                .as_ref()
                .is_some_and(|d| d.chars().count() > 2000)
            {
                return Err(SnapshotRejected {
                    reason: "evidence_detail_exceeds_bridge_bound",
                    detail: format!(
                        "evidence ref {} detail exceeds 2000 chars",
                        reference.ref_id
                    ),
                });
            }
            // Only bindable refs become requirements; the rest read as
            // missing evidence (schema-v2 bindings are mandatory).
            if !bound.contains_key(&(
                reference.kind.as_str().to_string(),
                reference.ref_id.clone(),
            )) {
                continue;
            }
            evidence.push(CodeggEvidenceRef {
                kind,
                ref_id: reference.ref_id.clone(),
                detail: reference.detail.clone(),
            });
        }
        if evidence.len() > 32 {
            return Err(SnapshotRejected {
                reason: "evidence_count_exceeds_bridge_bound",
                detail: format!("item {} exceeds 32 evidence refs", item.id.as_str()),
            });
        }
        snapshot_items.push(CodeggItemSnapshot {
            item_id: item.id.as_str().to_string(),
            position: item.position,
            parent_item_id: item
                .parent_item_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            dependencies: item
                .dependencies
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            status: map_item_status(item.status),
            description: item.description.clone(),
            acceptance: acceptances,
            evidence,
            owner_run_id: item.owner_run_id.clone(),
            owner_job_id: item.owner_job_id.clone(),
            blocker: item.blocker.clone(),
            next_action: item.next_action.clone(),
        });
    }
    if let Some(current) = plan.current_item_id.as_ref() {
        check_bridge_id(current.as_str(), "current item ID")?;
    }
    Ok((
        CodeggPlanSnapshot {
            plan_id: plan.id.as_str().to_string(),
            revision: plan.revision,
            status: map_plan_status(plan.status),
            objective: plan.objective.clone(),
            current_phase: plan.current_phase.clone(),
            current_item_id: plan
                .current_item_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            items: snapshot_items,
        },
        normalized_any,
    ))
}

// ── §14: host provider trust ─────────────────────────────────────────────────
//
// Trust is host code, never serialized evidence text. The registry carries
// exactly the fixed CodeGG-host descriptor; no model/provider plugin can
// self-register as trusted evidence authority, and the LLM
// `ProviderRegistry` is never consulted here (different domain).

fn codegg_host_providers() -> Result<ProviderRegistry, String> {
    let mut registry = ProviderRegistry::default();
    registry
        .register_trusted(
            ProviderDescriptor::new(
                EvidenceProviderId::new(EGGPLAN_PROVIDER_ID.to_string())
                    .map_err(|error| error.to_string())?,
                EGGPLAN_PROVIDER_CLASS,
                [
                    EggplanEvidenceKind::Test,
                    EggplanEvidenceKind::Command,
                    EggplanEvidenceKind::DelegatedRun,
                ],
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    Ok(registry)
}

// ── Subject conversion ──────────────────────────────────────────────────────

/// Project the durable CodeGG subject into the Eggplan input shape. The
/// five fields are pinned by C002 golden tests; this constructs the live
/// Eggplan value from the same durable record.
///
/// Format translation: M001 persists the dirty digest as bare lowercase
/// hex while Eggplan's `SubjectRevision` requires `sha256:`-prefixed
/// digests. The adapter prefixes (validating first) so the durable shape
/// stays exactly as M001 closed it.
fn eggplan_subject(revision: &ExecutionSubjectRevision) -> Result<SubjectRevision, String> {
    let fields = revision.to_eggplan_fields();
    let dirty_digest = match fields.dirty_digest {
        None => None,
        Some(digest) if is_bare_sha256_hex(&digest) => Some(format!("sha256:{digest}")),
        Some(digest) => {
            // Already namespaced (defensive): accept only exact shape.
            let valid = digest.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
            if valid {
                Some(digest)
            } else {
                return Err("durable dirty digest is not canonical sha256 hex".to_string());
            }
        }
    };
    let subject = SubjectRevision {
        subject_kind: fields.subject_kind,
        repository_id: fields.repository_id,
        revision: fields.revision,
        state: match fields.state.as_str() {
            "clean" => EggplanSubjectState::Clean,
            "dirty" => EggplanSubjectState::Dirty,
            _ => return Err("unknown subject state in durable record".to_string()),
        },
        dirty_digest,
    };
    subject.validate().map_err(|error| error.to_string())?;
    Ok(subject)
}

/// Compare digests across the M001/Eggplan namespace difference: either
/// side may carry the `sha256:` prefix.
fn digests_equal(a: Option<&str>, b: Option<&str>) -> bool {
    fn strip(value: &str) -> &str {
        value.strip_prefix("sha256:").unwrap_or(value)
    }
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => strip(a) == strip(b),
        _ => false,
    }
}

// ── Adapter runner (pool + items + explicit S1; no session/git) ─────────────
//
// Split from the facade so differential qualification can drive the exact
// production adapter against synthetic subjects without git or sessions.

/// Bound-evidence templates for one assessment, keyed by `(kind, ref_id)`.
async fn bind_plan_evidence(
    pool: &SqlitePool,
    resolved: &crate::work_plan_evidence::ResolvedWorkEvidenceSnapshot,
    current: &SubjectRevision,
) -> HashMap<(String, String), BoundEvidence> {
    let store = SqliteJobStore::new(pool.clone());
    let mut bound = HashMap::new();
    for entry in &resolved.entries {
        if bound.contains_key(&(entry.kind.as_str().to_string(), entry.ref_id.clone())) {
            continue;
        }
        if let Some(record) = bind_resolved_evidence(&store, entry, current).await {
            bound.insert(
                (entry.kind.as_str().to_string(), entry.ref_id.clone()),
                record,
            );
        }
    }
    bound
}

/// Run the production adapter end to end against an explicit current
/// subject: resolve, bind, snapshot, assess. Used by the facade (with the
/// captured S1) and by differential tests (with synthetic subjects).
/// Returns the bridge result plus the host resolution the adapter
/// consumed (needed for DTO detail projection).
pub async fn assess_snapshot_with_subject(
    pool: &SqlitePool,
    plan: &WorkPlan,
    items: &[WorkItem],
    current: &SubjectRevision,
) -> Result<
    (
        CodeggAssessmentBridgeResult,
        crate::work_plan_evidence::ResolvedWorkEvidenceSnapshot,
    ),
    AssessmentAdapterError,
> {
    let resolved = crate::work_plan_evidence::assemble_resolved(pool, items)
        .await
        .map_err(AssessmentAdapterError::Store)?;
    let bound = bind_plan_evidence(pool, &resolved, current).await;
    let (snapshot, _) =
        build_plan_snapshot(plan, items, &resolved, &bound).map_err(|rejected| {
            AssessmentAdapterError::BridgeRejected(format!("snapshot mapping: {rejected}"))
        })?;
    let mut resolver = SnapshotEvidenceResolver {
        subject: current.clone(),
        templates: bound,
        occurrences: HashMap::new(),
    };
    let providers = codegg_host_providers().map_err(AssessmentAdapterError::BridgeRejected)?;
    let result = assess_codegg_snapshot(&snapshot, current.clone(), &mut resolver, &providers)
        .map_err(|detail| AssessmentAdapterError::BridgeRejected(bounded_detail(detail)))?;
    result.manifest.validate().map_err(|detail| {
        AssessmentAdapterError::BridgeRejected(format!("mapping manifest: {detail}"))
    })?;
    Ok((result, resolved))
}

// ── §10: family → DTO projection ─────────────────────────────────────────────
//
// The Eggplan completion family is the decision family. All item detail
// (actionable identity, blocker, handle, awaiting-user reasons) comes from
// authoritative CodeGG plan/evidence state, never from free-form Eggplan
// diagnostic text (which must not become scheduler authority).

fn bounded_preview(value: &str) -> String {
    value.trim().chars().take(MAX_DETAIL_CHARS).collect()
}

fn source_item_for_eggplan_id<'a>(
    items: &'a [WorkItem],
    manifest: &eggplan_codegg_compat::MappingManifest,
    eggplan_id: &str,
) -> Option<&'a WorkItem> {
    let source = manifest
        .id_map
        .iter()
        .find(|entry| entry.eggplan_id == eggplan_id)?;
    items
        .iter()
        .find(|item| item.id.as_str() == source.source_id)
}

/// First source item (plan order) whose mapped assessment is not `Complete`.
fn first_incomplete_source_item<'a>(
    items: &'a [WorkItem],
    result: &CodeggAssessmentBridgeResult,
) -> Option<&'a WorkItem> {
    for assessed in &result.assessment.items {
        if assessed.status != eggplan_core::AssessmentStatus::Complete {
            if let Some(source) =
                source_item_for_eggplan_id(items, &result.manifest, assessed.item_id.as_str())
            {
                return Some(source);
            }
        }
    }
    None
}

fn unmet_acceptance_count(item: &WorkItem) -> usize {
    item.acceptance
        .iter()
        .filter(|c| matches!(c.disposition, WorkAcceptanceDisposition::Unmet))
        .count()
}

/// Project the bridge result onto the existing public DTO.
///
/// `resolved` is the same host resolution the adapter consumed, used only
/// to recover the in-flight handle with the legacy precedence.
pub fn project_bridge_assessment(
    plan: &WorkPlan,
    items: &[WorkItem],
    resolved: &crate::work_plan_evidence::ResolvedWorkEvidenceSnapshot,
    result: &CodeggAssessmentBridgeResult,
) -> WorkPlanCompletionAssessment {
    match result.completion_family {
        CodeggCompletionFamily::Complete => WorkPlanCompletionAssessment::Complete {
            reason: "all required items are complete".to_string(),
        },
        CodeggCompletionFamily::ActionableWorkRemaining => {
            let current = first_incomplete_source_item(items, result)
                .or_else(|| items.iter().find(|item| !item.status.is_terminal()));
            match current {
                Some(item) => WorkPlanCompletionAssessment::ActionableWorkRemaining {
                    current_item_id: item.id.clone(),
                    description: bounded_preview(&item.description),
                    unmet_count: unmet_acceptance_count(item),
                    next_action: item
                        .next_action
                        .as_deref()
                        .map(bounded_preview)
                        .filter(|action| !action.is_empty()),
                },
                None => WorkPlanCompletionAssessment::Complete {
                    reason: "all required items are complete".to_string(),
                },
            }
        }
        CodeggCompletionFamily::Blocked => {
            let blocked = items
                .iter()
                .find(|item| item.status == WorkItemStatus::Blocked);
            match blocked {
                Some(item) => WorkPlanCompletionAssessment::Blocked {
                    item_id: Some(item.id.clone()),
                    blocker: item
                        .blocker
                        .as_deref()
                        .map(bounded_preview)
                        .filter(|blocker| !blocker.is_empty())
                        .unwrap_or_else(|| "item is blocked".to_string()),
                },
                None => WorkPlanCompletionAssessment::Blocked {
                    item_id: plan
                        .current_item_id
                        .as_ref()
                        .and_then(|id| items.iter().find(|item| &item.id == id))
                        .map(|item| item.id.clone()),
                    blocker: "plan is blocked".to_string(),
                },
            }
        }
        CodeggCompletionFamily::AwaitingUserJudgment => {
            let mut reasons: Vec<String> = items
                .iter()
                .filter(|item| {
                    item.acceptance.iter().any(|c| {
                        matches!(
                            c.disposition,
                            WorkAcceptanceDisposition::RequiresUserJudgment
                        )
                    })
                })
                .take(4)
                .map(|item| {
                    format!(
                        "{}: awaiting user judgment",
                        bounded_preview(&item.description)
                    )
                })
                .collect();
            if reasons.is_empty() {
                reasons.push("remaining criteria require user judgment".to_string());
            }
            for code in result.reason_codes.iter().take(4) {
                reasons.push(format!("eggplan:{code}"));
            }
            WorkPlanCompletionAssessment::AwaitingUserJudgment { reasons }
        }
        CodeggCompletionFamily::InFlight => {
            let (kind, id) = inflight_handle_for(items, resolved);
            WorkPlanCompletionAssessment::InFlight {
                item_id: items
                    .iter()
                    .find(|item| item.status == WorkItemStatus::InProgress)
                    .map(|item| item.id.clone()),
                handle_kind: kind,
                handle_id: id,
            }
        }
    }
}

/// Resolve the in-flight handle from resolved CodeGG evidence, mirroring
/// the legacy `live_handle_for`/`owner_handle_for` precedence: the first
/// `InProgress` ref in item order, else owner run/job provenance, else the
/// unknown fallback.
fn inflight_handle_for(
    items: &[WorkItem],
    resolved: &crate::work_plan_evidence::ResolvedWorkEvidenceSnapshot,
) -> (String, String) {
    let statuses: HashMap<(String, String), HostEvidenceStatus> = resolved
        .entries
        .iter()
        .map(|entry| {
            (
                (entry.kind.as_str().to_string(), entry.ref_id.clone()),
                entry.status,
            )
        })
        .collect();
    for item in items {
        if item.status != WorkItemStatus::InProgress {
            continue;
        }
        for evidence in &item.evidence {
            if statuses.get(&(evidence.kind.as_str().to_string(), evidence.ref_id.clone()))
                == Some(&HostEvidenceStatus::InProgress)
            {
                return (
                    bounded_preview(evidence.kind.as_str()),
                    bounded_preview(&evidence.ref_id),
                );
            }
        }
        if let Some(owner) = item.owner_run_id.as_deref() {
            return ("delegated_run".to_string(), bounded_preview(owner));
        }
        if let Some(owner) = item.owner_job_id.as_deref() {
            return ("scheduler_job".to_string(), bounded_preview(owner));
        }
    }
    ("unknown".to_string(), "unknown".to_string())
}

// ── §10: facade ─────────────────────────────────────────────────────────────

/// Legacy compatibility result: the legacy pure assessment verbatim with
/// an explicit engine diagnostic. Used for every non-EggplanGit route so
/// behavior is byte-identical to pre-M002.
async fn legacy_compatibility_assessment(
    pool: &SqlitePool,
    plan: &WorkPlan,
    items: &[WorkItem],
    engine: AssessmentEngine,
    detail: impl Into<String>,
) -> Result<EggplanBackedAssessment, AssessmentAdapterError> {
    let evidence = crate::work_plan_evidence::assemble(pool, items)
        .await
        .unwrap_or_default();
    Ok(EggplanBackedAssessment::legacy(
        assess_work_plan(plan, items, &evidence),
        engine,
        detail,
    ))
}

/// Assess one WorkPlan through the staged Eggplan adoption.
///
/// Flow: terminal history short-circuits to the legacy result; otherwise
/// the canonical workspace root is resolved, unsupported evidence kinds
/// select the explicit legacy engine, the current subject is captured, and
/// the Eggplan bridge decides for the supported Git-backed subset. Any
/// fail-closed condition returns `Err` (never completion); legacy engines
/// return the legacy result verbatim with a diagnostic.
pub async fn assess_work_plan_with_eggplan(
    pool: &SqlitePool,
    plan: &WorkPlan,
    items: &[WorkItem],
) -> Result<EggplanBackedAssessment, AssessmentAdapterError> {
    if matches!(
        plan.status,
        WorkPlanStatus::Completed | WorkPlanStatus::Cancelled
    ) {
        return legacy_compatibility_assessment(
            pool,
            plan,
            items,
            AssessmentEngine::TerminalHistory,
            "terminal_history_passthrough: lifecycle history is not live-re-assessed",
        )
        .await;
    }
    let workspace = resolve_assessment_workspace(pool, plan).await?;
    let Some(workspace) = workspace else {
        return legacy_compatibility_assessment(
            pool,
            plan,
            items,
            AssessmentEngine::LegacyNoWorkspaceContext,
            "no_workspace_context: no workspace identity bound to the session",
        )
        .await;
    };
    if items.iter().any(|item| {
        item.evidence.iter().any(|evidence| {
            matches!(
                evidence.kind,
                WorkEvidenceKind::Artifact | WorkEvidenceKind::Commit
            )
        })
    }) {
        return legacy_compatibility_assessment(
            pool,
            plan,
            items,
            AssessmentEngine::LegacyUnsupportedEvidence,
            "unsupported_evidence_kind: artifact/commit authority is out of scope for M002",
        )
        .await;
    }
    let current_revision = match capture_assessment_subject(&workspace).await? {
        CurrentSubject::Git(revision) => revision,
        CurrentSubject::NonGit => {
            return legacy_compatibility_assessment(
                pool,
                plan,
                items,
                AssessmentEngine::LegacyNonGit,
                "non_git_workspace: current capture positively established no git subject",
            )
            .await;
        }
    };
    let current = match eggplan_subject(&current_revision) {
        Ok(subject) => subject,
        Err(detail) => {
            // The captured subject cannot enter the bridge input contract.
            // Same compatibility class as a rejected snapshot mapping.
            tracing::warn!(
                plan_id = %plan.id.as_str(),
                detail = %detail,
                "eggplan current-subject mapping rejected; using legacy compatibility assessment"
            );
            return legacy_compatibility_assessment(
                pool,
                plan,
                items,
                AssessmentEngine::LegacyUnsupportedEvidence,
                format!("current_subject_rejected: {detail}"),
            )
            .await;
        }
    };
    let (bridge, resolved) = match assess_snapshot_with_subject(pool, plan, items, &current).await {
        Ok(output) => output,
        Err(AssessmentAdapterError::BridgeRejected(detail)) => {
            // The plan shape cannot enter the pinned bridge contract
            // (empty/oversized item set, overlong IDs, unmappable input).
            // The legacy result is byte-identical to pre-M002 behavior, so
            // this compatibility route carries zero permissiveness delta.
            tracing::warn!(
                plan_id = %plan.id.as_str(),
                detail = %detail,
                "eggplan snapshot mapping rejected; using legacy compatibility assessment"
            );
            return legacy_compatibility_assessment(
                pool,
                plan,
                items,
                AssessmentEngine::LegacyUnsupportedEvidence,
                format!("snapshot_mapping_rejected: {detail}"),
            )
            .await;
        }
        Err(error) => return Err(error),
    };
    let assessment = project_bridge_assessment(plan, items, &resolved, &bridge);
    let detail = format!(
        "engine=eggplan_git family={} reasons={} mapping={:.16}",
        bridge.completion_family.as_str(),
        bridge.reason_codes.join(","),
        bridge.manifest.content_digest,
    );
    Ok(EggplanBackedAssessment {
        assessment,
        engine: AssessmentEngine::EggplanGit,
        eggplan_completion_family: Some(bridge.completion_family.as_str().to_string()),
        eggplan_reason_codes: bridge.reason_codes,
        mapping_digest: Some(bridge.manifest.content_digest),
        subject: Some(current_revision),
        engine_detail: bounded_detail_n(detail, MAX_ENGINE_DETAIL_CHARS),
    })
}

// ── §4/§13: completion-transition revalidation ──────────────────────────────
//
// Bounded revalidation, not a filesystem transaction: a read-only display
// may report the S1 assessment, but the status transition that treats the
// assessment as allowing completion must re-capture S2 immediately before
// the CAS and require S2 == S1. Drift, capture failure, or an unresolvable
// root blocks completion and preserves plan state; no current subject is
// ever written into historical evidence.

/// Stable `subject_changed_before_completion` diagnostic for logs/tests.
pub const SUBJECT_CHANGED_DIAGNOSTIC: &str = "subject_changed_before_completion";

/// Re-capture the current subject and require equality with the assessed
/// S1. Returns `Ok(true)` only when S2 == S1 across repository identity,
/// revision, state, and dirty digest.
pub async fn revalidate_subject_for_completion(
    pool: &SqlitePool,
    plan: &WorkPlan,
    assessed: &ExecutionSubjectRevision,
) -> Result<bool, AssessmentAdapterError> {
    let workspace = resolve_assessment_workspace(pool, plan).await?;
    let Some(workspace) = workspace else {
        return Err(AssessmentAdapterError::NoWorkspaceContext(
            "workspace context lost between assessment and completion".to_string(),
        ));
    };
    let current = match capture_assessment_subject(&workspace).await? {
        CurrentSubject::Git(revision) => revision,
        CurrentSubject::NonGit => {
            return Err(AssessmentAdapterError::CurrentCaptureFailed(
                "workspace is not git-backed at completion revalidation".to_string(),
            ));
        }
    };
    Ok(current.repository_identity == assessed.repository_identity
        && current.revision == assessed.revision
        && current.state == assessed.state
        && current.dirty_digest == assessed.dirty_digest)
}

// ── WP2 unit tests: spec canonicalization goldens ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        IdempotencyClass, JobKind, JobPriority, JobSource, JobState, ResourceRequest, RetryPolicy,
    };
    use codegg_core::workspace::WorkspaceId;
    pub(crate) fn job_with_payload(payload: JobPayload) -> JobRecord {
        JobRecord {
            job_id: JobId::new_unchecked("job-test-1".to_string()),
            workspace_id: WorkspaceId::new_unchecked("ws-test"),
            session_id: Some("sess-test".to_string()),
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload,
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            state: JobState::Completed,
            current_attempt_id: None,
            attempt_count: 1,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: vec![],
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: Default::default(),
            labels: HashMap::new(),
        }
    }

    fn test_job() -> JobRecord {
        job_with_payload(JobPayload::Test {
            command: "cargo test --lib".to_string(),
            argv: vec!["cargo".to_string(), "test".to_string(), "--lib".to_string()],
            cwd: Some("crates/codegg-core".to_string()),
            scope: Some("lib".to_string()),
            parent_run_id: None,
        })
    }

    #[test]
    fn test_spec_digest_is_deterministic_and_pinned() {
        let first = verification_digest_for_test(&test_job()).unwrap();
        let second = verification_digest_for_test(&test_job()).unwrap();
        assert_eq!(first, second);
        assert!(first.as_str().starts_with("sha256:"));
        // Golden pin: any canonicalization drift changes this value.
        assert_eq!(
            first.as_str(),
            "sha256:8e6db4aa76179d6b629f74a0ef2908d80c757604dc048d191564ee0811616de7"
        );
    }

    #[test]
    fn test_spec_semantic_change_changes_digest() {
        let base = verification_digest_for_test(&test_job()).unwrap();
        let mut changed = test_job();
        if let JobPayload::Test { argv, .. } = &mut changed.payload {
            argv.push("--release".to_string());
        }
        assert_ne!(verification_digest_for_test(&changed).unwrap(), base);
        // Display-only command text is not verification identity.
        let mut display_only = test_job();
        if let JobPayload::Test { command, .. } = &mut display_only.payload {
            *command = "a completely different display string".to_string();
        }
        assert_eq!(verification_digest_for_test(&display_only).unwrap(), base);
    }

    #[test]
    fn test_spec_empty_argv_fails_closed() {
        let mut job = test_job();
        if let JobPayload::Test { argv, .. } = &mut job.payload {
            argv.clear();
        }
        assert!(verification_digest_for_test(&job).is_err());
    }

    #[test]
    fn shell_without_argv_fails_closed() {
        let job = job_with_payload(JobPayload::Shell {
            command: "cargo test".to_string(),
            argv: None,
            cwd: None,
        });
        assert!(verification_digest_for_scheduler_job(&job).is_err());
    }

    #[test]
    fn python_without_source_identity_fails_closed() {
        let job = job_with_payload(JobPayload::Python {
            script_path: "scripts/analysis.py".to_string(),
            args: vec![],
            mode: "analyze".to_string(),
            source: None,
            source_hash: None,
            cwd: None,
            timeout_secs: None,
        });
        assert!(verification_digest_for_scheduler_job(&job).is_err());
    }

    #[test]
    fn legacy_subagent_stays_unavailable() {
        let job = job_with_payload(JobPayload::Subagent {
            prompt: "do work".to_string(),
            agent: "agent".to_string(),
            model: None,
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            max_tool_calls: None,
        });
        assert!(verification_digest_for_delegated_run(&job).is_err());
    }

    #[test]
    fn ref_id_alone_cannot_create_digest() {
        // Payloads without execution semantics (AgentTurn, Research, ...)
        // carry no verifiable specification.
        for payload in [
            JobPayload::AgentTurn {
                prompt: "p".to_string(),
                agent: "a".to_string(),
                model: None,
                submission_key: None,
            },
            JobPayload::Research {
                query: "q".to_string(),
                max_depth: None,
            },
            JobPayload::Maintenance {
                task: "t".to_string(),
            },
        ] {
            let job = job_with_payload(payload);
            assert!(verification_digest_for_test(&job).is_err());
            assert!(verification_digest_for_scheduler_job(&job).is_err());
            assert!(verification_digest_for_delegated_run(&job).is_err());
        }
    }

    #[test]
    fn spec_ignores_identity_and_labels() {
        // Job/ref/attempt IDs, labels, session, and timestamps never enter
        // verification identity: only execution semantics do.
        let base = verification_digest_for_test(&test_job()).unwrap();
        let mut relabeled = test_job();
        relabeled.job_id = codegg_core::jobs::JobId::new_unchecked("job-other".to_string());
        relabeled.session_id = Some("other-session".to_string());
        relabeled.attempt_count = 7;
        relabeled
            .labels
            .insert("workspace_root".to_string(), "/tmp/other".to_string());
        relabeled.created_at = chrono::Utc::now();
        assert_eq!(verification_digest_for_test(&relabeled).unwrap(), base);
    }

    #[test]
    fn spec_sensitive_to_cwd_scope_target_timeout() {
        let base = verification_digest_for_test(&test_job()).unwrap();
        let mut cwd = test_job();
        if let JobPayload::Test { cwd: c, .. } = &mut cwd.payload {
            *c = Some("other/dir".to_string());
        }
        assert_ne!(verification_digest_for_test(&cwd).unwrap(), base);
        let mut scope = test_job();
        if let JobPayload::Test { scope: s, .. } = &mut scope.payload {
            *s = Some("integration".to_string());
        }
        assert_ne!(verification_digest_for_test(&scope).unwrap(), base);
        let mut target = test_job();
        target.target = codegg_core::jobs::ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        };
        let remote = verification_digest_for_test(&target).unwrap();
        assert_ne!(remote, base);
        // The transient node address is not verification identity: two
        // nodes share the remote digest.
        let mut other_node = test_job();
        other_node.target = codegg_core::jobs::ExecutionTarget::EggworkNode {
            node_id: "node-2".to_string(),
        };
        assert_eq!(verification_digest_for_test(&other_node).unwrap(), remote);
        let mut timeout = test_job();
        timeout.timeout = Some(std::time::Duration::from_secs(300));
        assert_ne!(verification_digest_for_test(&timeout).unwrap(), base);
    }

    #[test]
    fn delegated_spec_covers_policy_and_identity() {
        use codegg_core::identity::{AgentRunId, AgentTaskId};
        let base_payload = JobPayload::SubagentRun {
            prompt: "summarize the diff".to_string(),
            agent: "reviewer".to_string(),
            model: Some("model-x".to_string()),
            parent_id: None,
            denied_tools: vec!["b".to_string(), "a".to_string()],
            allowed_paths: vec!["/repo".to_string()],
            max_tool_calls: Some(25),
            task_id: AgentTaskId::new(),
            run_id: AgentRunId::new(),
            delegation_key: "delegation-1".to_string(),
            base_commit: None,
        };
        let base =
            verification_digest_for_delegated_run(&job_with_payload(base_payload.clone())).unwrap();
        // Policy order is canonicalized: order alone never changes identity.
        let mut reordered = job_with_payload(base_payload.clone());
        if let JobPayload::SubagentRun { denied_tools, .. } = &mut reordered.payload {
            denied_tools.reverse();
        }
        assert_eq!(
            verification_digest_for_delegated_run(&reordered).unwrap(),
            base
        );
        // Prompt content participates only as a digest, but it participates.
        let mut prompt = job_with_payload(base_payload.clone());
        if let JobPayload::SubagentRun { prompt: p, .. } = &mut prompt.payload {
            *p = "summarize the other diff".to_string();
        }
        assert_ne!(
            verification_digest_for_delegated_run(&prompt).unwrap(),
            base
        );
        let mut policy = job_with_payload(base_payload);
        if let JobPayload::SubagentRun { max_tool_calls, .. } = &mut policy.payload {
            *max_tool_calls = Some(50);
        }
        assert_ne!(
            verification_digest_for_delegated_run(&policy).unwrap(),
            base
        );
    }

    #[test]
    fn python_spec_requires_durable_source_and_covers_mode() {
        let payload = |source: Option<String>, hash: Option<String>| JobPayload::Python {
            script_path: "scripts/check.py".to_string(),
            args: vec!["--strict".to_string()],
            mode: "verify".to_string(),
            source,
            source_hash: hash,
            cwd: None,
            timeout_secs: Some(120),
        };
        let from_source = verification_digest_for_scheduler_job(&job_with_payload(payload(
            Some("print('hi')".to_string()),
            None,
        )))
        .unwrap();
        // An explicit durable hash for identical content matches the
        // content-derived digest.
        let content_hash = format!("{:x}", Sha256::digest("print('hi')".as_bytes()));
        let from_hash = verification_digest_for_scheduler_job(&job_with_payload(payload(
            None,
            Some(content_hash),
        )))
        .unwrap();
        assert_eq!(from_source, from_hash);
        let mut mode = job_with_payload(payload(Some("print('hi')".to_string()), None));
        if let JobPayload::Python { mode: m, .. } = &mut mode.payload {
            *m = "analyze".to_string();
        }
        assert_ne!(
            verification_digest_for_scheduler_job(&mode).unwrap(),
            from_source
        );
    }

    #[test]
    fn dirty_digest_translation_matches_across_namespaces() {
        use codegg_core::jobs::{
            ExecutionSubjectKind, ExecutionSubjectProvenance, ExecutionSubjectRevision,
        };
        // M001 persists bare hex; Eggplan requires the sha256: prefix. The
        // adapter translates without touching the durable shape.
        let durable = ExecutionSubjectRevision {
            schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
            subject_kind: ExecutionSubjectKind::Git,
            repository_identity: "codegg-workspace:ws-test".to_string(),
            revision: "d".repeat(40),
            state: ExecutionSubjectState::Dirty,
            dirty_digest: Some("e".repeat(64)),
        };
        let current = eggplan_subject(&durable).unwrap();
        assert_eq!(
            current.dirty_digest.as_deref(),
            Some(format!("sha256:{}", "e".repeat(64)).as_str())
        );
        let provenance = ExecutionSubjectProvenance {
            schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
            captured: Some(durable.clone()),
            sealed: Some(durable),
            disposition: ExecutionSubjectDisposition::Stable,
            seal_kind: codegg_core::jobs::ExecutionSubjectSealKind::LiveExecutionEnd,
            unavailable_reason: None,
            materialization: None,
        };
        assert!(sealed_matches_current(&provenance, &current));
        // A different digest never matches, in either namespace.
        let mut other = current.clone();
        other.dirty_digest = Some(format!("sha256:{}", "f".repeat(64)));
        assert!(!sealed_matches_current(&provenance, &other));
    }

    #[test]
    fn engine_labels_are_stable() {
        assert_eq!(AssessmentEngine::EggplanGit.as_str(), "eggplan_git");
        assert_eq!(AssessmentEngine::LegacyNonGit.as_str(), "legacy_non_git");
        assert_eq!(
            AssessmentEngine::LegacyUnsupportedEvidence.as_str(),
            "legacy_unsupported_evidence"
        );
        assert_eq!(
            AssessmentEngine::LegacyNoWorkspaceContext.as_str(),
            "legacy_no_workspace_context"
        );
        assert_eq!(
            AssessmentEngine::TerminalHistory.as_str(),
            "terminal_history"
        );
    }

    #[test]
    fn subject_changed_diagnostic_is_stable() {
        assert_eq!(
            SUBJECT_CHANGED_DIAGNOSTIC,
            "subject_changed_before_completion"
        );
    }

    #[test]
    fn provider_identity_is_fixed() {
        assert_eq!(EGGPLAN_PROVIDER_ID, "epp_codegg_host");
        let registry = codegg_host_providers().unwrap();
        let id = EvidenceProviderId::new(EGGPLAN_PROVIDER_ID.to_string()).unwrap();
        let descriptor = registry.get(&id).unwrap();
        assert!(descriptor
            .allowed_kinds()
            .contains(&EggplanEvidenceKind::Test));
        assert!(descriptor
            .allowed_kinds()
            .contains(&EggplanEvidenceKind::Command));
        assert!(descriptor
            .allowed_kinds()
            .contains(&EggplanEvidenceKind::DelegatedRun));
        assert_eq!(descriptor.allowed_kinds().len(), 3);
    }

    #[test]
    fn snapshot_rejects_empty_item_set() {
        use codegg_core::work_plan::{WorkPlanId, WorkPlanStatus};
        let plan = WorkPlan {
            id: WorkPlanId("wp_test".to_string()),
            revision: 0,
            session_id: "sess-test".to_string(),
            project_id: "proj-test".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "objective".to_string(),
            objective_digest: "sha256:test".to_string(),
            origin_provenance: "turn:test".to_string(),
            status: WorkPlanStatus::Active,
            current_phase: None,
            current_item_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        };
        let resolved = crate::work_plan_evidence::ResolvedWorkEvidenceSnapshot::default();
        let bound = HashMap::new();
        let rejected = build_plan_snapshot(&plan, &[], &resolved, &bound).unwrap_err();
        assert_eq!(rejected.reason, "empty_item_set");
    }
}
