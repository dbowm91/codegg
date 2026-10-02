//! Typed causal contracts, host-owned state projection, the frozen
//! stateful benchmark, and the offline causal-admissibility frontier for the
//! tool-selection advisor causal-frontier experiment (M001/M002).
//!
//! This module is the smallest trustworthy substrate for causal tool-menu
//! experiments. It changes **no runtime disclosure behavior**: nothing here
//! filters provider definitions, alters broker authorization, or widens
//! execution authority. The causal frontier is a visibility-only planning
//! layer evaluated offline in M002 against the frozen benchmark preregistered
//! in M001.
//!
//! # Closed ontology
//!
//! [`CausalStateFact`] is a closed enum of host-derivable state facts and
//! [`CausalOutcome`] is a closed enum of coarse causal outcomes. No fact may
//! be created from an embedding/LLM classifier or hidden reasoning. Every
//! fact has exactly one canonical host source:
//!
//! | Fact | Canonical host source |
//! |---|---|
//! | `active_goal` | `GoalStore::active_for_session` / `Goal::is_active` |
//! | `active_work_plan` | `WorkPlanStore` non-terminal plan |
//! | `actionable_work_item` | `WorkItem` with status `Actionable` |
//! | `in_progress_work_item` | `WorkItem` with status `InProgress` |
//! | `blocked_work_item` | `WorkItem` with status `Blocked` |
//! | `artifact_handle_available` | `ContextLedgerState::artifact_handles` (bounded) |
//! | `touched_files_available` | `ContextLedgerState::touched_files` |
//! | `test_evidence_available` | `ContextLedgerState::test_results` |
//! | `failed_test_evidence` | structured test execution status (never prose parsing) |
//! | `unresolved_error` | `ContextLedgerState::unresolved_errors` |
//! | `security_finding` | turn-local recent security findings |
//! | `lsp_preview_available` | turn-local `PreviewArtifactRegistry` non-empty |
//! | `context_read_available` | `context_read` registered in this registry |
//! | `unmet_test_acceptance` | `WorkAcceptance` `Unmet` + `TestJob` evidence ref |
//! | `unmet_commit_acceptance` | `WorkAcceptance` `Unmet` + `Commit` evidence ref |
//! | `unmet_artifact_acceptance` | `WorkAcceptance` `Unmet` + `Artifact` evidence ref |
//! | `unmet_delegated_run_acceptance` | `WorkAcceptance` `Unmet` + `DelegatedRun` evidence ref |
//!
//! # State snapshot discipline
//!
//! [`CausalStateSnapshot`] stores booleans, capped counts, and bounded typed
//! ids only. It never copies raw tool output, prompts, file content, secrets,
//! or transcript history. Counts are capped and ids truncated so the
//! snapshot is bounded by construction.
//!
//! # Contract integrity
//!
//! [`ToolCausalContract`] is planning metadata, not execution policy. A
//! missing contract means "not causally classifiable", never "forbidden".
//! Each bound fingerprint ties together the canonical tool name, the
//! implementation id/version, the input-schema fingerprint, the causal
//! ontology version, and the contract payload (see [`ContractBinding`]).

use codegg_core::goal::model::Goal;
use codegg_core::work_plan::model::{
    WorkAcceptanceDisposition, WorkEvidenceKind, WorkItem, WorkItemStatus, WorkPlan, WorkPlanStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

// ─── Versions ─────────────────────────────────────────────────────────────

/// Causal fact/outcome ontology version. Bumped only by a new experiment
/// version; frozen for M001/M002 by the preregistration receipt.
pub const CAUSAL_ONTOLOGY_VERSION: u16 = 1;
/// Schema version for [`ToolCausalContract`].
pub const CAUSAL_CONTRACT_SCHEMA_VERSION: u16 = 1;
/// Schema version for [`CausalStateSnapshot`].
pub const CAUSAL_SNAPSHOT_SCHEMA_VERSION: u16 = 1;
/// Schema version for benchmark cases and the M001 preregistration.
pub const CAUSAL_BENCHMARK_SCHEMA_VERSION: u16 = 1;

/// Maximum characters retained for any typed id carried in a snapshot.
pub const MAX_CAUSAL_ID_CHARS: usize = 256;
/// Maximum characters for provenance source/rationale strings.
pub const MAX_CAUSAL_PROVENANCE_CHARS: usize = 512;
/// Cap applied to every count stored in a snapshot (bounded by construction).
pub const MAX_CAUSAL_COUNT: u32 = 9_999;

/// Stable host-revision key for the active goal revision.
pub const HOST_REVISION_GOAL: &str = "goal";
/// Stable host-revision key for the active work-plan revision.
pub const HOST_REVISION_WORK_PLAN: &str = "work_plan";

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn bound_count(count: usize) -> u32 {
    std::cmp::min(count as u64, u64::from(MAX_CAUSAL_COUNT)) as u32
}

fn bound_id(id: &str) -> String {
    if id.len() <= MAX_CAUSAL_ID_CHARS {
        id.to_string()
    } else {
        id[..MAX_CAUSAL_ID_CHARS].to_string()
    }
}

// ─── State facts ──────────────────────────────────────────────────────────

/// Closed vocabulary of host-derivable causal state facts.
///
/// Every variant has one canonical host source (see module docs). Unknown
/// wire values are rejected at parse time: there is no catch-all variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalStateFact {
    ActiveGoal,
    ActiveWorkPlan,
    ActionableWorkItem,
    InProgressWorkItem,
    BlockedWorkItem,
    ArtifactHandleAvailable,
    TouchedFilesAvailable,
    TestEvidenceAvailable,
    FailedTestEvidence,
    UnresolvedError,
    SecurityFinding,
    LspPreviewAvailable,
    ContextReadAvailable,
    UnmetTestAcceptance,
    UnmetCommitAcceptance,
    UnmetArtifactAcceptance,
    UnmetDelegatedRunAcceptance,
}

impl CausalStateFact {
    /// Stable wire string for this fact.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ActiveGoal => "active_goal",
            Self::ActiveWorkPlan => "active_work_plan",
            Self::ActionableWorkItem => "actionable_work_item",
            Self::InProgressWorkItem => "in_progress_work_item",
            Self::BlockedWorkItem => "blocked_work_item",
            Self::ArtifactHandleAvailable => "artifact_handle_available",
            Self::TouchedFilesAvailable => "touched_files_available",
            Self::TestEvidenceAvailable => "test_evidence_available",
            Self::FailedTestEvidence => "failed_test_evidence",
            Self::UnresolvedError => "unresolved_error",
            Self::SecurityFinding => "security_finding",
            Self::LspPreviewAvailable => "lsp_preview_available",
            Self::ContextReadAvailable => "context_read_available",
            Self::UnmetTestAcceptance => "unmet_test_acceptance",
            Self::UnmetCommitAcceptance => "unmet_commit_acceptance",
            Self::UnmetArtifactAcceptance => "unmet_artifact_acceptance",
            Self::UnmetDelegatedRunAcceptance => "unmet_delegated_run_acceptance",
        }
    }

    /// Canonical host source for this fact. Used by documentation and by
    /// the per-fact host-source tests.
    pub const fn canonical_source(self) -> &'static str {
        match self {
            Self::ActiveGoal => "GoalStore::active_for_session / Goal::is_active",
            Self::ActiveWorkPlan => "WorkPlanStore non-terminal plan",
            Self::ActionableWorkItem => "WorkItem status Actionable",
            Self::InProgressWorkItem => "WorkItem status InProgress",
            Self::BlockedWorkItem => "WorkItem status Blocked",
            Self::ArtifactHandleAvailable => "ContextLedgerState::artifact_handles",
            Self::TouchedFilesAvailable => "ContextLedgerState::touched_files",
            Self::TestEvidenceAvailable => "ContextLedgerState::test_results",
            Self::FailedTestEvidence => "structured test execution status",
            Self::UnresolvedError => "ContextLedgerState::unresolved_errors",
            Self::SecurityFinding => "turn-local recent security findings",
            Self::LspPreviewAvailable => "turn-local PreviewArtifactRegistry",
            Self::ContextReadAvailable => "context_read tool registration",
            Self::UnmetTestAcceptance => "WorkAcceptance Unmet + TestJob evidence ref",
            Self::UnmetCommitAcceptance => "WorkAcceptance Unmet + Commit evidence ref",
            Self::UnmetArtifactAcceptance => "WorkAcceptance Unmet + Artifact evidence ref",
            Self::UnmetDelegatedRunAcceptance => "WorkAcceptance Unmet + DelegatedRun evidence ref",
        }
    }

    /// Every fact in the closed ontology, in canonical order.
    pub const fn all() -> [Self; 17] {
        [
            Self::ActiveGoal,
            Self::ActiveWorkPlan,
            Self::ActionableWorkItem,
            Self::InProgressWorkItem,
            Self::BlockedWorkItem,
            Self::ArtifactHandleAvailable,
            Self::TouchedFilesAvailable,
            Self::TestEvidenceAvailable,
            Self::FailedTestEvidence,
            Self::UnresolvedError,
            Self::SecurityFinding,
            Self::LspPreviewAvailable,
            Self::ContextReadAvailable,
            Self::UnmetTestAcceptance,
            Self::UnmetCommitAcceptance,
            Self::UnmetArtifactAcceptance,
            Self::UnmetDelegatedRunAcceptance,
        ]
    }
}

// ─── Tool outcomes ────────────────────────────────────────────────────────

/// Closed vocabulary of coarse causal outcomes.
///
/// Planning metadata only: not a claim that every invocation succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalOutcome {
    FilesInspected,
    PathsDiscovered,
    TextMatchesProduced,
    SymbolFactsProduced,
    WorkspaceMutationProduced,
    VerificationEvidenceProduced,
    TestEvidenceProduced,
    GitFactsProduced,
    CommitEvidenceProduced,
    ExternalEvidenceProduced,
    DelegatedRunProduced,
    ArtifactExpanded,
    GoalStateUpdated,
    WorkPlanStateUpdated,
}

impl CausalOutcome {
    /// Stable wire string for this outcome.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FilesInspected => "files_inspected",
            Self::PathsDiscovered => "paths_discovered",
            Self::TextMatchesProduced => "text_matches_produced",
            Self::SymbolFactsProduced => "symbol_facts_produced",
            Self::WorkspaceMutationProduced => "workspace_mutation_produced",
            Self::VerificationEvidenceProduced => "verification_evidence_produced",
            Self::TestEvidenceProduced => "test_evidence_produced",
            Self::GitFactsProduced => "git_facts_produced",
            Self::CommitEvidenceProduced => "commit_evidence_produced",
            Self::ExternalEvidenceProduced => "external_evidence_produced",
            Self::DelegatedRunProduced => "delegated_run_produced",
            Self::ArtifactExpanded => "artifact_expanded",
            Self::GoalStateUpdated => "goal_state_updated",
            Self::WorkPlanStateUpdated => "work_plan_state_updated",
        }
    }

    /// Every outcome in the closed ontology, in canonical order.
    pub const fn all() -> [Self; 14] {
        [
            Self::FilesInspected,
            Self::PathsDiscovered,
            Self::TextMatchesProduced,
            Self::SymbolFactsProduced,
            Self::WorkspaceMutationProduced,
            Self::VerificationEvidenceProduced,
            Self::TestEvidenceProduced,
            Self::GitFactsProduced,
            Self::CommitEvidenceProduced,
            Self::ExternalEvidenceProduced,
            Self::DelegatedRunProduced,
            Self::ArtifactExpanded,
            Self::GoalStateUpdated,
            Self::WorkPlanStateUpdated,
        ]
    }
}

// ─── Contract ─────────────────────────────────────────────────────────────

/// Why a native causal contract exists and what justifies it.
///
/// Both fields are required and bounded. Native M001 contracts use the
/// source `static:M001-pilot-native`; inferred or LLM-generated contracts
/// are out of scope for this workstream and must never use that source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalContractProvenance {
    pub source: String,
    pub rationale: String,
}

impl CausalContractProvenance {
    pub fn new(source: impl Into<String>, rationale: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            rationale: rationale.into(),
        }
    }

    fn validate(&self) -> Result<(), CausalContractError> {
        if self.source.trim().is_empty() || self.rationale.trim().is_empty() {
            return Err(CausalContractError::EmptyProvenance);
        }
        if self.source.len() > MAX_CAUSAL_PROVENANCE_CHARS
            || self.rationale.len() > MAX_CAUSAL_PROVENANCE_CHARS
        {
            return Err(CausalContractError::ProvenanceTooLong);
        }
        Ok(())
    }
}

/// Failures from causal-contract validation and binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CausalContractError {
    /// Contract uses an unsupported schema version.
    UnsupportedSchemaVersion(u16),
    /// A fact appears in both `requires_all` and `forbids`.
    Contradiction(CausalStateFact),
    /// A `requires_any` group is empty (satisfiable by nothing).
    EmptyRequiresAnyGroup(usize),
    /// Provenance source or rationale is empty.
    EmptyProvenance,
    /// Provenance source or rationale exceeds the bound.
    ProvenanceTooLong,
    /// Binding attempted under an empty tool name.
    EmptyToolName,
}

impl std::fmt::Display for CausalContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion(v) => {
                write!(f, "unsupported causal contract schema version {v}")
            }
            Self::Contradiction(fact) => {
                write!(f, "causal contract contradiction on fact {}", fact.as_str())
            }
            Self::EmptyRequiresAnyGroup(i) => {
                write!(f, "causal contract requires_any group {i} is empty")
            }
            Self::EmptyProvenance => {
                write!(f, "causal contract provenance must not be empty")
            }
            Self::ProvenanceTooLong => {
                write!(f, "causal contract provenance exceeds bound")
            }
            Self::EmptyToolName => {
                write!(f, "causal contract tool name must not be empty")
            }
        }
    }
}

impl std::error::Error for CausalContractError {}

/// Planning-only causal contract for one tool.
///
/// The boolean language is intentionally small: conjunctive `requires_all`,
/// disjunctive `requires_any` groups, and a `forbids` set. No arbitrary
/// expression DSL in M001.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCausalContract {
    pub schema_version: u16,
    pub requires_all: BTreeSet<CausalStateFact>,
    pub requires_any: Vec<BTreeSet<CausalStateFact>>,
    pub forbids: BTreeSet<CausalStateFact>,
    pub produces: BTreeSet<CausalOutcome>,
    pub provenance: CausalContractProvenance,
}

impl ToolCausalContract {
    /// Validate internal consistency. Unknown enum values are rejected by
    /// serde at parse time before this runs.
    pub fn validate(&self) -> Result<(), CausalContractError> {
        if self.schema_version != CAUSAL_CONTRACT_SCHEMA_VERSION {
            return Err(CausalContractError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        for fact in &self.requires_all {
            if self.forbids.contains(fact) {
                return Err(CausalContractError::Contradiction(*fact));
            }
        }
        for (index, group) in self.requires_any.iter().enumerate() {
            if group.is_empty() {
                return Err(CausalContractError::EmptyRequiresAnyGroup(index));
            }
            for fact in group {
                if self.forbids.contains(fact) {
                    return Err(CausalContractError::Contradiction(*fact));
                }
            }
        }
        self.provenance.validate()?;
        Ok(())
    }

    /// Whether this contract's preconditions hold under `facts`.
    ///
    /// A tool with empty preconditions is always admissible; admissibility
    /// is a visibility recommendation, never call authorization.
    pub fn is_satisfied_by(&self, facts: &BTreeSet<CausalStateFact>) -> bool {
        if !self.requires_all.is_subset(facts) {
            return false;
        }
        if self
            .requires_any
            .iter()
            .any(|group| group.is_disjoint(facts))
        {
            return false;
        }
        if !self.forbids.is_disjoint(facts) {
            return false;
        }
        true
    }

    /// `requires_any` groups in canonical order (each group is already
    /// sorted by `BTreeSet`; groups sort lexicographically).
    fn canonical_requires_any(&self) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = self
            .requires_any
            .iter()
            .map(|group| group.iter().map(|fact| fact.as_str().to_string()).collect())
            .collect();
        groups.sort();
        groups
    }

    /// Canonical JSON payload of the contract. Deterministic: fact and
    /// outcome sets serialize sorted, and `requires_any` group order is
    /// normalized, so equivalent contracts always serialize identically.
    pub fn canonical_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": self.schema_version,
            "ontology_version": CAUSAL_ONTOLOGY_VERSION,
            "requires_all": self.requires_all.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
            "requires_any": self.canonical_requires_any(),
            "forbids": self.forbids.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
            "produces": self.produces.iter().map(|outcome| outcome.as_str()).collect::<Vec<_>>(),
            "provenance": {
                "source": self.provenance.source,
                "rationale": self.provenance.rationale,
            },
        })
    }

    /// Fingerprint binding the canonical tool name, the implementation
    /// id/version, the input-schema fingerprint, the causal ontology
    /// version, and the contract payload.
    pub fn fingerprint(&self, binding: &ContractBinding) -> String {
        let payload = serde_json::json!({
            "tool_name": binding.tool_name,
            "implementation_id": binding.implementation_id,
            "implementation_version": binding.implementation_version,
            "input_schema_fingerprint": binding.input_schema_fingerprint,
            "contract": self.canonical_json(),
        });
        sha256_hex(payload.to_string().as_bytes())
    }
}

/// Identity elements bound into a causal-contract fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractBinding<'a> {
    pub tool_name: &'a str,
    pub implementation_id: &'a str,
    pub implementation_version: &'a str,
    pub input_schema_fingerprint: &'a str,
}

/// A causal contract bound to its canonical tool identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundToolCausalContract {
    pub tool_name: String,
    pub contract: ToolCausalContract,
    pub fingerprint: String,
}

/// Bind a contract to a canonical tool name.
///
/// The fingerprint differs per tool name, so a contract silently reused
/// under another tool's identity is detectable. Returns an error on empty
/// names and invalid contracts.
pub fn bind_causal_contract(
    tool_name: &str,
    contract: &ToolCausalContract,
    implementation_id: &str,
    implementation_version: &str,
    input_schema: &serde_json::Value,
) -> Result<BoundToolCausalContract, CausalContractError> {
    if tool_name.is_empty() {
        return Err(CausalContractError::EmptyToolName);
    }
    contract.validate()?;
    let schema_fp = sha256_hex(canonical_json_bytes(input_schema).as_slice());
    let binding = ContractBinding {
        tool_name,
        implementation_id,
        implementation_version,
        input_schema_fingerprint: &schema_fp,
    };
    let fingerprint = contract.fingerprint(&binding);
    Ok(BoundToolCausalContract {
        tool_name: tool_name.to_string(),
        contract: contract.clone(),
        fingerprint,
    })
}

/// Canonical bytes for an arbitrary JSON value: object keys sort
/// recursively so fingerprints are stable regardless of map order.
fn canonical_json_bytes(value: &serde_json::Value) -> Vec<u8> {
    fn canonical(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut sorted = BTreeMap::new();
                for (key, val) in map {
                    sorted.insert(key.clone(), canonical(val));
                }
                serde_json::Value::Object(sorted.into_iter().collect())
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(canonical).collect())
            }
            other => other.clone(),
        }
    }
    canonical(value).to_string().into_bytes()
}

// ─── Native pilot contracts ───────────────────────────────────────────────

/// Canonical native tools carrying a static M001 causal contract, sorted.
pub const NATIVE_PILOT_TOOLS: &[&str] = &[
    "commit",
    "context_read",
    "edit",
    "git",
    "glob",
    "goal_get",
    "goal_update_progress",
    "grep",
    "lsp",
    "lsp_preview_apply",
    "read",
    "research",
    "task",
    "test",
    "verify",
    "webfetch",
    "websearch",
    "work_plan_get",
    "work_plan_update_item",
    "write",
];

fn pilot_contract(
    requires_all: &[CausalStateFact],
    requires_any: &[&[CausalStateFact]],
    forbids: &[CausalStateFact],
    produces: &[CausalOutcome],
    rationale: &str,
) -> ToolCausalContract {
    ToolCausalContract {
        schema_version: CAUSAL_CONTRACT_SCHEMA_VERSION,
        requires_all: requires_all.iter().copied().collect(),
        requires_any: requires_any
            .iter()
            .map(|group| group.iter().copied().collect())
            .collect(),
        forbids: forbids.iter().copied().collect(),
        produces: produces.iter().copied().collect(),
        provenance: CausalContractProvenance::new("static:M001-pilot-native", rationale),
    }
}

/// Static native causal contract for one canonical tool name.
///
/// Returns `None` for tools without trusted static metadata — including all
/// multiplexed tools with operation-dependent effects (e.g. `bash`),
/// deferred/specialist tools outside the pilot, and every MCP/plugin or
/// external tool. A missing contract means "not causally classifiable",
/// never "forbidden".
///
/// Generic inspection tools carry empty preconditions with outcome metadata
/// only. Stateful tools carry the minimal statically justifiable
/// preconditions; multiplexed tools use a conservative union or remain
/// uncontracted rather than pretending false precision.
pub fn native_causal_contract(tool_name: &str) -> Option<ToolCausalContract> {
    use CausalOutcome::{
        ArtifactExpanded, CommitEvidenceProduced, DelegatedRunProduced, ExternalEvidenceProduced,
        FilesInspected, GitFactsProduced, GoalStateUpdated, PathsDiscovered, SymbolFactsProduced,
        TestEvidenceProduced, TextMatchesProduced, VerificationEvidenceProduced,
        WorkPlanStateUpdated, WorkspaceMutationProduced,
    };
    use CausalStateFact::{
        ActionableWorkItem, ActiveGoal, ActiveWorkPlan, ArtifactHandleAvailable, BlockedWorkItem,
        InProgressWorkItem, LspPreviewAvailable, UnmetCommitAcceptance, UnresolvedError,
    };
    match tool_name {
        // Artifact expansion is only meaningful once an artifact handle exists.
        "context_read" => Some(pilot_contract(
            &[ArtifactHandleAvailable],
            &[],
            &[],
            &[ArtifactExpanded],
            "context_read expands a ctx:// artifact handle; without a bounded handle \
             there is nothing to expand",
        )),
        // Goal reads/updates are premature without an active goal.
        "goal_get" => Some(pilot_contract(
            &[ActiveGoal],
            &[],
            &[],
            &[GoalStateUpdated],
            "goal_get reports active-goal state; without an active goal it returns \
             empty state",
        )),
        "goal_update_progress" => Some(pilot_contract(
            &[ActiveGoal],
            &[],
            &[],
            &[GoalStateUpdated],
            "goal_update_progress mutates active-goal progress; requires an active goal",
        )),
        // Work-plan reads/updates are premature without a non-terminal plan.
        "work_plan_get" => Some(pilot_contract(
            &[ActiveWorkPlan],
            &[],
            &[],
            &[WorkPlanStateUpdated],
            "work_plan_get reports plan/item state; without an active plan it returns \
             empty state",
        )),
        "work_plan_update_item" => Some(pilot_contract(
            &[ActiveWorkPlan],
            &[&[ActionableWorkItem, InProgressWorkItem, BlockedWorkItem]],
            &[],
            &[WorkPlanStateUpdated],
            "work_plan_update_item advances one item; requires an active plan with an \
             item in a non-terminal actionable state",
        )),
        // Preview application requires a staged turn-local preview.
        "lsp_preview_apply" => Some(pilot_contract(
            &[LspPreviewAvailable],
            &[],
            &[],
            &[WorkspaceMutationProduced],
            "lsp_preview_apply applies a staged LSP preview; without a preview there \
             is nothing to apply",
        )),
        // Commit evidence satisfies commit acceptance; committing while an
        // unresolved error is recorded is premature.
        "commit" => Some(pilot_contract(
            &[UnmetCommitAcceptance],
            &[],
            &[UnresolvedError],
            &[CommitEvidenceProduced],
            "commit produces commit evidence against unmet commit acceptance; \
             committing with a recorded unresolved error is premature",
        )),
        // Generic tools: empty preconditions, outcome metadata only.
        "read" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[FilesInspected],
            "read inspects file content; statically safe in any host state",
        )),
        "glob" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[PathsDiscovered],
            "glob discovers paths; statically safe in any host state",
        )),
        "grep" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[TextMatchesProduced],
            "grep produces text matches; statically safe in any host state",
        )),
        "lsp" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[SymbolFactsProduced],
            "lsp symbol queries produce symbol facts; statically safe in any host state",
        )),
        "write" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[WorkspaceMutationProduced],
            "write is the ordinary mutation path; gating it on plan state would \
             over-claim static precision",
        )),
        "edit" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[WorkspaceMutationProduced],
            "edit is the ordinary mutation path; gating it on plan state would \
             over-claim static precision",
        )),
        // Multiplexed git read surface: conservative union of git facts.
        "git" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[GitFactsProduced],
            "git native surface is operation-dependent; conservative union reports \
             git facts without claiming per-operation precision",
        )),
        "test" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[TestEvidenceProduced],
            "test always produces test evidence; runnable in any host state",
        )),
        "verify" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[VerificationEvidenceProduced, TestEvidenceProduced],
            "verify always produces verification evidence; runnable in any host state",
        )),
        "task" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[DelegatedRunProduced],
            "task delegation availability is decided upstream by the functional \
             backend; the static contract claims no state precondition",
        )),
        "research" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[ExternalEvidenceProduced],
            "research produces external evidence; statically safe in any host state",
        )),
        "webfetch" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[ExternalEvidenceProduced],
            "webfetch produces external evidence; statically safe in any host state",
        )),
        "websearch" => Some(pilot_contract(
            &[],
            &[],
            &[],
            &[ExternalEvidenceProduced],
            "websearch produces external evidence; statically safe in any host state",
        )),
        _ => None,
    }
}

/// Name-keyed catalog of every native M001 pilot contract.
pub fn causal_catalog() -> BTreeMap<String, ToolCausalContract> {
    NATIVE_PILOT_TOOLS
        .iter()
        .map(|name| {
            (
                (*name).to_string(),
                native_causal_contract(name).expect("pilot tool must have a contract"),
            )
        })
        .collect()
}

/// Fingerprint over the frozen pilot catalog: canonical tool names, the
/// causal ontology version, and every contract payload. Recorded in the
/// M001 preregistration receipt; any contract edit changes it and requires
/// a new experiment version.
pub fn causal_catalog_fingerprint() -> String {
    let catalog = causal_catalog();
    let payload = serde_json::json!({
        "ontology_version": CAUSAL_ONTOLOGY_VERSION,
        "contract_schema_version": CAUSAL_CONTRACT_SCHEMA_VERSION,
        "contracts": catalog
            .iter()
            .map(|(name, contract)| {
                serde_json::json!({ "tool_name": name, "contract": contract.canonical_json() })
            })
            .collect::<Vec<_>>(),
    });
    sha256_hex(payload.to_string().as_bytes())
}

// ─── State snapshot ───────────────────────────────────────────────────────

/// Host-owned inputs for one [`CausalStateSnapshot`].
///
/// Every field is a boolean, a capped count, or a bounded typed id. Raw
/// tool output, prompts, file content, secrets, and transcript history
/// cannot be represented here by construction.
#[derive(Debug, Clone, Default)]
pub struct CausalStateInputs {
    pub has_active_goal: bool,
    pub goal_id: Option<String>,
    pub goal_revision: Option<i64>,
    pub has_active_work_plan: bool,
    pub work_plan_id: Option<String>,
    pub work_plan_revision: Option<i64>,
    pub current_item_id: Option<String>,
    pub has_actionable_item: bool,
    pub has_in_progress_item: bool,
    pub has_blocked_item: bool,
    pub unmet_test_acceptance: bool,
    pub unmet_commit_acceptance: bool,
    pub unmet_artifact_acceptance: bool,
    pub unmet_delegated_run_acceptance: bool,
    pub touched_file_count: usize,
    pub test_evidence_count: usize,
    pub has_failed_tests: bool,
    pub unresolved_error_count: usize,
    pub security_finding_count: usize,
    pub artifact_handle_count: usize,
    pub lsp_preview_available: bool,
    pub context_read_available: bool,
}

impl CausalStateInputs {
    /// Apply goal state from the canonical host source.
    ///
    /// Canonical source: `GoalStore::active_for_session`; only a goal with
    /// [`Goal::is_active`] sets the fact. The id is a bounded typed id;
    /// titles, objectives, and summaries never enter the snapshot.
    pub fn apply_goal(&mut self, goal: Option<&Goal>) {
        match goal {
            Some(goal) if goal.is_active() => {
                self.has_active_goal = true;
                self.goal_id = Some(bound_id(&goal.id));
                self.goal_revision = Some(goal.revision);
            }
            _ => {
                self.has_active_goal = false;
                self.goal_id = None;
                self.goal_revision = None;
            }
        }
    }

    /// Apply work-plan state from the canonical host source.
    ///
    /// Canonical source: `WorkPlanStore`. A non-terminal plan
    /// (`Active` or `Blocked`) sets the plan fact; item-status facts follow
    /// [`WorkItemStatus`]. Acceptance facts require an `Unmet` acceptance
    /// **plus** a typed evidence ref of the matching [`WorkEvidenceKind`]
    /// on the same item — descriptions are never interpreted.
    pub fn apply_work_plan(&mut self, plan: Option<&WorkPlan>, items: &[WorkItem]) {
        match plan {
            Some(plan)
                if matches!(
                    plan.status,
                    WorkPlanStatus::Active | WorkPlanStatus::Blocked
                ) =>
            {
                self.has_active_work_plan = true;
                self.work_plan_id = Some(bound_id(plan.id.as_str()));
                self.work_plan_revision = Some(plan.revision);
                self.current_item_id = plan
                    .current_item_id
                    .as_ref()
                    .map(|id| bound_id(id.as_str()));
            }
            _ => {
                self.has_active_work_plan = false;
                self.work_plan_id = None;
                self.work_plan_revision = None;
                self.current_item_id = None;
            }
        }
        if !self.has_active_work_plan {
            self.has_actionable_item = false;
            self.has_in_progress_item = false;
            self.has_blocked_item = false;
            self.unmet_test_acceptance = false;
            self.unmet_commit_acceptance = false;
            self.unmet_artifact_acceptance = false;
            self.unmet_delegated_run_acceptance = false;
            return;
        }
        self.has_actionable_item = items
            .iter()
            .any(|item| item.status == WorkItemStatus::Actionable);
        self.has_in_progress_item = items
            .iter()
            .any(|item| item.status == WorkItemStatus::InProgress);
        self.has_blocked_item = items
            .iter()
            .any(|item| item.status == WorkItemStatus::Blocked);
        let mut test = false;
        let mut commit = false;
        let mut artifact = false;
        let mut delegated = false;
        for item in items {
            let unmet = item
                .acceptance
                .iter()
                .any(|acceptance| acceptance.disposition == WorkAcceptanceDisposition::Unmet);
            if !unmet {
                continue;
            }
            for evidence in &item.evidence {
                match evidence.kind {
                    WorkEvidenceKind::TestJob => test = true,
                    WorkEvidenceKind::Commit => commit = true,
                    WorkEvidenceKind::Artifact => artifact = true,
                    WorkEvidenceKind::DelegatedRun => delegated = true,
                    WorkEvidenceKind::SchedulerJob | WorkEvidenceKind::AgentRun => {}
                }
            }
        }
        self.unmet_test_acceptance = test;
        self.unmet_commit_acceptance = commit;
        self.unmet_artifact_acceptance = artifact;
        self.unmet_delegated_run_acceptance = delegated;
    }

    /// Apply ledger presence signals from the canonical host source.
    ///
    /// Canonical source: [`crate::agent::context_frame::ContextLedgerState`].
    /// Only presence/counts are recorded; file paths, command strings, test
    /// output, error text, and handle contents never enter the snapshot.
    /// Failed-test state is set separately via [`Self::set_failed_tests`]
    /// from structured execution status, never by parsing result strings.
    pub fn apply_ledger(&mut self, ledger: &crate::agent::context_frame::ContextLedgerState) {
        self.touched_file_count = ledger.touched_files.len();
        self.test_evidence_count = ledger.test_results.len();
        self.unresolved_error_count = ledger.unresolved_errors.len();
        self.artifact_handle_count = ledger.artifact_handles.len();
    }

    /// Record failed-test evidence from structured execution status.
    ///
    /// Canonical source: structured test execution status (non-zero exit /
    /// typed test-job failure). Result strings are never parsed.
    pub fn set_failed_tests(&mut self, failed: bool) {
        self.has_failed_tests = failed;
    }

    /// Record security findings by count. Finding details never enter the
    /// snapshot.
    pub fn set_security_findings(&mut self, count: usize) {
        self.security_finding_count = count;
    }

    /// Apply turn-local LSP preview availability from the canonical host
    /// source: a non-empty [`egglsp::preview_registry::PreviewArtifactRegistry`].
    pub fn apply_preview_registry(
        &mut self,
        registry: &egglsp::preview_registry::PreviewArtifactRegistry,
    ) {
        self.lsp_preview_available = !registry.is_empty();
    }

    /// Record `context_read` availability. Canonical source: whether
    /// `context_read` is registered in the turn's tool registry.
    pub fn set_context_read_available(&mut self, available: bool) {
        self.context_read_available = available;
    }
}

/// Immutable bounded snapshot of host-owned causal state.
///
/// Stores booleans, capped counts, and bounded typed ids only. The
/// fingerprint binds the ontology version, the fact values, the relevant
/// host-state revision identifiers, and the resolved-surface fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalStateSnapshot {
    pub schema_version: u16,
    pub ontology_version: u16,
    pub facts: BTreeSet<CausalStateFact>,
    pub goal_id: Option<String>,
    pub work_plan_id: Option<String>,
    pub current_item_id: Option<String>,
    pub touched_file_count: u32,
    pub test_evidence_count: u32,
    pub unresolved_error_count: u32,
    pub security_finding_count: u32,
    pub artifact_handle_count: u32,
    pub host_revisions: BTreeMap<String, String>,
    pub surface_fingerprint: String,
    pub fingerprint: String,
}

impl CausalStateSnapshot {
    /// Build a snapshot from host-owned inputs and the already-resolved
    /// surface fingerprint. Deterministic: identical inputs always produce
    /// identical snapshots and fingerprints.
    pub fn from_inputs(inputs: &CausalStateInputs, surface_fingerprint: &str) -> Self {
        let mut facts = BTreeSet::new();
        if inputs.has_active_goal {
            facts.insert(CausalStateFact::ActiveGoal);
        }
        if inputs.has_active_work_plan {
            facts.insert(CausalStateFact::ActiveWorkPlan);
        }
        if inputs.has_actionable_item {
            facts.insert(CausalStateFact::ActionableWorkItem);
        }
        if inputs.has_in_progress_item {
            facts.insert(CausalStateFact::InProgressWorkItem);
        }
        if inputs.has_blocked_item {
            facts.insert(CausalStateFact::BlockedWorkItem);
        }
        if inputs.artifact_handle_count > 0 {
            facts.insert(CausalStateFact::ArtifactHandleAvailable);
        }
        if inputs.touched_file_count > 0 {
            facts.insert(CausalStateFact::TouchedFilesAvailable);
        }
        if inputs.test_evidence_count > 0 {
            facts.insert(CausalStateFact::TestEvidenceAvailable);
        }
        if inputs.has_failed_tests {
            facts.insert(CausalStateFact::FailedTestEvidence);
        }
        if inputs.unresolved_error_count > 0 {
            facts.insert(CausalStateFact::UnresolvedError);
        }
        if inputs.security_finding_count > 0 {
            facts.insert(CausalStateFact::SecurityFinding);
        }
        if inputs.lsp_preview_available {
            facts.insert(CausalStateFact::LspPreviewAvailable);
        }
        if inputs.context_read_available {
            facts.insert(CausalStateFact::ContextReadAvailable);
        }
        if inputs.unmet_test_acceptance {
            facts.insert(CausalStateFact::UnmetTestAcceptance);
        }
        if inputs.unmet_commit_acceptance {
            facts.insert(CausalStateFact::UnmetCommitAcceptance);
        }
        if inputs.unmet_artifact_acceptance {
            facts.insert(CausalStateFact::UnmetArtifactAcceptance);
        }
        if inputs.unmet_delegated_run_acceptance {
            facts.insert(CausalStateFact::UnmetDelegatedRunAcceptance);
        }

        let mut host_revisions = BTreeMap::new();
        if let Some(revision) = inputs.goal_revision {
            host_revisions.insert(HOST_REVISION_GOAL.to_string(), revision.to_string());
        }
        if let Some(revision) = inputs.work_plan_revision {
            host_revisions.insert(HOST_REVISION_WORK_PLAN.to_string(), revision.to_string());
        }

        let fingerprint =
            Self::compute_fingerprint(&facts, inputs, &host_revisions, surface_fingerprint);
        Self {
            schema_version: CAUSAL_SNAPSHOT_SCHEMA_VERSION,
            ontology_version: CAUSAL_ONTOLOGY_VERSION,
            facts,
            goal_id: inputs.goal_id.as_deref().map(bound_id),
            work_plan_id: inputs.work_plan_id.as_deref().map(bound_id),
            current_item_id: inputs.current_item_id.as_deref().map(bound_id),
            touched_file_count: bound_count(inputs.touched_file_count),
            test_evidence_count: bound_count(inputs.test_evidence_count),
            unresolved_error_count: bound_count(inputs.unresolved_error_count),
            security_finding_count: bound_count(inputs.security_finding_count),
            artifact_handle_count: bound_count(inputs.artifact_handle_count),
            host_revisions,
            surface_fingerprint: surface_fingerprint.to_string(),
            fingerprint,
        }
    }

    fn compute_fingerprint(
        facts: &BTreeSet<CausalStateFact>,
        inputs: &CausalStateInputs,
        host_revisions: &BTreeMap<String, String>,
        surface_fingerprint: &str,
    ) -> String {
        let payload = serde_json::json!({
            "schema_version": CAUSAL_SNAPSHOT_SCHEMA_VERSION,
            "ontology_version": CAUSAL_ONTOLOGY_VERSION,
            "facts": facts.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
            "goal_id": inputs.goal_id.as_deref().map(bound_id),
            "work_plan_id": inputs.work_plan_id.as_deref().map(bound_id),
            "current_item_id": inputs.current_item_id.as_deref().map(bound_id),
            "touched_file_count": bound_count(inputs.touched_file_count),
            "test_evidence_count": bound_count(inputs.test_evidence_count),
            "unresolved_error_count": bound_count(inputs.unresolved_error_count),
            "security_finding_count": bound_count(inputs.security_finding_count),
            "artifact_handle_count": bound_count(inputs.artifact_handle_count),
            "host_revisions": host_revisions,
            "surface_fingerprint": surface_fingerprint,
        });
        sha256_hex(payload.to_string().as_bytes())
    }

    /// Whether the snapshot carries goal/plan/acceptance structure that a
    /// causal frontier may promote on. Snapshots without structured signal
    /// must abstain to the fallback universe rather than invent a frontier.
    pub fn has_structured_signal(&self) -> bool {
        const STRUCTURED: [CausalStateFact; 9] = [
            CausalStateFact::ActiveGoal,
            CausalStateFact::ActiveWorkPlan,
            CausalStateFact::ActionableWorkItem,
            CausalStateFact::InProgressWorkItem,
            CausalStateFact::BlockedWorkItem,
            CausalStateFact::UnmetTestAcceptance,
            CausalStateFact::UnmetCommitAcceptance,
            CausalStateFact::UnmetArtifactAcceptance,
            CausalStateFact::UnmetDelegatedRunAcceptance,
        ];
        STRUCTURED.iter().any(|fact| self.facts.contains(fact))
    }

    /// Whether M002 must abstain on this state: no structured signal means
    /// no causal promotion claim is justified.
    pub fn is_insufficient_state(&self) -> bool {
        !self.has_structured_signal()
    }
}

// ─── Stateful benchmark ───────────────────────────────────────────────────

/// Benchmark families in `assets/tool-advisor/causal-frontier-v1.jsonl`.
pub const CAUSAL_BENCHMARK_FAMILIES: &[&str] = &[
    "no_goal_plan",
    "active_goal",
    "workplan_state",
    "unmet_test_acceptance",
    "unmet_commit_acceptance",
    "artifact_recovery",
    "unresolved_error",
    "failed_tests",
    "lsp_preview",
    "delegation",
    "security_finding",
    "mixed_state",
    "unknown_uncontracted",
];

/// Optional structured desired outcome for later M003 effect-path work.
/// Present only when derivable from typed acceptance/evidence demand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalDesiredOutcome {
    pub kind: CausalOutcome,
    pub evidence_kind: Option<WorkEvidenceKind>,
}

/// One frozen stateful benchmark case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalBenchmarkCase {
    pub schema_version: u16,
    pub case_id: String,
    pub family: String,
    pub facts: BTreeSet<CausalStateFact>,
    /// Resolved eligible tool identities (post authority filtering).
    pub eligible: Vec<String>,
    /// Eligible tools carrying a native causal contract.
    pub contracted: Vec<String>,
    /// Eligible tools without a causal contract (always discoverable).
    pub uncontracted: Vec<String>,
    /// Tools the current step genuinely needs.
    pub gold_current: Vec<String>,
    /// Contracted tools that are premature/inadmissible in this state.
    pub gold_premature: Vec<String>,
    /// Tools that bypass causal suppression and remain visible.
    pub required_visible: Vec<String>,
    /// Tools that must never be reduced from the visible surface.
    pub never_reduce: Vec<String>,
    /// Tools filtered upstream (hidden/denied/unavailable). Must never
    /// appear in any frontier constructed from this case.
    #[serde(default)]
    pub withheld: Vec<String>,
    #[serde(default)]
    pub desired_outcome: Option<CausalDesiredOutcome>,
    /// True when the state carries no structured signal and M002 must
    /// abstain to the fallback universe.
    #[serde(default)]
    pub insufficient_state: bool,
    pub rationale: String,
    pub provenance: String,
}

/// Failures from benchmark-case validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CausalBenchmarkError {
    UnsupportedSchemaVersion(u16),
    EmptyCaseId,
    UnknownFamily(String),
    EmptyEligible,
    DuplicateTool(String),
    ContractedMismatch { tool: String, reason: String },
    GoldCurrentUnsatisfied(String),
    GoldCurrentUnknown(String),
    GoldPrematureSatisfied(String),
    GoldPrematureUncontracted(String),
    GoldOverlap(String),
    RequiredNotEligible(String),
    WithheldInEligible(String),
    InsufficientFlagMismatch { case_id: String },
}

impl std::fmt::Display for CausalBenchmarkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion(v) => {
                write!(f, "unsupported causal benchmark schema version {v}")
            }
            Self::EmptyCaseId => write!(f, "causal benchmark case id must not be empty"),
            Self::UnknownFamily(family) => {
                write!(f, "unknown causal benchmark family {family}")
            }
            Self::EmptyEligible => {
                write!(f, "causal benchmark case must list eligible tools")
            }
            Self::DuplicateTool(tool) => {
                write!(f, "duplicate tool identity {tool}")
            }
            Self::ContractedMismatch { tool, reason } => {
                write!(
                    f,
                    "contracted/uncontracted identity error for {tool}: {reason}"
                )
            }
            Self::GoldCurrentUnsatisfied(tool) => {
                write!(
                    f,
                    "gold current-step tool {tool} is not satisfied by case facts"
                )
            }
            Self::GoldCurrentUnknown(tool) => {
                write!(f, "gold current-step tool {tool} is not eligible")
            }
            Self::GoldPrematureSatisfied(tool) => {
                write!(f, "gold premature tool {tool} is satisfied by case facts")
            }
            Self::GoldPrematureUncontracted(tool) => {
                write!(f, "gold premature tool {tool} carries no contract")
            }
            Self::GoldOverlap(tool) => {
                write!(f, "tool {tool} appears in both gold sets")
            }
            Self::RequiredNotEligible(tool) => {
                write!(f, "required/never-reduce tool {tool} is not eligible")
            }
            Self::WithheldInEligible(tool) => {
                write!(f, "withheld tool {tool} appears in the eligible surface")
            }
            Self::InsufficientFlagMismatch { case_id } => {
                write!(f, "insufficient_state flag mismatch for case {case_id}")
            }
        }
    }
}

impl std::error::Error for CausalBenchmarkError {}

impl CausalBenchmarkCase {
    /// Validate one case against the live native contract table.
    ///
    /// Rules: contracted/uncontracted identity must match
    /// [`native_causal_contract`]; every contracted gold current-step tool
    /// must be satisfied by the case facts; every gold premature tool must
    /// carry a contract that is *not* satisfied; gold sets are disjoint;
    /// required/never-reduce tools are eligible and never premature;
    /// withheld tools never appear in the eligible surface; and the
    /// insufficient-state flag matches the structured-signal definition.
    pub fn validate(&self) -> Result<(), CausalBenchmarkError> {
        if self.schema_version != CAUSAL_BENCHMARK_SCHEMA_VERSION {
            return Err(CausalBenchmarkError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if self.case_id.trim().is_empty() {
            return Err(CausalBenchmarkError::EmptyCaseId);
        }
        if !CAUSAL_BENCHMARK_FAMILIES.contains(&self.family.as_str()) {
            return Err(CausalBenchmarkError::UnknownFamily(self.family.clone()));
        }
        if self.eligible.is_empty() {
            return Err(CausalBenchmarkError::EmptyEligible);
        }
        let mut seen = BTreeSet::new();
        for tool in &self.eligible {
            if !seen.insert(tool.clone()) {
                return Err(CausalBenchmarkError::DuplicateTool(tool.clone()));
            }
        }
        let mut seen_withheld = BTreeSet::new();
        for tool in &self.withheld {
            if !seen_withheld.insert(tool.clone()) {
                return Err(CausalBenchmarkError::DuplicateTool(tool.clone()));
            }
            if seen.contains(tool) {
                return Err(CausalBenchmarkError::WithheldInEligible(tool.clone()));
            }
        }
        let catalog = causal_catalog();
        // Contracted/uncontracted identity must exactly partition the
        // eligible surface according to the live native contract table.
        let mut partition: BTreeSet<&str> = BTreeSet::new();
        for tool in self.contracted.iter().chain(self.uncontracted.iter()) {
            if !partition.insert(tool.as_str()) {
                return Err(CausalBenchmarkError::DuplicateTool(tool.clone()));
            }
        }
        let eligible_set: BTreeSet<&str> = self.eligible.iter().map(String::as_str).collect();
        if partition != eligible_set {
            let missing: Vec<&&str> = eligible_set.difference(&partition).collect();
            let extra: Vec<&&str> = partition.difference(&eligible_set).collect();
            let tool = missing
                .first()
                .or(extra.first())
                .map(|name| (*name).to_string())
                .unwrap_or_default();
            return Err(CausalBenchmarkError::ContractedMismatch {
                tool,
                reason: "contracted/uncontracted lists must partition eligible".into(),
            });
        }
        for tool in &self.contracted {
            if !catalog.contains_key(tool) {
                return Err(CausalBenchmarkError::ContractedMismatch {
                    tool: tool.clone(),
                    reason: "listed contracted but carries no native contract".into(),
                });
            }
        }
        for tool in &self.uncontracted {
            if catalog.contains_key(tool) {
                return Err(CausalBenchmarkError::ContractedMismatch {
                    tool: tool.clone(),
                    reason: "listed uncontracted but carries a native contract".into(),
                });
            }
        }
        for tool in &self.gold_current {
            if !self.eligible.iter().any(|t| t == tool) {
                return Err(CausalBenchmarkError::GoldCurrentUnknown(tool.clone()));
            }
            match catalog.get(tool) {
                Some(contract) if !contract.is_satisfied_by(&self.facts) => {
                    return Err(CausalBenchmarkError::GoldCurrentUnsatisfied(tool.clone()));
                }
                _ => {}
            }
            if self.gold_premature.iter().any(|t| t == tool) {
                return Err(CausalBenchmarkError::GoldOverlap(tool.clone()));
            }
        }
        for tool in &self.gold_premature {
            if !self.eligible.iter().any(|t| t == tool) {
                return Err(CausalBenchmarkError::GoldCurrentUnknown(tool.clone()));
            }
            match catalog.get(tool) {
                Some(contract) if contract.is_satisfied_by(&self.facts) => {
                    return Err(CausalBenchmarkError::GoldPrematureSatisfied(tool.clone()));
                }
                Some(_) => {}
                None => {
                    return Err(CausalBenchmarkError::GoldPrematureUncontracted(
                        tool.clone(),
                    ));
                }
            }
            if self.required_visible.iter().any(|t| t == tool)
                || self.never_reduce.iter().any(|t| t == tool)
            {
                return Err(CausalBenchmarkError::GoldOverlap(tool.clone()));
            }
        }
        for tool in self.required_visible.iter().chain(self.never_reduce.iter()) {
            if !self.eligible.iter().any(|t| t == tool) {
                return Err(CausalBenchmarkError::RequiredNotEligible(tool.clone()));
            }
        }
        let expected_insufficient = !has_structured_signal(&self.facts);
        if self.insufficient_state != expected_insufficient {
            return Err(CausalBenchmarkError::InsufficientFlagMismatch {
                case_id: self.case_id.clone(),
            });
        }
        Ok(())
    }
}

/// Whether a fact set carries goal/plan/acceptance structure. Shared by
/// [`CausalStateSnapshot::has_structured_signal`] and benchmark validation
/// so the insufficient-state flag cannot drift from the definition.
pub fn has_structured_signal(facts: &BTreeSet<CausalStateFact>) -> bool {
    const STRUCTURED: [CausalStateFact; 9] = [
        CausalStateFact::ActiveGoal,
        CausalStateFact::ActiveWorkPlan,
        CausalStateFact::ActionableWorkItem,
        CausalStateFact::InProgressWorkItem,
        CausalStateFact::BlockedWorkItem,
        CausalStateFact::UnmetTestAcceptance,
        CausalStateFact::UnmetCommitAcceptance,
        CausalStateFact::UnmetArtifactAcceptance,
        CausalStateFact::UnmetDelegatedRunAcceptance,
    ];
    STRUCTURED.iter().any(|fact| facts.contains(fact))
}

/// Frontier-construction universe for one benchmark case: the eligible
/// surface only. Hidden, denied, disabled, or otherwise withheld tools can
/// never enter frontier construction through this helper.
pub fn benchmark_frontier_universe(case: &CausalBenchmarkCase) -> BTreeSet<String> {
    case.eligible.iter().cloned().collect()
}

/// Parse and validate every case in a causal-frontier benchmark JSONL
/// document. Returns cases in file order.
pub fn load_causal_benchmark(jsonl: &str) -> Result<Vec<CausalBenchmarkCase>, String> {
    let mut cases = Vec::new();
    let mut ids = BTreeSet::new();
    for (index, line) in jsonl.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.len() > crate::tool_advisor::MAX_CASE_BYTES {
            return Err(format!("case line {index} exceeds byte cap"));
        }
        let case: CausalBenchmarkCase = serde_json::from_str(line)
            .map_err(|err| format!("case line {index} parse error: {err}"))?;
        if !ids.insert(case.case_id.clone()) {
            return Err(format!("duplicate case id {}", case.case_id));
        }
        case.validate()
            .map_err(|err| format!("case {} invalid: {err}", case.case_id))?;
        cases.push(case);
    }
    Ok(cases)
}

// ─── M001 preregistration ─────────────────────────────────────────────────

/// Frozen M002 selection gates. Recorded before any frontier is measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalM002Gates {
    /// Pooled gold current-step tool preservation over qualification.
    pub gold_current_step_preservation: f64,
    /// Frontier tools outside the eligible surface or from withheld sets.
    pub authority_violations: u64,
    /// Uncontracted gold tools retained in the fallback universe.
    pub uncontracted_fallback_preservation: f64,
    /// 1 - (filtered premature exposure / baseline premature exposure).
    pub premature_exposure_reduction_min: f64,
    /// Median causally admissible deferred promotion set where structured
    /// signal exists.
    pub median_promotion_set_max_structured: usize,
    /// Pure frontier evaluation p95 budget in milliseconds (excludes I/O).
    pub pure_frontier_eval_p95_ms_max: f64,
}

impl CausalM002Gates {
    pub fn m001_frozen() -> Self {
        Self {
            gold_current_step_preservation: 1.0,
            authority_violations: 0,
            uncontracted_fallback_preservation: 1.0,
            premature_exposure_reduction_min: 0.5,
            median_promotion_set_max_structured: 4,
            pure_frontier_eval_p95_ms_max: 5.0,
        }
    }
}

/// Frozen M001 preregistration receipt: benchmark identity, contract
/// catalog identity, family-aware dev/qualification split, exact metric
/// formulas with tie-breaking, and the M002 gates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalM001Preregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub ontology_version: u16,
    pub contract_schema_version: u16,
    pub snapshot_schema_version: u16,
    pub benchmark_asset: String,
    pub benchmark_fingerprint: String,
    pub benchmark_cases: usize,
    pub contract_catalog_fingerprint: String,
    pub contracts: BTreeMap<String, serde_json::Value>,
    pub dev_case_ids: Vec<String>,
    pub qualification_case_ids: Vec<String>,
    pub dev_fingerprint: String,
    pub qualification_fingerprint: String,
    pub metric_formulas: BTreeMap<String, String>,
    pub tie_breaking: Vec<String>,
    pub gates: CausalM002Gates,
}

impl CausalM001Preregistration {
    /// Check the receipt against live recomputation: ontology/contract
    /// versions, catalog fingerprint, case counts, split coverage (disjoint,
    /// jointly exhaustive, family-balanced), and gate values.
    pub fn verify_against(&self, cases: &[CausalBenchmarkCase]) -> Result<(), String> {
        if self.schema_version != CAUSAL_BENCHMARK_SCHEMA_VERSION {
            return Err(format!(
                "prereg schema version {} != {CAUSAL_BENCHMARK_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.ontology_version != CAUSAL_ONTOLOGY_VERSION {
            return Err("prereg ontology version drift".into());
        }
        if self.contract_schema_version != CAUSAL_CONTRACT_SCHEMA_VERSION {
            return Err("prereg contract schema version drift".into());
        }
        if self.snapshot_schema_version != CAUSAL_SNAPSHOT_SCHEMA_VERSION {
            return Err("prereg snapshot schema version drift".into());
        }
        if self.contract_catalog_fingerprint != causal_catalog_fingerprint() {
            return Err("prereg contract catalog fingerprint drift".into());
        }
        if self.benchmark_cases != cases.len() {
            return Err(format!(
                "prereg case count {} != loaded {}",
                self.benchmark_cases,
                cases.len()
            ));
        }
        if self.gates != CausalM002Gates::m001_frozen() {
            return Err("prereg M002 gates differ from frozen values".into());
        }
        let all_ids: BTreeSet<&str> = cases.iter().map(|case| case.case_id.as_str()).collect();
        let dev: BTreeSet<&str> = self.dev_case_ids.iter().map(String::as_str).collect();
        let qual: BTreeSet<&str> = self
            .qualification_case_ids
            .iter()
            .map(String::as_str)
            .collect();
        if dev.intersection(&qual).next().is_some() {
            return Err("dev/qualification split leaks case ids".into());
        }
        let union: BTreeSet<&str> = dev.union(&qual).copied().collect();
        if union != all_ids {
            return Err("dev/qualification split does not cover the benchmark".into());
        }
        if self.dev_case_ids.len() != dev.len() || self.qualification_case_ids.len() != qual.len() {
            return Err("split lists contain duplicate case ids".into());
        }
        // Family balance: every family appears on both sides of the split.
        let mut dev_families = BTreeSet::new();
        let mut qual_families = BTreeSet::new();
        for case in cases {
            if dev.contains(case.case_id.as_str()) {
                dev_families.insert(case.family.clone());
            }
            if qual.contains(case.case_id.as_str()) {
                qual_families.insert(case.family.clone());
            }
        }
        let expected: BTreeSet<String> = CAUSAL_BENCHMARK_FAMILIES
            .iter()
            .map(|family| (*family).to_string())
            .collect();
        if dev_families != expected || qual_families != expected {
            return Err("split is not family-balanced".into());
        }
        let dev_fp = sha256_hex(self.dev_case_ids.join(",").as_bytes());
        let qual_fp = sha256_hex(self.qualification_case_ids.join(",").as_bytes());
        if dev_fp != self.dev_fingerprint || qual_fp != self.qualification_fingerprint {
            return Err("split fingerprint mismatch".into());
        }
        Ok(())
    }
}

// ─── M002 offline causal-admissibility frontier ────────────────────────────
//
// M002 evaluates deterministic precondition filtering over the already
// resolved eligible surface. It changes no runtime behavior: the frontier is
// a visibility recommendation computed offline against the frozen M001
// benchmark, compared against frozen baselines, and frozen into a
// machine-readable receipt. Uncontracted tools never enter the promotion
// frontier and remain in the fallback discovery universe; required and
// never-reduce tools bypass causal suppression; withheld tools fail closed.

/// Schema version of [`CausalFrontier`].
pub const CAUSAL_FRONTIER_SCHEMA_VERSION: u16 = 1;

/// Schema version of [`CausalM002Report`].
pub const CAUSAL_M002_REPORT_SCHEMA_VERSION: u16 = 1;

/// Checked-in machine-readable M002 result, relative to the workspace root.
pub const CAUSAL_M002_RESULT_ASSET: &str = "assets/tool-advisor/causal-frontier-m002-result.json";

/// Frozen M001 benchmark asset, relative to the workspace root.
pub const CAUSAL_BENCHMARK_ASSET: &str = "assets/tool-advisor/causal-frontier-v1.jsonl";

/// Frozen M001 preregistration asset, relative to the workspace root.
pub const CAUSAL_PREREG_ASSET: &str =
    "assets/tool-advisor/causal-frontier-m001-preregistration.json";

/// Frozen derived relevance view used by the historical-label diagnostic.
pub const CAUSAL_RELEVANCE_ASSET: &str = "assets/tool-advisor/retrieval-relevance-v1.json";

/// Pure frontier-evaluation iterations per qualification surface for the p95
/// latency gate (M001 prereg formula `pure_frontier_eval_p95_ms`).
pub const CAUSAL_M002_LATENCY_ITERATIONS: usize = 1001;

/// Whether a canonical tool name is deferred from the immediately visible
/// palette. Backed live by [`crate::tool::disclosure::CORE_PALETTE`]: names
/// outside the core palette are promotion candidates, names inside it are
/// already visible so promoting them is meaningless. The receipt records a
/// palette fingerprint so later palette edits invalidate M002 evidence
/// loudly instead of silently shifting the promotion scope.
pub fn is_palette_deferred(tool_name: &str) -> bool {
    !crate::tool::disclosure::CORE_PALETTE.contains(&tool_name)
}

/// Fingerprint of the live core palette backing the deferred-promotion
/// scope. Bound into [`CausalM002Report`] for drift detection.
pub fn palette_fingerprint() -> String {
    sha256_hex(crate::tool::disclosure::CORE_PALETTE.join(",").as_bytes())
}

/// Why a contracted tool is inadmissible for causal promotion under a fact
/// set. Exactly one reason is reported per tool, in contract-evaluation
/// order: missing conjunctive fact, then first unsatisfied disjunctive
/// group, then present forbidden fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalInadmissibilityReason {
    MissingRequired(CausalStateFact),
    UnsatisfiedAnyGroup(usize),
    ForbiddenPresent(CausalStateFact),
}

impl CausalInadmissibilityReason {
    /// Stable wire string for this reason (group index rendered decimal).
    pub fn as_str(self) -> String {
        match self {
            Self::MissingRequired(fact) => format!("missing_required:{}", fact.as_str()),
            Self::UnsatisfiedAnyGroup(index) => format!("unsatisfied_any_group:{index}"),
            Self::ForbiddenPresent(fact) => format!("forbidden_present:{}", fact.as_str()),
        }
    }

    /// First failing precondition of `contract` under `facts`, or `None`
    /// when the contract is satisfied. Iteration follows canonical
    /// (BTreeSet/enum-declaration) order, so the reported reason is
    /// deterministic for a given contract and fact set.
    pub fn primary_reason(
        contract: &ToolCausalContract,
        facts: &BTreeSet<CausalStateFact>,
    ) -> Option<Self> {
        for fact in &contract.requires_all {
            if !facts.contains(fact) {
                return Some(Self::MissingRequired(*fact));
            }
        }
        for (index, group) in contract.requires_any.iter().enumerate() {
            if group.is_disjoint(facts) {
                return Some(Self::UnsatisfiedAnyGroup(index));
            }
        }
        for fact in &contract.forbids {
            if facts.contains(fact) {
                return Some(Self::ForbiddenPresent(*fact));
            }
        }
        None
    }
}

/// Fail-closed frontier-construction failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CausalFrontierError {
    /// A withheld (hidden/denied/unavailable) tool reached frontier input.
    WithheldInSurface(String),
    /// A required/never-reduce tool is not on the eligible surface.
    RequiredNotEligible(String),
}

impl std::fmt::Display for CausalFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WithheldInSurface(tool) => {
                write!(f, "withheld tool {tool} reached causal frontier input")
            }
            Self::RequiredNotEligible(tool) => {
                write!(f, "required tool {tool} is not on the eligible surface")
            }
        }
    }
}

impl std::error::Error for CausalFrontierError {}

/// Inputs to one offline frontier evaluation. Everything is already
/// post-authority: `eligible` is the resolved surface after
/// hidden/denied/disabled/parent-ceiling filtering, and `facts` is the
/// host-owned state for the turn being evaluated.
pub struct FrontierInputs<'a> {
    pub eligible: &'a BTreeSet<String>,
    pub facts: &'a BTreeSet<CausalStateFact>,
    pub required_visible: &'a BTreeSet<String>,
    pub never_reduce: &'a BTreeSet<String>,
    pub withheld: &'a BTreeSet<String>,
    pub snapshot_fingerprint: &'a str,
    pub catalog: &'a BTreeMap<String, ToolCausalContract>,
    pub contract_catalog_fingerprint: &'a str,
}

/// Offline causal-admissibility frontier for one resolved surface.
///
/// A visibility recommendation only: admissible tools are promoted
/// candidates, inadmissible contracted tools are withheld from promotion
/// (never from callability), uncontracted tools stay in the fallback
/// discovery universe, and required tools stay visible. Carries no tool
/// definitions, no scores, and no provider payloads — only canonical names
/// and fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalFrontier {
    pub schema_version: u16,
    pub admissible_contracted: BTreeSet<String>,
    pub inadmissible_contracted: BTreeMap<String, CausalInadmissibilityReason>,
    pub uncontracted_fallback: BTreeSet<String>,
    pub required_visible: BTreeSet<String>,
    pub snapshot_fingerprint: String,
    pub contract_catalog_fingerprint: String,
}

impl CausalFrontier {
    /// Evaluate deterministic precondition filtering over the resolved
    /// surface. Iteration is BTreeSet-ordered by canonical tool name, so
    /// surface input order cannot affect the result. Fails closed when a
    /// withheld tool reaches the input or a required tool is missing from
    /// the eligible surface.
    pub fn evaluate(inputs: &FrontierInputs<'_>) -> Result<Self, CausalFrontierError> {
        for tool in inputs.eligible {
            if inputs.withheld.contains(tool) {
                return Err(CausalFrontierError::WithheldInSurface(tool.clone()));
            }
        }
        for tool in inputs
            .required_visible
            .iter()
            .chain(inputs.never_reduce.iter())
        {
            if !inputs.eligible.contains(tool) {
                return Err(CausalFrontierError::RequiredNotEligible(tool.clone()));
            }
            if inputs.withheld.contains(tool) {
                return Err(CausalFrontierError::WithheldInSurface(tool.clone()));
            }
        }
        let bypass: BTreeSet<&String> = inputs
            .required_visible
            .iter()
            .chain(inputs.never_reduce.iter())
            .collect();
        let mut frontier = Self {
            schema_version: CAUSAL_FRONTIER_SCHEMA_VERSION,
            admissible_contracted: BTreeSet::new(),
            inadmissible_contracted: BTreeMap::new(),
            uncontracted_fallback: BTreeSet::new(),
            required_visible: bypass.iter().map(|name| (*name).clone()).collect(),
            snapshot_fingerprint: inputs.snapshot_fingerprint.to_string(),
            contract_catalog_fingerprint: inputs.contract_catalog_fingerprint.to_string(),
        };
        for tool in inputs.eligible {
            if bypass.contains(tool) {
                continue;
            }
            match inputs.catalog.get(tool) {
                Some(contract) => {
                    match CausalInadmissibilityReason::primary_reason(contract, inputs.facts) {
                        None => {
                            frontier.admissible_contracted.insert(tool.clone());
                        }
                        Some(reason) => {
                            frontier
                                .inadmissible_contracted
                                .insert(tool.clone(), reason);
                        }
                    }
                }
                None => {
                    frontier.uncontracted_fallback.insert(tool.clone());
                }
            }
        }
        Ok(frontier)
    }

    /// Causally admissible deferred promotion set: admissible contracted
    /// tools that are deferred from the immediately visible palette and not
    /// in the required bypass. This is the only set M004/M005 may promote;
    /// required tools are excluded from reduction accounting per the frozen
    /// tie-breaking rules.
    pub fn deferred_promotion(&self) -> BTreeSet<String> {
        self.admissible_contracted
            .iter()
            .filter(|tool| is_palette_deferred(tool) && !self.required_visible.contains(*tool))
            .cloned()
            .collect()
    }

    /// Promotion set actually used for a case: the deferred promotion when
    /// the state carries structured signal, empty otherwise. Insufficient
    /// states abstain to the fallback universe rather than promoting a
    /// frontier.
    pub fn promotion_for_use(&self, structured_signal: bool) -> BTreeSet<String> {
        if structured_signal {
            self.deferred_promotion()
        } else {
            BTreeSet::new()
        }
    }

    /// Every tool the frontier keeps visible: admissible promotion,
    /// uncontracted fallback, and required bypass. Inadmissible contracted
    /// tools are the only eligible tools excluded.
    pub fn visible_union(&self) -> BTreeSet<String> {
        self.admissible_contracted
            .iter()
            .chain(self.uncontracted_fallback.iter())
            .chain(self.required_visible.iter())
            .cloned()
            .collect()
    }

    /// Whether this frontier is still fresh against a live snapshot and
    /// catalog fingerprint. State or contract drift invalidates the cached
    /// result instead of silently reusing it.
    pub fn is_fresh_against(
        &self,
        snapshot_fingerprint: &str,
        contract_catalog_fingerprint: &str,
    ) -> bool {
        self.snapshot_fingerprint == snapshot_fingerprint
            && self.contract_catalog_fingerprint == contract_catalog_fingerprint
    }
}

/// State fingerprint for one benchmark case: binds the case identity to its
/// frozen fact set. Benchmark cases carry facts rather than live snapshots,
/// so the frontier's snapshot slot records this derivation explicitly.
pub fn benchmark_case_state_fingerprint(case: &CausalBenchmarkCase) -> String {
    let facts: Vec<&str> = case.facts.iter().map(|fact| fact.as_str()).collect();
    let payload = serde_json::json!({
        "derivation": "causal-benchmark-state-v1",
        "case_id": case.case_id,
        "facts": facts,
    });
    sha256_hex(payload.to_string().as_bytes())
}

/// Evaluate the offline frontier for one frozen benchmark case.
pub fn evaluate_benchmark_case(
    case: &CausalBenchmarkCase,
    catalog: &BTreeMap<String, ToolCausalContract>,
    contract_catalog_fingerprint: &str,
) -> Result<CausalFrontier, CausalFrontierError> {
    let inputs = FrontierInputs {
        eligible: &case.eligible.iter().cloned().collect(),
        facts: &case.facts,
        required_visible: &case.required_visible.iter().cloned().collect(),
        never_reduce: &case.never_reduce.iter().cloned().collect(),
        withheld: &case.withheld.iter().cloned().collect(),
        snapshot_fingerprint: &benchmark_case_state_fingerprint(case),
        catalog,
        contract_catalog_fingerprint,
    };
    CausalFrontier::evaluate(&inputs)
}

/// Per-case M002 evidence row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalCaseRow {
    pub case_id: String,
    pub family: String,
    pub structured_signal: bool,
    /// Used promotion set size (empty by abstention policy when no signal).
    pub promotion_size: usize,
    pub gold_current_total: usize,
    pub gold_current_visible: usize,
    /// Gold premature tools still exposed after filtering (admissible).
    /// The reduction gate is `1 - remaining / baseline` per the frozen
    /// formula; a fully effective filter drives this to zero.
    pub premature_baseline: usize,
    pub premature_remaining: usize,
    pub uncontracted_gold_total: usize,
    pub uncontracted_gold_retained: usize,
    pub core_projection_size: usize,
    pub core_projection_premature: usize,
}

/// Per-family M002 evidence row. Promotion medians cover structured-signal
/// cases only; families without structured signal report no median.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalFamilyRow {
    pub family: String,
    pub cases: usize,
    pub structured_cases: usize,
    pub gold_current_total: usize,
    pub gold_current_visible: usize,
    pub premature_baseline: usize,
    pub premature_remaining: usize,
    pub median_promotion_structured: Option<usize>,
    pub max_promotion_structured: usize,
}

/// Pooled M002 metrics for one benchmark partition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalPartitionReport {
    pub cases: usize,
    pub structured_cases: usize,
    pub gold_current_total: usize,
    pub gold_current_visible: usize,
    pub gold_current_step_preservation: f64,
    pub premature_baseline: usize,
    pub premature_remaining: usize,
    pub premature_exposure_reduction: f64,
    pub uncontracted_gold_total: usize,
    pub uncontracted_gold_retained: usize,
    pub uncontracted_fallback_preservation: f64,
    pub authority_violations: u64,
    pub abstention_violations: u64,
    /// Used promotion sizes over structured-signal cases, ascending.
    pub promotion_sizes_structured: Vec<usize>,
    pub median_promotion_structured: usize,
    pub max_promotion_structured: usize,
    /// `|eligible ∩ CORE_PALETTE|` per case, ascending (baseline arm 1).
    pub core_projection_sizes: Vec<usize>,
    pub core_projection_premature_total: usize,
    pub families: Vec<CausalFamilyRow>,
    pub rows: Vec<CausalCaseRow>,
}

/// Lower median of an ascending size list (index `(n-1)/2`). All M002
/// partitions have odd structured-case counts, so lower and upper medians
/// coincide; the convention is frozen here for determinism regardless.
fn lower_median_ascending(sorted: &[usize]) -> usize {
    debug_assert!(!sorted.is_empty());
    sorted[(sorted.len() - 1) / 2]
}

/// Score one benchmark partition: evaluate the frontier per case and pool
/// the M001-preregistered metrics. Pure measurement — no tuning, no
/// contract edits, no threshold fitting.
pub fn score_partition(
    cases: &[&CausalBenchmarkCase],
    catalog: &BTreeMap<String, ToolCausalContract>,
    contract_catalog_fingerprint: &str,
) -> Result<CausalPartitionReport, CausalFrontierError> {
    let mut rows = Vec::with_capacity(cases.len());
    let mut authority_violations: u64 = 0;
    for case in cases {
        let frontier = evaluate_benchmark_case(case, catalog, contract_catalog_fingerprint)?;
        let eligible: BTreeSet<&String> = case.eligible.iter().collect();
        let withheld: BTreeSet<&String> = case.withheld.iter().collect();
        // Authority audit: every frontier name must be eligible and never
        // withheld. evaluate() fails closed on withheld input, so this
        // double-checks output containment as well.
        for tool in frontier
            .admissible_contracted
            .iter()
            .chain(frontier.uncontracted_fallback.iter())
            .chain(frontier.required_visible.iter())
        {
            if !eligible.contains(tool) || withheld.contains(tool) {
                authority_violations += 1;
            }
        }
        let structured = has_structured_signal(&case.facts);
        debug_assert_eq!(structured, !case.insufficient_state);
        let used_promotion = frontier.promotion_for_use(structured);
        let visible = frontier.visible_union();
        let gold_current_total = case.gold_current.len();
        let gold_current_visible = case
            .gold_current
            .iter()
            .filter(|tool| visible.contains(*tool))
            .count();
        let premature_baseline = case.gold_premature.len();
        let premature_remaining = case
            .gold_premature
            .iter()
            .filter(|tool| frontier.admissible_contracted.contains(*tool))
            .count();
        let uncontracted_gold: Vec<&String> = case
            .gold_current
            .iter()
            .filter(|tool| case.uncontracted.iter().any(|name| name == *tool))
            .collect();
        // Retained means still discoverable: the fallback universe, or the
        // required bypass (which preserves visibility at least as strongly).
        let uncontracted_gold_retained = uncontracted_gold
            .iter()
            .filter(|tool| visible.contains(**tool))
            .count();
        let core_projection: Vec<&String> = case
            .eligible
            .iter()
            .filter(|tool| !is_palette_deferred(tool))
            .collect();
        let core_projection_premature = case
            .gold_premature
            .iter()
            .filter(|tool| !is_palette_deferred(tool))
            .count();
        rows.push(CausalCaseRow {
            case_id: case.case_id.clone(),
            family: case.family.clone(),
            structured_signal: structured,
            promotion_size: used_promotion.len(),
            gold_current_total,
            gold_current_visible,
            premature_baseline,
            premature_remaining,
            uncontracted_gold_total: uncontracted_gold.len(),
            uncontracted_gold_retained,
            core_projection_size: core_projection.len(),
            core_projection_premature,
        });
    }
    let structured_rows: Vec<&CausalCaseRow> =
        rows.iter().filter(|row| row.structured_signal).collect();
    let mut promotion_sizes: Vec<usize> = structured_rows
        .iter()
        .map(|row| row.promotion_size)
        .collect();
    promotion_sizes.sort_unstable();
    let mut core_sizes: Vec<usize> = rows.iter().map(|row| row.core_projection_size).collect();
    core_sizes.sort_unstable();
    let sum = |f: fn(&CausalCaseRow) -> usize| rows.iter().map(f).sum::<usize>();
    let gold_total = sum(|row| row.gold_current_total);
    let gold_visible = sum(|row| row.gold_current_visible);
    let prem_base = sum(|row| row.premature_baseline);
    let prem_remaining = sum(|row| row.premature_remaining);
    let unc_total = sum(|row| row.uncontracted_gold_total);
    let unc_retained = sum(|row| row.uncontracted_gold_retained);
    let abstention_violations = rows
        .iter()
        .filter(|row| !row.structured_signal && row.promotion_size > 0)
        .count() as u64;
    let mut families: Vec<CausalFamilyRow> = Vec::new();
    for family in CAUSAL_BENCHMARK_FAMILIES {
        let family_rows: Vec<&CausalCaseRow> =
            rows.iter().filter(|row| row.family == *family).collect();
        if family_rows.is_empty() {
            continue;
        }
        let mut family_sizes: Vec<usize> = family_rows
            .iter()
            .filter(|row| row.structured_signal)
            .map(|row| row.promotion_size)
            .collect();
        family_sizes.sort_unstable();
        families.push(CausalFamilyRow {
            family: (*family).to_string(),
            cases: family_rows.len(),
            structured_cases: family_sizes.len(),
            gold_current_total: family_rows.iter().map(|row| row.gold_current_total).sum(),
            gold_current_visible: family_rows.iter().map(|row| row.gold_current_visible).sum(),
            premature_baseline: family_rows.iter().map(|row| row.premature_baseline).sum(),
            premature_remaining: family_rows.iter().map(|row| row.premature_remaining).sum(),
            median_promotion_structured: if family_sizes.is_empty() {
                None
            } else {
                Some(lower_median_ascending(&family_sizes))
            },
            max_promotion_structured: family_sizes.last().copied().unwrap_or(0),
        });
    }
    Ok(CausalPartitionReport {
        cases: rows.len(),
        structured_cases: structured_rows.len(),
        gold_current_total: gold_total,
        gold_current_visible: gold_visible,
        gold_current_step_preservation: if gold_total == 0 {
            1.0
        } else {
            gold_visible as f64 / gold_total as f64
        },
        premature_baseline: prem_base,
        premature_remaining: prem_remaining,
        premature_exposure_reduction: if prem_base == 0 {
            1.0
        } else {
            1.0 - prem_remaining as f64 / prem_base as f64
        },
        uncontracted_gold_total: unc_total,
        uncontracted_gold_retained: unc_retained,
        uncontracted_fallback_preservation: if unc_total == 0 {
            1.0
        } else {
            unc_retained as f64 / unc_total as f64
        },
        authority_violations,
        abstention_violations,
        median_promotion_structured: if promotion_sizes.is_empty() {
            0
        } else {
            lower_median_ascending(&promotion_sizes)
        },
        max_promotion_structured: promotion_sizes.last().copied().unwrap_or(0),
        promotion_sizes_structured: promotion_sizes,
        core_projection_sizes: core_sizes,
        core_projection_premature_total: sum(|row| row.core_projection_premature),
        families,
        rows,
    })
}

/// Nearest-rank percentile of ascending samples (rank `ceil(p*n)`,
/// 1-indexed). Used for the p95 pure-evaluation latency gate.
fn nearest_rank_percentile(sorted: &[f64], rank: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    debug_assert!((0.0..=1.0).contains(&rank));
    let index = (f64::ceil(rank * sorted.len() as f64) as usize).max(1) - 1;
    sorted[index.min(sorted.len() - 1)]
}

/// Pure frontier-evaluation latency samples in milliseconds, pooled over
/// every surface × `iterations`. Inputs are precomputed once per surface so
/// only contract precondition checks over the resolved surface are timed;
/// state projection and I/O are excluded per the prereg formula.
pub fn measure_frontier_latency_ms(
    cases: &[&CausalBenchmarkCase],
    catalog: &BTreeMap<String, ToolCausalContract>,
    contract_catalog_fingerprint: &str,
    iterations: usize,
) -> Vec<f64> {
    struct Precomputed {
        eligible: BTreeSet<String>,
        facts: BTreeSet<CausalStateFact>,
        required_visible: BTreeSet<String>,
        never_reduce: BTreeSet<String>,
        withheld: BTreeSet<String>,
        snapshot_fingerprint: String,
    }
    let surfaces: Vec<Precomputed> = cases
        .iter()
        .map(|case| Precomputed {
            eligible: case.eligible.iter().cloned().collect(),
            facts: case.facts.clone(),
            required_visible: case.required_visible.iter().cloned().collect(),
            never_reduce: case.never_reduce.iter().cloned().collect(),
            withheld: case.withheld.iter().cloned().collect(),
            snapshot_fingerprint: benchmark_case_state_fingerprint(case),
        })
        .collect();
    let mut samples = Vec::with_capacity(surfaces.len() * iterations);
    for surface in &surfaces {
        let inputs = FrontierInputs {
            eligible: &surface.eligible,
            facts: &surface.facts,
            required_visible: &surface.required_visible,
            never_reduce: &surface.never_reduce,
            withheld: &surface.withheld,
            snapshot_fingerprint: &surface.snapshot_fingerprint,
            catalog,
            contract_catalog_fingerprint,
        };
        for _ in 0..iterations {
            let start = std::time::Instant::now();
            let frontier = CausalFrontier::evaluate(&inputs)
                .expect("benchmark surfaces evaluate without authority errors");
            std::hint::black_box(frontier.admissible_contracted.len());
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    samples
}

/// M002 selection disposition over a scored qualification partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalM002Disposition {
    /// All frozen gates pass: M004 observe integration unblocks, and M003
    /// may run as the optional effect-path experiment.
    Positive,
    /// Correctness holds but reduction/menu-quality gates fail: the
    /// workstream closes negative; no semantic rescue permitted.
    NoReduction,
    /// Correctness failure: stop and register a corrective.
    ContractFailure,
}

impl CausalM002Disposition {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Positive => "A",
            Self::NoReduction => "D",
            Self::ContractFailure => "E",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::Positive => "A — positive causal-admissibility architecture",
            Self::NoReduction => "D — no useful structural reduction",
            Self::ContractFailure => "E — contract/state correctness failure",
        }
    }
}

/// Decide the M002 disposition from scored qualification metrics and the
/// measured p95 latency. Correctness gates use exact integer accounting, so
/// no float threshold ambiguity can flip D/E; the reduction gate compares
/// the recorded ratio against the frozen minimum.
pub fn decide_disposition(
    metrics: &CausalPartitionReport,
    gates: &CausalM002Gates,
    p95_ms: f64,
) -> CausalM002Disposition {
    let correct = metrics.gold_current_visible == metrics.gold_current_total
        && metrics.authority_violations == 0
        && metrics.uncontracted_gold_retained == metrics.uncontracted_gold_total
        && metrics.abstention_violations == 0;
    if !correct {
        return CausalM002Disposition::ContractFailure;
    }
    let reduction_holds =
        metrics.premature_exposure_reduction >= gates.premature_exposure_reduction_min;
    if reduction_holds
        && metrics.median_promotion_structured <= gates.median_promotion_set_max_structured
        && p95_ms <= gates.pure_frontier_eval_p95_ms_max
    {
        CausalM002Disposition::Positive
    } else {
        CausalM002Disposition::NoReduction
    }
}

/// Historical retrieval-label class for one frozen relevance-view candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalLabelClass {
    /// Carries a native contract satisfied with no state facts.
    AdmissibleAnyState,
    /// Carries a native contract requiring host state.
    ContractedStateGated,
    /// Registered tool without a causal contract: discoverable, never
    /// promoted.
    UncontractedKnown,
    /// No registered tool under this identity.
    UnavailableUnknown,
}

/// Classify one historical retrieval label against the live native catalog.
pub fn classify_historical_label(
    candidate: &str,
    is_known_tool: impl Fn(&str) -> bool,
) -> HistoricalLabelClass {
    match native_causal_contract(candidate) {
        Some(contract) if contract.is_satisfied_by(&BTreeSet::new()) => {
            HistoricalLabelClass::AdmissibleAnyState
        }
        Some(_) => HistoricalLabelClass::ContractedStateGated,
        None if is_known_tool(candidate) => HistoricalLabelClass::UncontractedKnown,
        None => HistoricalLabelClass::UnavailableUnknown,
    }
}

/// Diagnostic-only classification of the distinct current-step candidate
/// labels in the frozen derived relevance view. The causal benchmark owns
/// selection; this report only shows where historical retrieval labels fall
/// under causal contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalLabelDiagnostic {
    pub source_asset: String,
    pub labels: BTreeMap<String, HistoricalLabelClass>,
    pub admissible_any_state: usize,
    pub contracted_state_gated: usize,
    pub uncontracted_known: usize,
    pub unavailable_unknown: usize,
}

/// Distinct `candidate` names among `current-step` entries of the frozen
/// derived relevance view.
pub fn historical_label_candidates(relevance_json: &str) -> Result<BTreeSet<String>, String> {
    let document: serde_json::Value = serde_json::from_str(relevance_json)
        .map_err(|err| format!("relevance parse error: {err}"))?;
    let entries = document
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "relevance view has no entries array".to_string())?;
    let mut candidates = BTreeSet::new();
    for entry in entries {
        let is_current =
            entry.get("class").and_then(serde_json::Value::as_str) == Some("current-step");
        if !is_current {
            continue;
        }
        let name = entry
            .get("candidate")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "current-step entry without candidate".to_string())?;
        candidates.insert(name.to_string());
    }
    Ok(candidates)
}

/// Run the historical-label diagnostic over a candidate set.
pub fn diagnose_retrieval_labels(
    candidates: &BTreeSet<String>,
    is_known_tool: impl Fn(&str) -> bool,
) -> RetrievalLabelDiagnostic {
    let mut labels = BTreeMap::new();
    let mut counts = [0usize; 4];
    for candidate in candidates {
        let class = classify_historical_label(candidate, &is_known_tool);
        match class {
            HistoricalLabelClass::AdmissibleAnyState => counts[0] += 1,
            HistoricalLabelClass::ContractedStateGated => counts[1] += 1,
            HistoricalLabelClass::UncontractedKnown => counts[2] += 1,
            HistoricalLabelClass::UnavailableUnknown => counts[3] += 1,
        }
        labels.insert(candidate.clone(), class);
    }
    RetrievalLabelDiagnostic {
        source_asset: CAUSAL_RELEVANCE_ASSET.to_string(),
        labels,
        admissible_any_state: counts[0],
        contracted_state_gated: counts[1],
        uncontracted_known: counts[2],
        unavailable_unknown: counts[3],
    }
}

/// Machine-readable M002 offline-qualification result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalM002Report {
    pub schema_version: u16,
    pub protocol: String,
    pub ontology_version: u16,
    pub contract_schema_version: u16,
    pub benchmark_asset: String,
    pub benchmark_fingerprint: String,
    pub contract_catalog_fingerprint: String,
    pub palette_fingerprint: String,
    pub dev_fingerprint: String,
    pub qualification_fingerprint: String,
    pub dev: CausalPartitionReport,
    pub qualification: CausalPartitionReport,
    pub latency_iterations_per_surface: usize,
    pub latency_samples: usize,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_max_ms: f64,
    pub gates: CausalM002Gates,
    pub gate_results: BTreeMap<String, bool>,
    pub disposition: String,
    pub disposition_summary: String,
    pub retrieval_label_diagnostic: RetrievalLabelDiagnostic,
}

/// Run the full M002 offline qualification: verify the frozen M001 inputs,
/// score dev (baselines + M002 arm) and qualification (M002 arm, once),
/// measure pure-evaluation latency, run the historical-label diagnostic,
/// and decide the disposition. No tunable parameters exist — contract
/// semantics are frozen in M001 — so dev inspection cannot leak into
/// qualification: both partitions are scored by the same deterministic
/// evaluation in one pass, and contracts are never edited afterward.
pub fn qualify_m002(
    benchmark_jsonl: &str,
    prereg_json: &str,
    relevance_json: &str,
    latency_iterations_per_surface: usize,
) -> Result<CausalM002Report, String> {
    let prereg: CausalM001Preregistration =
        serde_json::from_str(prereg_json).map_err(|err| format!("prereg parse error: {err}"))?;
    if sha256_hex(benchmark_jsonl.as_bytes()) != prereg.benchmark_fingerprint {
        return Err("benchmark bytes differ from preregistered fingerprint".into());
    }
    let cases = load_causal_benchmark(benchmark_jsonl)?;
    prereg.verify_against(&cases)?;
    let catalog = causal_catalog();
    let catalog_fingerprint = causal_catalog_fingerprint();
    let dev_ids: BTreeSet<&str> = prereg.dev_case_ids.iter().map(String::as_str).collect();
    let qual_ids: BTreeSet<&str> = prereg
        .qualification_case_ids
        .iter()
        .map(String::as_str)
        .collect();
    let dev_cases: Vec<&CausalBenchmarkCase> = cases
        .iter()
        .filter(|case| dev_ids.contains(case.case_id.as_str()))
        .collect();
    let qual_cases: Vec<&CausalBenchmarkCase> = cases
        .iter()
        .filter(|case| qual_ids.contains(case.case_id.as_str()))
        .collect();
    if dev_cases.len() != prereg.dev_case_ids.len()
        || qual_cases.len() != prereg.qualification_case_ids.len()
    {
        return Err("split ids do not resolve to benchmark cases".into());
    }
    let dev = score_partition(&dev_cases, &catalog, &catalog_fingerprint)
        .map_err(|err| format!("dev scoring failed: {err}"))?;
    let qualification = score_partition(&qual_cases, &catalog, &catalog_fingerprint)
        .map_err(|err| format!("qualification scoring failed: {err}"))?;
    let latency = measure_frontier_latency_ms(
        &qual_cases,
        &catalog,
        &catalog_fingerprint,
        latency_iterations_per_surface,
    );
    if latency.is_empty() {
        return Err("no latency samples collected".into());
    }
    let p50 = nearest_rank_percentile(&latency, 0.50);
    let p95 = nearest_rank_percentile(&latency, 0.95);
    let max = *latency.last().unwrap_or(&0.0);
    let gates = CausalM002Gates::m001_frozen();
    let disposition = decide_disposition(&qualification, &gates, p95);
    let mut gate_results = BTreeMap::new();
    gate_results.insert(
        "gold_current_step_preservation".to_string(),
        qualification.gold_current_visible == qualification.gold_current_total
            && qualification.gold_current_step_preservation == gates.gold_current_step_preservation,
    );
    gate_results.insert(
        "authority_violations".to_string(),
        qualification.authority_violations == gates.authority_violations,
    );
    gate_results.insert(
        "uncontracted_fallback_preservation".to_string(),
        qualification.uncontracted_gold_retained == qualification.uncontracted_gold_total
            && qualification.uncontracted_fallback_preservation
                == gates.uncontracted_fallback_preservation,
    );
    gate_results.insert(
        "premature_exposure_reduction".to_string(),
        qualification.premature_exposure_reduction >= gates.premature_exposure_reduction_min,
    );
    gate_results.insert(
        "median_promotion_set_size_structured".to_string(),
        qualification.median_promotion_structured <= gates.median_promotion_set_max_structured,
    );
    gate_results.insert(
        "pure_frontier_eval_p95_ms".to_string(),
        p95 <= gates.pure_frontier_eval_p95_ms_max,
    );
    gate_results.insert(
        "insufficient_state_abstention".to_string(),
        qualification.abstention_violations == 0,
    );
    let candidates = historical_label_candidates(relevance_json)?;
    let registry = crate::tool::ToolRegistry::with_defaults();
    let known: BTreeSet<String> = registry
        .list()
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect();
    let diagnostic = diagnose_retrieval_labels(&candidates, |name| known.contains(name));
    Ok(CausalM002Report {
        schema_version: CAUSAL_M002_REPORT_SCHEMA_VERSION,
        protocol: "causal-frontier-m002-offline-admissibility".to_string(),
        ontology_version: CAUSAL_ONTOLOGY_VERSION,
        contract_schema_version: CAUSAL_CONTRACT_SCHEMA_VERSION,
        benchmark_asset: CAUSAL_BENCHMARK_ASSET.to_string(),
        benchmark_fingerprint: prereg.benchmark_fingerprint.clone(),
        contract_catalog_fingerprint: catalog_fingerprint,
        palette_fingerprint: palette_fingerprint(),
        dev_fingerprint: prereg.dev_fingerprint.clone(),
        qualification_fingerprint: prereg.qualification_fingerprint.clone(),
        dev,
        qualification,
        latency_iterations_per_surface,
        latency_samples: latency.len(),
        latency_p50_ms: p50,
        latency_p95_ms: p95,
        latency_max_ms: max,
        gates,
        gate_results,
        disposition: disposition.code().to_string(),
        disposition_summary: disposition.summary().to_string(),
        retrieval_label_diagnostic: diagnostic,
    })
}

// ─── M003 structured effect-path frontier ────────────────────────────────────
//
// Bounded effect-path refinement over the positive M002 admissibility
// frontier. M003 runs ONLY when CodeGG already holds an explicit structured
// desired outcome (a typed WorkPlan acceptance/evidence demand frozen in
// the benchmark, an unmet-acceptance demand derived from live host state,
// or explicitly armed preview-apply state). It never parses free-form
// prompts into goals: without a typed demand it returns `NoStructuredDemand`
// and the caller uses the M002 frontier unchanged.
//
// The planner treats admissible contracted tools as directed transitions
// from current facts to declared outcomes, enumerated breadth-first with
// maximum path depth 3, no repeated tool per path, no cycle expansion,
// deterministic lexicographic tie-break after path length, and no
// probabilistic score. A path is advisory: it does not execute tools and
// does not claim effects have already occurred. Unknown/uncontracted tools
// are outside the graph and remain fallback-discoverable through M002.

/// Schema version of [`EffectPathFrontier`].
pub const CAUSAL_EFFECT_PATH_SCHEMA_VERSION: u16 = 1;

/// Schema version of [`CausalM003Report`].
pub const CAUSAL_M003_REPORT_SCHEMA_VERSION: u16 = 1;

/// Checked-in machine-readable M003 result, relative to the workspace root.
pub const CAUSAL_M003_RESULT_ASSET: &str = "assets/tool-advisor/causal-frontier-m003-result.json";

/// Maximum effect-path length (plan section 3).
pub const CAUSAL_M003_MAX_PATH_DEPTH: usize = 3;

/// Pure effect-path computation iterations per structured-demand surface
/// for the p95 latency gate. State projection and I/O are excluded, as in
/// M002: only typed demand handling plus bounded path search are timed.
pub const CAUSAL_M003_LATENCY_ITERATIONS: usize = 1001;

/// Frozen M003 selection gates. The median gate (`2`) is the largest
/// integer at least 25% below the M002 median promotion (`3`) measured on
/// the same frozen structured-demand qualification subset (M002 receipt:
/// qual structured-demand median 3, preservation 1.00, premature
/// remaining 0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalM003Gates {
    /// Pooled gold current-step tool preservation over qualification
    /// structured-demand cases.
    pub gold_current_step_preservation: f64,
    /// Frontier tools outside the eligible surface or from withheld sets.
    pub authority_violations: u64,
    /// Uncontracted gold tools retained in the fallback universe.
    pub uncontracted_fallback_preservation: f64,
    /// Maximum median caller-visible promoted size over qualification
    /// structured-demand cases (25%-below-M002 reduction gate).
    pub median_used_max_structured_demand: usize,
    /// Maximum p95 pure effect-path computation latency in milliseconds.
    pub pure_effect_path_p95_ms_max: f64,
}

impl CausalM003Gates {
    /// Frozen gate values for the M003 selection decision.
    pub const fn m003_frozen() -> Self {
        Self {
            gold_current_step_preservation: 1.0,
            authority_violations: 0,
            uncontracted_fallback_preservation: 1.0,
            median_used_max_structured_demand: 2,
            pure_effect_path_p95_ms_max: 5.0,
        }
    }
}

/// Why no structured demand exists. The caller must use the M002
/// admissibility frontier unchanged; no promotion claim is made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoStructuredDemandReason {
    /// The case carries no frozen structured desired outcome. Free-form
    /// text is never consulted, so prose mentioning "test", "commit",
    /// "fix", "search" or "rename" cannot create a demand.
    NoDesiredOutcome,
    /// The evidence kind has no outcome mapping (scheduler jobs).
    UnsupportedEvidenceKind { evidence: String },
    /// The frozen outcome and evidence kinds disagree; fail closed rather
    /// than guess which one the host meant.
    EvidenceMappingMismatch { outcome: String, evidence: String },
}

impl NoStructuredDemandReason {
    /// Stable wire code for this reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoDesiredOutcome => "no_desired_outcome",
            Self::UnsupportedEvidenceKind { .. } => "unsupported_evidence_kind",
            Self::EvidenceMappingMismatch { .. } => "evidence_mapping_mismatch",
        }
    }
}

/// Derive the structured demand for one frozen benchmark case.
///
/// Reads ONLY the typed `desired_outcome` field frozen in M001 and checks
/// it against the allowed host-owned demand mapping (plan section 2):
/// unmet `TestJob` -> `TestEvidenceProduced`, unmet `Commit` ->
/// `CommitEvidenceProduced`, unmet `Artifact` -> `ArtifactExpanded`, unmet
/// `DelegatedRun`/`AgentRun` -> `DelegatedRunProduced`. `SchedulerJob` and
/// absent evidence kinds abstain; a kind/evidence mismatch fails closed.
/// `rationale`, `provenance`, `case_id` and `family` are never read.
pub fn derive_structured_demand(
    case: &CausalBenchmarkCase,
) -> Result<CausalDesiredOutcome, NoStructuredDemandReason> {
    let desired = case
        .desired_outcome
        .as_ref()
        .ok_or(NoStructuredDemandReason::NoDesiredOutcome)?;
    let expected = match desired.evidence_kind {
        None => {
            return Err(NoStructuredDemandReason::UnsupportedEvidenceKind {
                evidence: "none".to_string(),
            });
        }
        Some(WorkEvidenceKind::TestJob) => CausalOutcome::TestEvidenceProduced,
        Some(WorkEvidenceKind::Commit) => CausalOutcome::CommitEvidenceProduced,
        Some(WorkEvidenceKind::Artifact) => CausalOutcome::ArtifactExpanded,
        Some(WorkEvidenceKind::DelegatedRun) | Some(WorkEvidenceKind::AgentRun) => {
            CausalOutcome::DelegatedRunProduced
        }
        Some(WorkEvidenceKind::SchedulerJob) => {
            return Err(NoStructuredDemandReason::UnsupportedEvidenceKind {
                evidence: WorkEvidenceKind::SchedulerJob.as_str().to_string(),
            });
        }
    };
    if desired.kind != expected {
        return Err(NoStructuredDemandReason::EvidenceMappingMismatch {
            outcome: desired.kind.as_str().to_string(),
            evidence: desired
                .evidence_kind
                .map(WorkEvidenceKind::as_str)
                .unwrap_or("none")
                .to_string(),
        });
    }
    Ok(desired.clone())
}

/// Derive structured demands from live host-owned acceptance state.
///
/// Mirrors the typed `apply_work_plan` mapping (unmet acceptance plus an
/// evidence-kind ref): test, commit, artifact, and delegated-run demands.
/// `SchedulerJob` refs have no outcome mapping and are skipped, matching
/// the snapshot projection. Results follow [`CausalOutcome::all`]
/// canonical order, so simultaneous demands are deterministic. An empty
/// result means the caller must use the M002 frontier unchanged.
///
/// Note: the M001 snapshot projects only `DelegatedRun`-typed refs into
/// `unmet_delegated_run_acceptance`; `AgentRun`-typed refs never set that
/// fact, so a live `AgentRun` demand cannot arise through this helper
/// until the projection covers it. The benchmark mapping accepts both
/// evidence kinds for forward compatibility.
pub fn derive_unmet_demands(inputs: &CausalStateInputs) -> Vec<CausalDesiredOutcome> {
    let mut demands = Vec::new();
    if inputs.unmet_test_acceptance {
        demands.push(CausalDesiredOutcome {
            kind: CausalOutcome::TestEvidenceProduced,
            evidence_kind: Some(WorkEvidenceKind::TestJob),
        });
    }
    if inputs.unmet_commit_acceptance {
        demands.push(CausalDesiredOutcome {
            kind: CausalOutcome::CommitEvidenceProduced,
            evidence_kind: Some(WorkEvidenceKind::Commit),
        });
    }
    if inputs.unmet_delegated_run_acceptance {
        demands.push(CausalDesiredOutcome {
            kind: CausalOutcome::DelegatedRunProduced,
            evidence_kind: Some(WorkEvidenceKind::DelegatedRun),
        });
    }
    if inputs.unmet_artifact_acceptance {
        demands.push(CausalDesiredOutcome {
            kind: CausalOutcome::ArtifactExpanded,
            evidence_kind: Some(WorkEvidenceKind::Artifact),
        });
    }
    demands
}

/// Derive a workspace-mutation demand from explicit host checked-apply
/// state. Returns a demand ONLY when the host marks the turn-local LSP
/// preview as the active next transition (`preview_armed_for_apply`).
/// Preview availability alone never implies a mutation demand.
pub fn derive_preview_demand(preview_armed_for_apply: bool) -> Option<CausalDesiredOutcome> {
    if preview_armed_for_apply {
        Some(CausalDesiredOutcome {
            kind: CausalOutcome::WorkspaceMutationProduced,
            evidence_kind: None,
        })
    } else {
        None
    }
}

/// Bounded minimal effect-path search over contracted tools.
///
/// Breadth-first enumeration from `initial_facts`: at each level every
/// queued path extends with every candidate (lexicographic tool-name
/// order) whose preconditions hold and which is not already on the path.
/// The first path whose accumulated outcomes cover `demand` is minimal by
/// construction: levels run shortest-first and paths within a level run
/// in global lexicographic order, so the first cover is the
/// shortest-then-lexicographic minimum. No probabilistic score exists.
///
/// `fact_enablement` maps a produced outcome to extra facts it makes
/// available to later steps. M003 passes an empty map: the M001 pilot
/// ontology declares no outcome-to-fact enablement (every `requires_*`
/// fact is host state that no tool produces), so every M003 path under
/// the frozen catalog is a single direct-producer step. The bound
/// machinery (depth cap, no repeats, no cycle expansion) is implemented
/// generically and pinned by synthetic tests, so a future ontology with
/// real enablement inherits the same bounds.
pub fn find_minimal_effect_path(
    demand: CausalOutcome,
    initial_facts: &BTreeSet<CausalStateFact>,
    candidates: &BTreeMap<String, ToolCausalContract>,
    fact_enablement: &BTreeMap<CausalOutcome, BTreeSet<CausalStateFact>>,
    max_depth: usize,
) -> Option<Vec<String>> {
    if max_depth == 0 {
        return None;
    }
    let mut ordered: Vec<String> = candidates.keys().cloned().collect();
    ordered.sort();
    struct QueueEntry {
        path: Vec<String>,
        facts: BTreeSet<CausalStateFact>,
        outcomes: BTreeSet<CausalOutcome>,
    }
    let mut level = vec![QueueEntry {
        path: Vec::new(),
        facts: initial_facts.clone(),
        outcomes: BTreeSet::new(),
    }];
    for _ in 0..max_depth {
        let mut next = Vec::new();
        for entry in &level {
            for name in &ordered {
                if entry.path.iter().any(|step| step == name) {
                    continue;
                }
                let contract = candidates
                    .get(name)
                    .expect("ordered names come from candidates");
                if !contract.is_satisfied_by(&entry.facts) {
                    continue;
                }
                let mut facts = entry.facts.clone();
                let mut outcomes = entry.outcomes.clone();
                for outcome in &contract.produces {
                    outcomes.insert(*outcome);
                    if let Some(extra) = fact_enablement.get(outcome) {
                        facts.extend(extra.iter().copied());
                    }
                }
                let mut path = entry.path.clone();
                path.push(name.clone());
                if outcomes.contains(&demand) {
                    return Some(path);
                }
                next.push(QueueEntry {
                    path,
                    facts,
                    outcomes,
                });
            }
        }
        if next.is_empty() {
            return None;
        }
        level = next;
    }
    None
}

/// Plan the minimal effect path for one structured demand over the M002
/// admissible contracted set.
///
/// Only admissible contracted tools enter the graph. Unknown and
/// uncontracted tools are outside the graph by construction (they are not
/// in `catalog`), as are inadmissible contracted tools (they are not in
/// `admissible_contracted`); both stay fallback-discoverable through the
/// M002 frontier. Returns `None` when no admissible path covers the
/// demand; the caller then uses the M002 frontier unchanged.
pub fn plan_effect_path(
    demand: &CausalDesiredOutcome,
    facts: &BTreeSet<CausalStateFact>,
    admissible_contracted: &BTreeSet<String>,
    catalog: &BTreeMap<String, ToolCausalContract>,
) -> Option<Vec<String>> {
    let candidates: BTreeMap<String, ToolCausalContract> = admissible_contracted
        .iter()
        .filter_map(|name| {
            catalog
                .get(name)
                .map(|contract| (name.clone(), contract.clone()))
        })
        .collect();
    find_minimal_effect_path(
        demand.kind,
        facts,
        &candidates,
        &BTreeMap::new(),
        CAUSAL_M003_MAX_PATH_DEPTH,
    )
}

/// Registry backing effect-catalog bindings: the default registry plus
/// the session-gated pilot tools (`goal_get`, `goal_update_progress`,
/// `work_plan_get`, `work_plan_update_item`, `context_read`,
/// `lsp_preview_apply`) constructed over lazy handles, mirroring the M001
/// seam test. Construction performs no I/O: the pool is lazy, the
/// artifact store is in-memory, and the LSP service is unconnected.
/// Contract metadata is static (no session dependence), so bindings are
/// deterministic for a fixed binary.
fn effect_binding_registry() -> crate::tool::ToolRegistry {
    // `SqlitePool::connect_lazy` requires a Tokio context even though it
    // performs no I/O. Async callers already provide one; sync callers
    // (offline qualification, unit tests) get a scoped current-thread
    // runtime that is dropped before return.
    let owned_runtime: Option<tokio::runtime::Runtime> =
        if tokio::runtime::Handle::try_current().is_err() {
            Some(
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("effect-binding runtime builds"),
            )
        } else {
            None
        };
    let _context_guard = owned_runtime.as_ref().map(|runtime| runtime.enter());
    let mut registry = crate::tool::ToolRegistry::with_defaults();
    let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:")
        .expect("lazy pool construction performs no I/O");
    registry.register(crate::tool::goal::GoalGetTool::new(
        pool.clone(),
        "session_effect_binding".to_string(),
    ));
    registry.register(crate::tool::goal::GoalUpdateProgressTool::new(
        pool.clone(),
        "session_effect_binding".to_string(),
    ));
    registry.register(crate::tool::work_plan::WorkPlanGetTool::new(
        pool.clone(),
        "session_effect_binding".to_string(),
    ));
    registry.register(crate::tool::work_plan::WorkPlanUpdateItemTool::new(
        pool.clone(),
        "session_effect_binding".to_string(),
    ));
    registry.register(crate::context::ContextReadTool::new(
        std::sync::Arc::new(crate::context::InMemoryArtifactStore::new()),
        "session_effect_binding".to_string(),
    ));
    let preview_registry: crate::tool::LspPreviewRegistryHandle = std::sync::Arc::new(
        parking_lot::Mutex::new(egglsp::preview_registry::PreviewArtifactRegistry::new()),
    );
    let lsp_service = crate::lsp::service::LspService::new_arc(crate::lsp::config_lsp_to_egglsp(
        crate::config::schema::LspConfig::default(),
    ));
    registry.register(crate::tool::lsp_preview_apply::LspPreviewApplyTool::new(
        pool,
        std::path::PathBuf::from("/tmp"),
        "workspace_effect_binding".to_string(),
        "session_effect_binding".to_string(),
        None,
        std::sync::Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
        lsp_service,
        preview_registry,
    ));
    registry
}

/// Fingerprint over the effect-path contract catalog: the causal ontology
/// and contract schema versions plus, per pilot tool, the bound
/// fingerprint tying the causal payload to the live tool implementation
/// id/version and input-schema fingerprint (see
/// [`bind_causal_contract`]). Unlike [`causal_catalog_fingerprint`]
/// (payload-only), this fingerprint goes stale when a tool implementation
/// or schema drifts, failing closed to the M002 fallback instead of
/// planning against outdated effects. Tools that do not resolve in the
/// live registry bind under an explicit `unresolved-tool` marker so the
/// gap is visible in the fingerprint rather than silently skipped.
pub fn effect_catalog_fingerprint() -> String {
    let registry = effect_binding_registry();
    let catalog = causal_catalog();
    let mut entries = Vec::with_capacity(catalog.len());
    for (name, contract) in catalog.iter() {
        let bound = match registry.get(name) {
            Some(tool) => {
                let live = tool.contract(name, tool.parameters());
                bind_causal_contract(
                    name,
                    contract,
                    &live.implementation_id,
                    &live.implementation_version,
                    &live.input_schema,
                )
                .map(|bound| bound.fingerprint)
                .unwrap_or_else(|_| "binding-error".to_string())
            }
            None => "unresolved-tool".to_string(),
        };
        entries.push(serde_json::json!({ "tool_name": name, "bound_fingerprint": bound }));
    }
    let payload = serde_json::json!({
        "ontology_version": CAUSAL_ONTOLOGY_VERSION,
        "contract_schema_version": CAUSAL_CONTRACT_SCHEMA_VERSION,
        "effect_path_schema_version": CAUSAL_EFFECT_PATH_SCHEMA_VERSION,
        "contracts": entries,
    });
    sha256_hex(payload.to_string().as_bytes())
}

/// Whether every native pilot tool resolves to a live registry
/// implementation. A missing binding means the effect catalog cannot
/// prove implementation identity for that tool.
pub fn effect_catalog_binding_complete() -> bool {
    let registry = effect_binding_registry();
    NATIVE_PILOT_TOOLS
        .iter()
        .all(|name| registry.get(name).is_some())
}

/// Advisory minimal effect-path frontier for one structured demand.
///
/// Planning metadata only: it names the tools whose declared effects
/// cover the demand in the fewest steps, with per-tool contract
/// provenance. It does not execute tools, authorize calls, or claim
/// effects have occurred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectPathFrontier {
    pub schema_version: u16,
    pub demand: CausalDesiredOutcome,
    /// Ordered minimal tool steps covering the demand.
    pub path: Vec<String>,
    /// Deduplicated path tools.
    pub path_tools: BTreeSet<String>,
    /// Contract provenance source per path tool (native static source).
    pub tool_provenance: BTreeMap<String, String>,
    pub snapshot_fingerprint: String,
    pub contract_catalog_fingerprint: String,
    pub effect_catalog_fingerprint: String,
}

impl EffectPathFrontier {
    /// Whether this frontier is still fresh against a live snapshot and
    /// effect-catalog fingerprint. State or contract drift invalidates the
    /// cached result instead of silently reusing it.
    pub fn is_fresh_against(
        &self,
        snapshot_fingerprint: &str,
        effect_catalog_fingerprint: &str,
    ) -> bool {
        self.snapshot_fingerprint == snapshot_fingerprint
            && self.effect_catalog_fingerprint == effect_catalog_fingerprint
    }
}

/// Fail-closed effect-path construction failures. Every other outcome is
/// an [`EffectPathStatus`] telling the caller to keep the M002 frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectPathError {
    /// A withheld (hidden/denied/unavailable) tool reached frontier input.
    WithheldInSurface(String),
    /// A required/never-reduce tool is not on the eligible surface.
    RequiredNotEligible(String),
}

impl std::fmt::Display for EffectPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WithheldInSurface(tool) => {
                write!(f, "withheld tool {tool} reached effect-path input")
            }
            Self::RequiredNotEligible(tool) => {
                write!(f, "required tool {tool} is not on the eligible surface")
            }
        }
    }
}

impl std::error::Error for EffectPathError {}

/// Caller disposition for one case. Only [`EffectPathStatus::Path`]
/// narrows the promotion frontier; every other status means the caller
/// uses the M002 admissibility frontier unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectPathStatus {
    /// A minimal effect path covers the structured demand.
    Path(EffectPathFrontier),
    /// No typed demand exists; M003 abstains without looking at prose.
    NoStructuredDemand(NoStructuredDemandReason),
    /// A typed demand exists but no admissible contracted path covers it
    /// (every producer is inadmissible under current facts); hiding the
    /// M002 frontier would be a false exclusion, so M003 abstains.
    NoAdmissiblePath { demand: CausalDesiredOutcome },
    /// The live effect catalog differs from the expected fingerprint;
    /// planning against stale contracts is refused.
    StaleCatalogFallback { expected: String, observed: String },
}

impl EffectPathStatus {
    /// Stable wire code for this status.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Path(_) => "path",
            Self::NoStructuredDemand(_) => "no_structured_demand",
            Self::NoAdmissiblePath { .. } => "no_admissible_path",
            Self::StaleCatalogFallback { .. } => "stale_catalog_fallback",
        }
    }

    /// Caller-visible promoted size: the path length when a path exists,
    /// otherwise the M002 promotion size (the caller keeps M002).
    pub fn used_size(&self, m002_promotion_size: usize) -> usize {
        match self {
            Self::Path(frontier) => frontier.path.len(),
            Self::NoStructuredDemand(_)
            | Self::NoAdmissiblePath { .. }
            | Self::StaleCatalogFallback { .. } => m002_promotion_size,
        }
    }
}

/// Evaluate the structured effect path for one frozen benchmark case.
///
/// Returns the M003 status together with the underlying M002 frontier so
/// scoring can compare both arms on identical inputs. Fails closed only
/// on authority errors (withheld/required); stale contracts and missing
/// or uncovered demands are abstention statuses, not errors.
pub fn evaluate_effect_path(
    case: &CausalBenchmarkCase,
    catalog: &BTreeMap<String, ToolCausalContract>,
    contract_catalog_fingerprint: &str,
    live_effect_catalog_fingerprint: &str,
    expected_effect_catalog_fingerprint: &str,
) -> Result<(EffectPathStatus, CausalFrontier), EffectPathError> {
    if live_effect_catalog_fingerprint != expected_effect_catalog_fingerprint {
        let frontier = evaluate_benchmark_case(case, catalog, contract_catalog_fingerprint)
            .map_err(|err| match err {
                CausalFrontierError::WithheldInSurface(tool) => {
                    EffectPathError::WithheldInSurface(tool)
                }
                CausalFrontierError::RequiredNotEligible(tool) => {
                    EffectPathError::RequiredNotEligible(tool)
                }
            })?;
        return Ok((
            EffectPathStatus::StaleCatalogFallback {
                expected: expected_effect_catalog_fingerprint.to_string(),
                observed: live_effect_catalog_fingerprint.to_string(),
            },
            frontier,
        ));
    }
    let frontier =
        evaluate_benchmark_case(case, catalog, contract_catalog_fingerprint).map_err(|err| {
            match err {
                CausalFrontierError::WithheldInSurface(tool) => {
                    EffectPathError::WithheldInSurface(tool)
                }
                CausalFrontierError::RequiredNotEligible(tool) => {
                    EffectPathError::RequiredNotEligible(tool)
                }
            }
        })?;
    let demand = match derive_structured_demand(case) {
        Ok(demand) => demand,
        Err(reason) => return Ok((EffectPathStatus::NoStructuredDemand(reason), frontier)),
    };
    match plan_effect_path(
        &demand,
        &case.facts,
        &frontier.admissible_contracted,
        catalog,
    ) {
        Some(path) => {
            let mut path_tools = BTreeSet::new();
            let mut tool_provenance = BTreeMap::new();
            for step in &path {
                path_tools.insert(step.clone());
                if let Some(contract) = catalog.get(step) {
                    tool_provenance.insert(step.clone(), contract.provenance.source.clone());
                }
            }
            Ok((
                EffectPathStatus::Path(EffectPathFrontier {
                    schema_version: CAUSAL_EFFECT_PATH_SCHEMA_VERSION,
                    demand,
                    path,
                    path_tools,
                    tool_provenance,
                    snapshot_fingerprint: benchmark_case_state_fingerprint(case),
                    contract_catalog_fingerprint: contract_catalog_fingerprint.to_string(),
                    effect_catalog_fingerprint: live_effect_catalog_fingerprint.to_string(),
                }),
                frontier,
            ))
        }
        None => Ok((EffectPathStatus::NoAdmissiblePath { demand }, frontier)),
    }
}

/// Per-case M003 evidence row over structured-demand cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectPathCaseRow {
    pub case_id: String,
    pub family: String,
    pub demand_kind: String,
    pub demand_evidence: String,
    pub status: String,
    pub status_detail: String,
    pub path_length: Option<usize>,
    pub path_tools: Vec<String>,
    pub m002_promotion_size: usize,
    pub m003_used_size: usize,
    pub gold_current_total: usize,
    pub gold_current_visible_m002: usize,
    pub gold_current_visible_m003: usize,
    /// Gold tools the M002 frontier keeps visible but M003 would hide.
    pub false_exclusions: Vec<String>,
    pub premature_baseline: usize,
    pub premature_m002_remaining: usize,
    pub premature_m003_remaining: usize,
    pub uncontracted_gold_total: usize,
    pub uncontracted_gold_retained: usize,
}

/// Per-family M003 evidence row over structured-demand cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectPathFamilyRow {
    pub family: String,
    pub cases: usize,
    pub path_found: usize,
    pub gold_current_total: usize,
    pub gold_current_visible_m003: usize,
    pub loses_gold_tool: bool,
    pub median_m002_promotion: usize,
    pub median_m003_used: usize,
    pub max_m003_used: usize,
}

/// Pooled M003 metrics for one structured-demand benchmark partition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectPathPartitionReport {
    pub cases: usize,
    pub path_found: usize,
    pub no_admissible_path: usize,
    pub abstentions: usize,
    /// Path lengths over path cases, ascending.
    pub path_lengths: Vec<usize>,
    pub median_path_length: usize,
    pub max_path_length: usize,
    pub gold_current_total: usize,
    pub gold_current_visible_m002: usize,
    pub gold_current_visible_m003: usize,
    pub gold_current_step_preservation_m002: f64,
    pub gold_current_step_preservation_m003: f64,
    /// Gold tools the M002 frontier keeps visible but M003 would hide.
    pub false_causal_exclusions: usize,
    /// Families where at least one gold current-step tool leaves the
    /// caller-visible set under M003, in canonical family order.
    pub families_losing_gold_tool: Vec<String>,
    pub premature_baseline: usize,
    pub premature_m002_remaining: usize,
    pub premature_m003_remaining: usize,
    /// Valid demands whose kind has at least one contracted producer.
    pub contract_demands_covered: usize,
    pub contract_demands_total: usize,
    pub contract_coverage: f64,
    pub uncontracted_gold_total: usize,
    pub uncontracted_gold_retained: usize,
    pub uncontracted_fallback_preservation: f64,
    pub authority_violations: u64,
    /// M002 used-promotion sizes over the same cases, ascending.
    pub m002_promotion_sizes: Vec<usize>,
    pub median_m002_promotion: usize,
    /// Caller-visible M003 promoted sizes over the same cases, ascending
    /// (path length when a path exists, else the M002 size).
    pub m003_used_sizes: Vec<usize>,
    pub median_m003_used: usize,
    pub max_m003_used: usize,
    pub families: Vec<EffectPathFamilyRow>,
    pub rows: Vec<EffectPathCaseRow>,
}

/// Score one structured-demand benchmark partition: evaluate the M003
/// status per case against the M002 reference arm on identical inputs and
/// pool the plan metrics. Pure measurement — no tuning, no contract
/// edits, no threshold fitting.
pub fn score_effect_partition(
    cases: &[&CausalBenchmarkCase],
    catalog: &BTreeMap<String, ToolCausalContract>,
    contract_catalog_fingerprint: &str,
    live_effect_catalog_fingerprint: &str,
    expected_effect_catalog_fingerprint: &str,
) -> Result<EffectPathPartitionReport, EffectPathError> {
    let mut rows = Vec::with_capacity(cases.len());
    let mut authority_violations: u64 = 0;
    for case in cases {
        let (status, m002_frontier) = evaluate_effect_path(
            case,
            catalog,
            contract_catalog_fingerprint,
            live_effect_catalog_fingerprint,
            expected_effect_catalog_fingerprint,
        )?;
        if let EffectPathStatus::Path(frontier) = &status {
            let eligible: BTreeSet<&String> = case.eligible.iter().collect();
            let withheld: BTreeSet<&String> = case.withheld.iter().collect();
            for tool in &frontier.path_tools {
                if !eligible.contains(tool) || withheld.contains(tool) {
                    authority_violations += 1;
                }
            }
        }
        let structured = has_structured_signal(&case.facts);
        let m002_promotion = m002_frontier.promotion_for_use(structured);
        let m002_visible = m002_frontier.visible_union();
        let m003_visible: BTreeSet<String> = match &status {
            EffectPathStatus::Path(frontier) => frontier
                .path_tools
                .iter()
                .chain(m002_frontier.uncontracted_fallback.iter())
                .chain(m002_frontier.required_visible.iter())
                .cloned()
                .collect(),
            EffectPathStatus::NoStructuredDemand(_)
            | EffectPathStatus::NoAdmissiblePath { .. }
            | EffectPathStatus::StaleCatalogFallback { .. } => m002_visible.clone(),
        };
        let (demand_kind, demand_evidence) = match &status {
            EffectPathStatus::Path(frontier) => (
                frontier.demand.kind.as_str().to_string(),
                frontier
                    .demand
                    .evidence_kind
                    .map(WorkEvidenceKind::as_str)
                    .unwrap_or("none")
                    .to_string(),
            ),
            EffectPathStatus::NoAdmissiblePath { demand } => (
                demand.kind.as_str().to_string(),
                demand
                    .evidence_kind
                    .map(WorkEvidenceKind::as_str)
                    .unwrap_or("none")
                    .to_string(),
            ),
            EffectPathStatus::NoStructuredDemand(_)
            | EffectPathStatus::StaleCatalogFallback { .. } => (
                case.desired_outcome
                    .as_ref()
                    .map(|desired| desired.kind.as_str().to_string())
                    .unwrap_or_else(|| "none".to_string()),
                case.desired_outcome
                    .as_ref()
                    .and_then(|desired| desired.evidence_kind)
                    .map(WorkEvidenceKind::as_str)
                    .unwrap_or("none")
                    .to_string(),
            ),
        };
        let status_detail = match &status {
            EffectPathStatus::Path(frontier) => {
                format!("demand={}", frontier.demand.kind.as_str())
            }
            EffectPathStatus::NoStructuredDemand(reason) => reason.code().to_string(),
            EffectPathStatus::NoAdmissiblePath { demand } => {
                format!("uncovered_demand={}", demand.kind.as_str())
            }
            EffectPathStatus::StaleCatalogFallback { .. } => "stale_effect_catalog".to_string(),
        };
        let gold_total = case.gold_current.len();
        let gold_m002 = case
            .gold_current
            .iter()
            .filter(|tool| m002_visible.contains(*tool))
            .count();
        let gold_m003 = case
            .gold_current
            .iter()
            .filter(|tool| m003_visible.contains(*tool))
            .count();
        let mut false_exclusions: Vec<String> = case
            .gold_current
            .iter()
            .filter(|tool| m002_visible.contains(*tool) && !m003_visible.contains(*tool))
            .cloned()
            .collect();
        false_exclusions.sort();
        let premature_baseline = case.gold_premature.len();
        let premature_m002 = case
            .gold_premature
            .iter()
            .filter(|tool| m002_frontier.admissible_contracted.contains(*tool))
            .count();
        let premature_m003 = match &status {
            EffectPathStatus::Path(frontier) => case
                .gold_premature
                .iter()
                .filter(|tool| frontier.path_tools.contains(*tool))
                .count(),
            EffectPathStatus::NoStructuredDemand(_)
            | EffectPathStatus::NoAdmissiblePath { .. }
            | EffectPathStatus::StaleCatalogFallback { .. } => premature_m002,
        };
        let uncontracted_gold: Vec<&String> = case
            .gold_current
            .iter()
            .filter(|tool| case.uncontracted.iter().any(|name| name == *tool))
            .collect();
        let uncontracted_retained = uncontracted_gold
            .iter()
            .filter(|tool| m003_visible.contains(**tool))
            .count();
        rows.push(EffectPathCaseRow {
            case_id: case.case_id.clone(),
            family: case.family.clone(),
            demand_kind,
            demand_evidence,
            status: status.code().to_string(),
            status_detail,
            path_length: match &status {
                EffectPathStatus::Path(frontier) => Some(frontier.path.len()),
                EffectPathStatus::NoStructuredDemand(_)
                | EffectPathStatus::NoAdmissiblePath { .. }
                | EffectPathStatus::StaleCatalogFallback { .. } => None,
            },
            path_tools: match &status {
                EffectPathStatus::Path(frontier) => frontier.path.clone(),
                EffectPathStatus::NoStructuredDemand(_)
                | EffectPathStatus::NoAdmissiblePath { .. }
                | EffectPathStatus::StaleCatalogFallback { .. } => Vec::new(),
            },
            m002_promotion_size: m002_promotion.len(),
            m003_used_size: status.used_size(m002_promotion.len()),
            gold_current_total: gold_total,
            gold_current_visible_m002: gold_m002,
            gold_current_visible_m003: gold_m003,
            false_exclusions,
            premature_baseline,
            premature_m002_remaining: premature_m002,
            premature_m003_remaining: premature_m003,
            uncontracted_gold_total: uncontracted_gold.len(),
            uncontracted_gold_retained: uncontracted_retained,
        });
    }
    let mut path_lengths: Vec<usize> = rows.iter().filter_map(|row| row.path_length).collect();
    path_lengths.sort_unstable();
    let mut m002_sizes: Vec<usize> = rows.iter().map(|row| row.m002_promotion_size).collect();
    m002_sizes.sort_unstable();
    let mut m003_sizes: Vec<usize> = rows.iter().map(|row| row.m003_used_size).collect();
    m003_sizes.sort_unstable();
    let sum = |f: fn(&EffectPathCaseRow) -> usize| rows.iter().map(f).sum::<usize>();
    let gold_total = sum(|row| row.gold_current_total);
    let gold_m002 = sum(|row| row.gold_current_visible_m002);
    let gold_m003 = sum(|row| row.gold_current_visible_m003);
    let prem_base = sum(|row| row.premature_baseline);
    let prem_m002 = sum(|row| row.premature_m002_remaining);
    let prem_m003 = sum(|row| row.premature_m003_remaining);
    let unc_total = sum(|row| row.uncontracted_gold_total);
    let unc_retained = sum(|row| row.uncontracted_gold_retained);
    let false_exclusions = sum(|row| row.false_exclusions.len());
    let valid_demands: Vec<&EffectPathCaseRow> = rows
        .iter()
        .filter(|row| row.status == "path" || row.status == "no_admissible_path")
        .collect();
    let covered = valid_demands
        .iter()
        .filter(|row| {
            catalog.values().any(|contract| {
                contract
                    .produces
                    .iter()
                    .any(|outcome| outcome.as_str() == row.demand_kind)
            })
        })
        .count();
    let mut families_losing = Vec::new();
    let mut families = Vec::new();
    for family in CAUSAL_BENCHMARK_FAMILIES {
        let family_rows: Vec<&EffectPathCaseRow> =
            rows.iter().filter(|row| row.family == *family).collect();
        if family_rows.is_empty() {
            continue;
        }
        let family_gold_total: usize = family_rows.iter().map(|row| row.gold_current_total).sum();
        let family_gold_m003: usize = family_rows
            .iter()
            .map(|row| row.gold_current_visible_m003)
            .sum();
        let loses = family_gold_m003 < family_gold_total;
        if loses {
            families_losing.push((*family).to_string());
        }
        let mut family_m002: Vec<usize> = family_rows
            .iter()
            .map(|row| row.m002_promotion_size)
            .collect();
        family_m002.sort_unstable();
        let mut family_m003: Vec<usize> =
            family_rows.iter().map(|row| row.m003_used_size).collect();
        family_m003.sort_unstable();
        families.push(EffectPathFamilyRow {
            family: (*family).to_string(),
            cases: family_rows.len(),
            path_found: family_rows
                .iter()
                .filter(|row| row.status == "path")
                .count(),
            gold_current_total: family_gold_total,
            gold_current_visible_m003: family_gold_m003,
            loses_gold_tool: loses,
            median_m002_promotion: lower_median_ascending(&family_m002),
            median_m003_used: lower_median_ascending(&family_m003),
            max_m003_used: family_m003.last().copied().unwrap_or(0),
        });
    }
    Ok(EffectPathPartitionReport {
        cases: rows.len(),
        path_found: rows.iter().filter(|row| row.status == "path").count(),
        no_admissible_path: rows
            .iter()
            .filter(|row| row.status == "no_admissible_path")
            .count(),
        abstentions: rows
            .iter()
            .filter(|row| {
                row.status == "no_structured_demand" || row.status == "stale_catalog_fallback"
            })
            .count(),
        median_path_length: if path_lengths.is_empty() {
            0
        } else {
            lower_median_ascending(&path_lengths)
        },
        max_path_length: path_lengths.last().copied().unwrap_or(0),
        path_lengths,
        gold_current_total: gold_total,
        gold_current_visible_m002: gold_m002,
        gold_current_visible_m003: gold_m003,
        gold_current_step_preservation_m002: if gold_total == 0 {
            1.0
        } else {
            gold_m002 as f64 / gold_total as f64
        },
        gold_current_step_preservation_m003: if gold_total == 0 {
            1.0
        } else {
            gold_m003 as f64 / gold_total as f64
        },
        false_causal_exclusions: false_exclusions,
        families_losing_gold_tool: families_losing,
        premature_baseline: prem_base,
        premature_m002_remaining: prem_m002,
        premature_m003_remaining: prem_m003,
        contract_demands_covered: covered,
        contract_demands_total: valid_demands.len(),
        contract_coverage: if valid_demands.is_empty() {
            1.0
        } else {
            covered as f64 / valid_demands.len() as f64
        },
        uncontracted_gold_total: unc_total,
        uncontracted_gold_retained: unc_retained,
        uncontracted_fallback_preservation: if unc_total == 0 {
            1.0
        } else {
            unc_retained as f64 / unc_total as f64
        },
        authority_violations,
        median_m002_promotion: if m002_sizes.is_empty() {
            0
        } else {
            lower_median_ascending(&m002_sizes)
        },
        m002_promotion_sizes: m002_sizes,
        median_m003_used: if m003_sizes.is_empty() {
            0
        } else {
            lower_median_ascending(&m003_sizes)
        },
        max_m003_used: m003_sizes.last().copied().unwrap_or(0),
        m003_used_sizes: m003_sizes,
        families,
        rows,
    })
}

/// Pure effect-path computation latency samples in milliseconds, pooled
/// over every structured-demand surface × `iterations`. Demands,
/// admissible sets, and facts are precomputed once per surface so only
/// typed demand handling plus bounded path search are timed; state
/// projection and I/O are excluded per the plan budget.
pub fn measure_effect_path_latency_ms(
    cases: &[&CausalBenchmarkCase],
    catalog: &BTreeMap<String, ToolCausalContract>,
    contract_catalog_fingerprint: &str,
) -> Vec<f64> {
    struct Precomputed {
        demand: CausalDesiredOutcome,
        facts: BTreeSet<CausalStateFact>,
        admissible: BTreeSet<String>,
    }
    let mut surfaces = Vec::new();
    for case in cases {
        let Ok(demand) = derive_structured_demand(case) else {
            continue;
        };
        let Ok((_, frontier)) =
            evaluate_effect_path(case, catalog, contract_catalog_fingerprint, "fp", "fp")
                .map(|(status, frontier)| (status.code(), frontier))
        else {
            continue;
        };
        surfaces.push(Precomputed {
            demand,
            facts: case.facts.clone(),
            admissible: frontier.admissible_contracted,
        });
    }
    let mut samples = Vec::with_capacity(surfaces.len() * CAUSAL_M003_LATENCY_ITERATIONS);
    for surface in &surfaces {
        for _ in 0..CAUSAL_M003_LATENCY_ITERATIONS {
            let start = std::time::Instant::now();
            let path = plan_effect_path(
                &surface.demand,
                &surface.facts,
                &surface.admissible,
                catalog,
            );
            std::hint::black_box(path.as_ref().map(Vec::len));
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    samples
}

/// M003 selection disposition over a scored qualification partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalM003Disposition {
    /// All frozen gates pass: M004 observe integration selects M003.
    Positive,
    /// Correctness holds but preservation/reduction gates fail: M003
    /// closes negative and M004 uses the positive M002 frontier.
    Negative,
    /// Correctness failure: stop and register a corrective.
    ContractFailure,
}

impl CausalM003Disposition {
    /// Stable single-letter code, shared with the M002 disposition
    /// vocabulary under a different protocol string.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Positive => "A",
            Self::Negative => "D",
            Self::ContractFailure => "E",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::Positive => "A — positive structured effect-path frontier",
            Self::Negative => {
                "D — negative: effect-path narrowing is unsafe or unhelpful; M002 remains selected"
            }
            Self::ContractFailure => "E — contract/state correctness failure",
        }
    }
}

/// Decide the M003 disposition from scored qualification metrics and the
/// measured p95 latency. Correctness uses exact integer accounting, so no
/// float threshold ambiguity can flip D/E; the reduction gate additionally
/// requires the frozen median cap and an exact integer 25% comparison
/// (`4 * m003 <= 3 * m002`) against the M002 reference median on the same
/// structured-demand subset.
pub fn decide_m003_disposition(
    metrics: &EffectPathPartitionReport,
    gates: &CausalM003Gates,
    p95_ms: f64,
) -> CausalM003Disposition {
    let correct = metrics.authority_violations == 0
        && metrics.uncontracted_gold_retained == metrics.uncontracted_gold_total;
    if !correct {
        return CausalM003Disposition::ContractFailure;
    }
    let reduction_holds = metrics.median_m003_used <= gates.median_used_max_structured_demand
        && (metrics.median_m002_promotion == 0
            || 4 * metrics.median_m003_used <= 3 * metrics.median_m002_promotion);
    if metrics.gold_current_visible_m003 == metrics.gold_current_total
        && metrics.families_losing_gold_tool.is_empty()
        && metrics.premature_m003_remaining <= metrics.premature_m002_remaining
        && metrics.contract_demands_covered == metrics.contract_demands_total
        && metrics.contract_demands_total > 0
        && reduction_holds
        && p95_ms <= gates.pure_effect_path_p95_ms_max
    {
        CausalM003Disposition::Positive
    } else {
        CausalM003Disposition::Negative
    }
}

/// Machine-readable M003 structured-effect-path qualification result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalM003Report {
    pub schema_version: u16,
    pub protocol: String,
    pub ontology_version: u16,
    pub contract_schema_version: u16,
    pub effect_path_schema_version: u16,
    pub benchmark_asset: String,
    pub benchmark_fingerprint: String,
    pub contract_catalog_fingerprint: String,
    pub effect_catalog_fingerprint: String,
    pub catalog_binding_complete: bool,
    pub dev_fingerprint: String,
    pub qualification_fingerprint: String,
    pub structured_dev_cases: usize,
    pub structured_qualification_cases: usize,
    pub dev: EffectPathPartitionReport,
    pub qualification: EffectPathPartitionReport,
    /// Exact integer 25% comparison on qualification medians:
    /// `4 * m003 <= 3 * m002`.
    pub reduction_holds_exact: bool,
    pub latency_iterations_per_surface: usize,
    pub latency_samples: usize,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_max_ms: f64,
    pub gates: CausalM003Gates,
    pub gate_results: BTreeMap<String, bool>,
    pub disposition: String,
    pub disposition_summary: String,
}

/// Run the full M003 offline qualification: verify the frozen M001 inputs,
/// score the structured-demand dev and qualification subsets (M003 arm
/// against the M002 reference arm on identical inputs), measure
/// pure-computation latency, and decide the disposition. No tunable
/// parameters exist — the planner has no scores or thresholds — so dev
/// inspection cannot leak into qualification.
pub fn qualify_m003(
    benchmark_jsonl: &str,
    prereg_json: &str,
    latency_iterations_per_surface: usize,
) -> Result<CausalM003Report, String> {
    let prereg: CausalM001Preregistration =
        serde_json::from_str(prereg_json).map_err(|err| format!("prereg parse error: {err}"))?;
    if sha256_hex(benchmark_jsonl.as_bytes()) != prereg.benchmark_fingerprint {
        return Err("benchmark bytes differ from preregistered fingerprint".into());
    }
    let cases = load_causal_benchmark(benchmark_jsonl)?;
    prereg.verify_against(&cases)?;
    let catalog = causal_catalog();
    let contract_fingerprint = causal_catalog_fingerprint();
    let effect_fingerprint = effect_catalog_fingerprint();
    let binding_complete = effect_catalog_binding_complete();
    let dev_ids: BTreeSet<&str> = prereg.dev_case_ids.iter().map(String::as_str).collect();
    let qual_ids: BTreeSet<&str> = prereg
        .qualification_case_ids
        .iter()
        .map(String::as_str)
        .collect();
    let dev_cases: Vec<&CausalBenchmarkCase> = cases
        .iter()
        .filter(|case| dev_ids.contains(case.case_id.as_str()) && case.desired_outcome.is_some())
        .collect();
    let qual_cases: Vec<&CausalBenchmarkCase> = cases
        .iter()
        .filter(|case| qual_ids.contains(case.case_id.as_str()) && case.desired_outcome.is_some())
        .collect();
    if dev_cases.is_empty() || qual_cases.is_empty() {
        return Err("no structured-demand cases in dev or qualification split".into());
    }
    let dev = score_effect_partition(
        &dev_cases,
        &catalog,
        &contract_fingerprint,
        &effect_fingerprint,
        &effect_fingerprint,
    )
    .map_err(|err| format!("dev scoring failed: {err}"))?;
    let qualification = score_effect_partition(
        &qual_cases,
        &catalog,
        &contract_fingerprint,
        &effect_fingerprint,
        &effect_fingerprint,
    )
    .map_err(|err| format!("qualification scoring failed: {err}"))?;
    let latency = measure_effect_path_latency_ms(&qual_cases, &catalog, &contract_fingerprint);
    if latency.is_empty() {
        return Err("no latency samples collected".into());
    }
    let p50 = nearest_rank_percentile(&latency, 0.50);
    let p95 = nearest_rank_percentile(&latency, 0.95);
    let max = *latency.last().unwrap_or(&0.0);
    let gates = CausalM003Gates::m003_frozen();
    let disposition = decide_m003_disposition(&qualification, &gates, p95);
    let reduction_holds_exact = qualification.median_m002_promotion == 0
        || 4 * qualification.median_m003_used <= 3 * qualification.median_m002_promotion;
    let mut gate_results = BTreeMap::new();
    gate_results.insert(
        "gold_current_step_preservation".to_string(),
        qualification.gold_current_visible_m003 == qualification.gold_current_total
            && qualification.gold_current_step_preservation_m003
                == gates.gold_current_step_preservation,
    );
    gate_results.insert(
        "no_family_loses_gold_tool".to_string(),
        qualification.families_losing_gold_tool.is_empty(),
    );
    gate_results.insert(
        "authority_violations".to_string(),
        qualification.authority_violations == gates.authority_violations,
    );
    gate_results.insert(
        "uncontracted_fallback_preservation".to_string(),
        qualification.uncontracted_gold_retained == qualification.uncontracted_gold_total
            && qualification.uncontracted_fallback_preservation
                == gates.uncontracted_fallback_preservation,
    );
    gate_results.insert(
        "median_used_reduction".to_string(),
        qualification.median_m003_used <= gates.median_used_max_structured_demand
            && reduction_holds_exact,
    );
    gate_results.insert(
        "premature_non_increasing".to_string(),
        qualification.premature_m003_remaining <= qualification.premature_m002_remaining,
    );
    gate_results.insert(
        "contract_coverage".to_string(),
        qualification.contract_demands_covered == qualification.contract_demands_total
            && qualification.contract_demands_total > 0,
    );
    gate_results.insert(
        "pure_effect_path_p95_ms".to_string(),
        p95 <= gates.pure_effect_path_p95_ms_max,
    );
    Ok(CausalM003Report {
        schema_version: CAUSAL_M003_REPORT_SCHEMA_VERSION,
        protocol: "causal-frontier-m003-structured-effect-path".to_string(),
        ontology_version: CAUSAL_ONTOLOGY_VERSION,
        contract_schema_version: CAUSAL_CONTRACT_SCHEMA_VERSION,
        effect_path_schema_version: CAUSAL_EFFECT_PATH_SCHEMA_VERSION,
        benchmark_asset: CAUSAL_BENCHMARK_ASSET.to_string(),
        benchmark_fingerprint: prereg.benchmark_fingerprint.clone(),
        contract_catalog_fingerprint: contract_fingerprint,
        effect_catalog_fingerprint: effect_fingerprint,
        catalog_binding_complete: binding_complete,
        dev_fingerprint: prereg.dev_fingerprint.clone(),
        qualification_fingerprint: prereg.qualification_fingerprint.clone(),
        structured_dev_cases: dev_cases.len(),
        structured_qualification_cases: qual_cases.len(),
        dev,
        qualification,
        reduction_holds_exact,
        latency_iterations_per_surface,
        latency_samples: latency.len(),
        latency_p50_ms: p50,
        latency_p95_ms: p95,
        latency_max_ms: max,
        gates,
        gate_results,
        disposition: disposition.code().to_string(),
        disposition_summary: disposition.summary().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    const BENCHMARK_JSONL: &str =
        include_str!("../../assets/tool-advisor/causal-frontier-v1.jsonl");
    const PREREG_JSON: &str =
        include_str!("../../assets/tool-advisor/causal-frontier-m001-preregistration.json");

    fn test_goal(status: codegg_core::goal::model::GoalStatus) -> Goal {
        Goal {
            id: "goal_test_1".to_string(),
            revision: 7,
            session_id: "session_test".to_string(),
            project_id: "project_test".to_string(),
            title: "test goal".to_string(),
            objective: "objective".to_string(),
            status,
            plan_path: None,
            checkpoint_path: None,
            current_phase: None,
            progress_summary: String::new(),
            next_action: None,
            completion_criteria: Vec::new(),
            open_questions: Vec::new(),
            budget: Default::default(),
            usage: Default::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    fn test_plan(status: WorkPlanStatus) -> WorkPlan {
        WorkPlan {
            id: codegg_core::work_plan::model::WorkPlanId("wp_test_1".to_string()),
            revision: 3,
            session_id: "session_test".to_string(),
            project_id: "project_test".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "objective".to_string(),
            objective_digest: "digest".to_string(),
            origin_provenance: "test".to_string(),
            status,
            current_phase: None,
            current_item_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        }
    }

    fn test_item(
        status: WorkItemStatus,
        acceptance: Vec<codegg_core::work_plan::model::WorkAcceptance>,
        evidence: Vec<codegg_core::work_plan::model::WorkEvidenceRef>,
    ) -> WorkItem {
        WorkItem {
            id: codegg_core::work_plan::model::WorkItemId("wi_test_1".to_string()),
            plan_id: codegg_core::work_plan::model::WorkPlanId("wp_test_1".to_string()),
            revision: 1,
            position: 0,
            parent_item_id: None,
            dependencies: Vec::new(),
            status,
            description: "item".to_string(),
            acceptance,
            evidence,
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: None,
            next_action: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn unmet_acceptance(
        kind: WorkEvidenceKind,
    ) -> (
        Vec<codegg_core::work_plan::model::WorkAcceptance>,
        Vec<codegg_core::work_plan::model::WorkEvidenceRef>,
    ) {
        (
            vec![codegg_core::work_plan::model::WorkAcceptance {
                description: "criterion".to_string(),
                disposition: WorkAcceptanceDisposition::Unmet,
                note: None,
            }],
            vec![codegg_core::work_plan::model::WorkEvidenceRef {
                kind,
                ref_id: "ref_1".to_string(),
                detail: None,
            }],
        )
    }

    fn snapshot_of(inputs: &CausalStateInputs) -> CausalStateSnapshot {
        CausalStateSnapshot::from_inputs(inputs, "surface_fp_test")
    }

    // ── Closed ontology ──────────────────────────────────────────────

    #[test]
    fn unknown_fact_value_rejected() {
        let parsed: Result<CausalStateFact, _> = serde_json::from_str("\"no_such_fact\"");
        assert!(parsed.is_err());
    }

    #[test]
    fn unknown_outcome_value_rejected() {
        let parsed: Result<CausalOutcome, _> = serde_json::from_str("\"no_such_outcome\"");
        assert!(parsed.is_err());
    }

    #[test]
    fn ontology_sizes_frozen() {
        assert_eq!(CausalStateFact::all().len(), 17);
        assert_eq!(CausalOutcome::all().len(), 14);
        assert_eq!(CAUSAL_ONTOLOGY_VERSION, 1);
    }

    #[test]
    fn fact_wire_strings_stable() {
        let mut seen = BTreeSet::new();
        for fact in CausalStateFact::all() {
            assert!(seen.insert(fact.as_str()));
            let json = serde_json::to_string(&fact).unwrap();
            assert_eq!(json, format!("\"{}\"", fact.as_str()));
            let back: CausalStateFact = serde_json::from_str(&json).unwrap();
            assert_eq!(back, fact);
        }
    }

    // ── One canonical host-source test per fact ──────────────────────

    #[test]
    fn fact_active_goal_from_goal_store() {
        let mut inputs = CausalStateInputs::default();
        inputs.apply_goal(Some(&test_goal(
            codegg_core::goal::model::GoalStatus::Active,
        )));
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::ActiveGoal));
        let mut idle = CausalStateInputs::default();
        idle.apply_goal(Some(&test_goal(
            codegg_core::goal::model::GoalStatus::Complete,
        )));
        assert!(!snapshot_of(&idle)
            .facts
            .contains(&CausalStateFact::ActiveGoal));
        let mut none = CausalStateInputs::default();
        none.apply_goal(None);
        assert!(!snapshot_of(&none)
            .facts
            .contains(&CausalStateFact::ActiveGoal));
    }

    #[test]
    fn fact_active_work_plan_from_plan_store() {
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(Some(&test_plan(WorkPlanStatus::Active)), &[]);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::ActiveWorkPlan));
        let mut blocked = CausalStateInputs::default();
        blocked.apply_work_plan(Some(&test_plan(WorkPlanStatus::Blocked)), &[]);
        assert!(snapshot_of(&blocked)
            .facts
            .contains(&CausalStateFact::ActiveWorkPlan));
        let mut done = CausalStateInputs::default();
        done.apply_work_plan(Some(&test_plan(WorkPlanStatus::Completed)), &[]);
        assert!(!snapshot_of(&done)
            .facts
            .contains(&CausalStateFact::ActiveWorkPlan));
    }

    #[test]
    fn fact_actionable_work_item_from_item_status() {
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(
                WorkItemStatus::Actionable,
                Vec::new(),
                Vec::new(),
            )],
        );
        let snapshot = snapshot_of(&inputs);
        assert!(snapshot
            .facts
            .contains(&CausalStateFact::ActionableWorkItem));
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::InProgressWorkItem));
    }

    #[test]
    fn fact_in_progress_work_item_from_item_status() {
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(
                WorkItemStatus::InProgress,
                Vec::new(),
                Vec::new(),
            )],
        );
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::InProgressWorkItem));
    }

    #[test]
    fn fact_blocked_work_item_from_item_status() {
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::Blocked, Vec::new(), Vec::new())],
        );
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::BlockedWorkItem));
    }

    #[test]
    fn fact_pending_item_sets_no_item_status_fact() {
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::Pending, Vec::new(), Vec::new())],
        );
        let snapshot = snapshot_of(&inputs);
        assert!(snapshot.facts.contains(&CausalStateFact::ActiveWorkPlan));
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::ActionableWorkItem));
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::InProgressWorkItem));
        assert!(!snapshot.facts.contains(&CausalStateFact::BlockedWorkItem));
    }

    #[test]
    fn fact_artifact_handle_available_from_ledger() {
        let mut inputs = CausalStateInputs::default();
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        ledger.artifact_handles.push("ctx://test/1".to_string());
        inputs.apply_ledger(&ledger);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::ArtifactHandleAvailable));
    }

    #[test]
    fn fact_touched_files_available_from_ledger() {
        let mut inputs = CausalStateInputs::default();
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        ledger.touched_files.push("src/main.rs".to_string());
        inputs.apply_ledger(&ledger);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::TouchedFilesAvailable));
    }

    #[test]
    fn fact_test_evidence_available_from_ledger() {
        let mut inputs = CausalStateInputs::default();
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        ledger.test_results.push("3 passed".to_string());
        inputs.apply_ledger(&ledger);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::TestEvidenceAvailable));
    }

    #[test]
    fn fact_failed_test_evidence_from_structured_status() {
        let mut inputs = CausalStateInputs::default();
        // Result strings alone never set the fact: only structured status does.
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        ledger.test_results.push("1 failed, 2 passed".to_string());
        inputs.apply_ledger(&ledger);
        assert!(!snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::FailedTestEvidence));
        inputs.set_failed_tests(true);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::FailedTestEvidence));
    }

    #[test]
    fn fact_unresolved_error_from_ledger() {
        let mut inputs = CausalStateInputs::default();
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        ledger.unresolved_errors.push("E Garcia".to_string());
        inputs.apply_ledger(&ledger);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::UnresolvedError));
    }

    #[test]
    fn fact_security_finding_from_count() {
        let mut inputs = CausalStateInputs::default();
        inputs.set_security_findings(2);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::SecurityFinding));
        let mut clear = CausalStateInputs::default();
        clear.set_security_findings(0);
        assert!(!snapshot_of(&clear)
            .facts
            .contains(&CausalStateFact::SecurityFinding));
    }

    #[test]
    fn fact_lsp_preview_available_from_preview_registry() {
        let mut inputs = CausalStateInputs::default();
        let empty = egglsp::preview_registry::PreviewArtifactRegistry::new();
        inputs.apply_preview_registry(&empty);
        assert!(!snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::LspPreviewAvailable));
    }

    #[test]
    fn fact_context_read_available_from_registration() {
        let mut inputs = CausalStateInputs::default();
        inputs.set_context_read_available(true);
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::ContextReadAvailable));
    }

    #[test]
    fn fact_unmet_test_acceptance_from_typed_evidence() {
        let (acceptance, evidence) = unmet_acceptance(WorkEvidenceKind::TestJob);
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::InProgress, acceptance, evidence)],
        );
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::UnmetTestAcceptance));
    }

    #[test]
    fn fact_unmet_commit_acceptance_from_typed_evidence() {
        let (acceptance, evidence) = unmet_acceptance(WorkEvidenceKind::Commit);
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::InProgress, acceptance, evidence)],
        );
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::UnmetCommitAcceptance));
    }

    #[test]
    fn fact_unmet_artifact_acceptance_from_typed_evidence() {
        let (acceptance, evidence) = unmet_acceptance(WorkEvidenceKind::Artifact);
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::InProgress, acceptance, evidence)],
        );
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::UnmetArtifactAcceptance));
    }

    #[test]
    fn fact_unmet_delegated_run_acceptance_from_typed_evidence() {
        let (acceptance, evidence) = unmet_acceptance(WorkEvidenceKind::DelegatedRun);
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::InProgress, acceptance, evidence)],
        );
        assert!(snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::UnmetDelegatedRunAcceptance));
    }

    #[test]
    fn unmet_acceptance_without_typed_evidence_sets_no_fact() {
        let acceptance = vec![codegg_core::work_plan::model::WorkAcceptance {
            description: "unmet but no typed evidence".to_string(),
            disposition: WorkAcceptanceDisposition::Unmet,
            note: None,
        }];
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(
                WorkItemStatus::InProgress,
                acceptance,
                Vec::new(),
            )],
        );
        let snapshot = snapshot_of(&inputs);
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::UnmetTestAcceptance));
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::UnmetCommitAcceptance));
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::UnmetArtifactAcceptance));
        assert!(!snapshot
            .facts
            .contains(&CausalStateFact::UnmetDelegatedRunAcceptance));
    }

    #[test]
    fn satisfied_acceptance_sets_no_unmet_fact() {
        let acceptance = vec![codegg_core::work_plan::model::WorkAcceptance {
            description: "done".to_string(),
            disposition: WorkAcceptanceDisposition::Satisfied,
            note: None,
        }];
        let evidence = vec![codegg_core::work_plan::model::WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job_1".to_string(),
            detail: None,
        }];
        let mut inputs = CausalStateInputs::default();
        inputs.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(WorkItemStatus::Completed, acceptance, evidence)],
        );
        assert!(!snapshot_of(&inputs)
            .facts
            .contains(&CausalStateFact::UnmetTestAcceptance));
    }

    // ── Snapshot properties ──────────────────────────────────────────

    #[test]
    fn snapshot_deterministic_and_fingerprint_stable() {
        let mut first = CausalStateInputs::default();
        first.apply_goal(Some(&test_goal(
            codegg_core::goal::model::GoalStatus::Active,
        )));
        first.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(
                WorkItemStatus::Actionable,
                Vec::new(),
                Vec::new(),
            )],
        );
        first.touched_file_count = 3;
        first.set_failed_tests(true);
        let mut second = CausalStateInputs::default();
        second.set_failed_tests(true);
        second.touched_file_count = 3;
        second.apply_work_plan(
            Some(&test_plan(WorkPlanStatus::Active)),
            &[test_item(
                WorkItemStatus::Actionable,
                Vec::new(),
                Vec::new(),
            )],
        );
        second.apply_goal(Some(&test_goal(
            codegg_core::goal::model::GoalStatus::Active,
        )));
        let left = CausalStateSnapshot::from_inputs(&first, "fp");
        let right = CausalStateSnapshot::from_inputs(&second, "fp");
        assert_eq!(left, right);
        assert_eq!(left.fingerprint, right.fingerprint);
        let json = serde_json::to_string(&left).unwrap();
        let back: CausalStateSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, left);
    }

    #[test]
    fn snapshot_binds_surface_fingerprint() {
        let inputs = CausalStateInputs::default();
        let left = CausalStateSnapshot::from_inputs(&inputs, "surface_a");
        let right = CausalStateSnapshot::from_inputs(&inputs, "surface_b");
        assert_ne!(left.fingerprint, right.fingerprint);
    }

    #[test]
    fn snapshot_binds_host_revisions() {
        let mut first = CausalStateInputs::default();
        first.apply_goal(Some(&test_goal(
            codegg_core::goal::model::GoalStatus::Active,
        )));
        let mut second = CausalStateInputs::default();
        second.apply_goal(Some(&test_goal(
            codegg_core::goal::model::GoalStatus::Active,
        )));
        second.goal_revision = Some(8);
        let left = CausalStateSnapshot::from_inputs(&first, "fp");
        let right = CausalStateSnapshot::from_inputs(&second, "fp");
        assert_ne!(left.fingerprint, right.fingerprint);
        assert_eq!(left.host_revisions.get(HOST_REVISION_GOAL).unwrap(), "7");
    }

    #[test]
    fn snapshot_contains_no_raw_prompt_or_output() {
        let secret_output = "sk-live-SECRET-OUTPUT-DO-NOT-STORE";
        let secret_error = "api_key=SECRET-ERROR-DO-NOT-STORE";
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        ledger.touched_files.push(secret_output.to_string());
        ledger.test_results.push(secret_output.to_string());
        ledger.unresolved_errors.push(secret_error.to_string());
        ledger.artifact_handles.push("ctx://secret/1".to_string());
        ledger.commands_run.push_back(secret_error.to_string());
        let mut inputs = CausalStateInputs::default();
        inputs.apply_ledger(&ledger);
        let snapshot = snapshot_of(&inputs);
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains(secret_output));
        assert!(!json.contains(secret_error));
        assert!(!json.contains("ctx://secret/1"));
        // Only the closed key vocabulary may appear.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let allowed = [
            "schema_version",
            "ontology_version",
            "facts",
            "goal_id",
            "work_plan_id",
            "current_item_id",
            "touched_file_count",
            "test_evidence_count",
            "unresolved_error_count",
            "security_finding_count",
            "artifact_handle_count",
            "host_revisions",
            "surface_fingerprint",
            "fingerprint",
        ];
        for key in value.as_object().unwrap().keys() {
            assert!(allowed.contains(&key.as_str()), "unexpected key {key}");
        }
    }

    #[test]
    fn snapshot_ids_and_counts_bounded() {
        let inputs = CausalStateInputs {
            goal_id: Some("g".repeat(10_000)),
            touched_file_count: usize::MAX,
            ..Default::default()
        };
        let snapshot = snapshot_of(&inputs);
        assert!(snapshot.goal_id.unwrap().len() <= MAX_CAUSAL_ID_CHARS);
        assert!(snapshot.touched_file_count <= MAX_CAUSAL_COUNT);
    }

    #[test]
    fn structured_signal_matches_helper() {
        let empty = snapshot_of(&CausalStateInputs::default());
        assert!(!empty.has_structured_signal());
        assert!(empty.is_insufficient_state());
        assert!(!has_structured_signal(&empty.facts));
        let inputs = CausalStateInputs {
            touched_file_count: 2,
            ..Default::default()
        };
        let unstructured = snapshot_of(&inputs);
        assert!(!unstructured.has_structured_signal());
        assert!(unstructured.is_insufficient_state());
        let structured = CausalStateInputs {
            has_active_goal: true,
            ..Default::default()
        };
        let with_goal = snapshot_of(&structured);
        assert!(with_goal.has_structured_signal());
        assert!(!with_goal.is_insufficient_state());
    }

    // ── Contract integrity ───────────────────────────────────────────

    fn sample_contract() -> ToolCausalContract {
        native_causal_contract("commit").expect("pilot contract")
    }

    #[test]
    fn pilot_contracts_validate() {
        for name in NATIVE_PILOT_TOOLS {
            let contract = native_causal_contract(name)
                .unwrap_or_else(|| panic!("missing pilot contract for {name}"));
            assert!(contract.validate().is_ok(), "invalid contract for {name}");
        }
    }

    #[test]
    fn pilot_tool_list_sorted_unique() {
        let mut sorted = NATIVE_PILOT_TOOLS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, NATIVE_PILOT_TOOLS);
    }

    #[test]
    fn uncontracted_tools_have_no_contract() {
        for name in [
            "bash",
            "tool_search",
            "skill",
            "question",
            "apply_patch",
            "external_mcp_tool",
            "custom_plugin_tool",
            "no_such_tool",
        ] {
            assert!(
                native_causal_contract(name).is_none(),
                "{name} must stay uncontracted"
            );
        }
    }

    #[test]
    fn contradiction_requires_all_forbids_rejected() {
        let mut contract = sample_contract();
        contract
            .forbids
            .insert(CausalStateFact::UnmetCommitAcceptance);
        assert_eq!(
            contract.validate(),
            Err(CausalContractError::Contradiction(
                CausalStateFact::UnmetCommitAcceptance
            ))
        );
    }

    #[test]
    fn contradiction_requires_any_forbids_rejected() {
        let mut contract = sample_contract();
        contract.requires_any = vec![[CausalStateFact::UnresolvedError].into_iter().collect()];
        contract.forbids.insert(CausalStateFact::UnresolvedError);
        assert_eq!(
            contract.validate(),
            Err(CausalContractError::Contradiction(
                CausalStateFact::UnresolvedError
            ))
        );
    }

    #[test]
    fn empty_requires_any_group_rejected() {
        let mut contract = sample_contract();
        contract.requires_any = vec![BTreeSet::new()];
        assert_eq!(
            contract.validate(),
            Err(CausalContractError::EmptyRequiresAnyGroup(0))
        );
    }

    #[test]
    fn empty_provenance_rejected() {
        let mut contract = sample_contract();
        contract.provenance = CausalContractProvenance::new("", "");
        assert_eq!(
            contract.validate(),
            Err(CausalContractError::EmptyProvenance)
        );
    }

    #[test]
    fn unsupported_schema_version_rejected() {
        let mut contract = sample_contract();
        contract.schema_version = 999;
        assert_eq!(
            contract.validate(),
            Err(CausalContractError::UnsupportedSchemaVersion(999))
        );
    }

    #[test]
    fn contract_serialization_deterministic() {
        let contract = native_causal_contract("work_plan_update_item").unwrap();
        let first = serde_json::to_string(&contract).unwrap();
        let second = serde_json::to_string(&contract).unwrap();
        assert_eq!(first, second);
        // Group order is normalized: a contract with swapped requires_any
        // groups shares one canonical form and fingerprint.
        let mut base = sample_contract();
        base.requires_any = vec![
            [CausalStateFact::ActiveGoal].into_iter().collect(),
            [CausalStateFact::ActiveWorkPlan].into_iter().collect(),
        ];
        let mut swapped = base.clone();
        swapped.requires_any.reverse();
        assert_ne!(base.requires_any, swapped.requires_any);
        assert_eq!(base.canonical_json(), swapped.canonical_json());
        let binding = ContractBinding {
            tool_name: "commit",
            implementation_id: "codegg/commit",
            implementation_version: "test",
            input_schema_fingerprint: "schema",
        };
        assert_eq!(base.fingerprint(&binding), swapped.fingerprint(&binding));
    }

    #[test]
    fn contract_name_mismatch_detectable() {
        let contract = native_causal_contract("read").unwrap();
        let schema = serde_json::json!({"type": "object"});
        let bound_read =
            bind_causal_contract("read", &contract, "codegg/read", "1.0", &schema).unwrap();
        let bound_write =
            bind_causal_contract("write", &contract, "codegg/read", "1.0", &schema).unwrap();
        assert_ne!(bound_read.fingerprint, bound_write.fingerprint);
        assert!(bind_causal_contract("", &contract, "codegg/read", "1.0", &schema).is_err());
    }

    #[test]
    fn contract_binding_sensitive_to_schema() {
        let contract = native_causal_contract("read").unwrap();
        let left = bind_causal_contract(
            "read",
            &contract,
            "codegg/read",
            "1.0",
            &serde_json::json!({"type": "object"}),
        )
        .unwrap();
        let right = bind_causal_contract(
            "read",
            &contract,
            "codegg/read",
            "1.0",
            &serde_json::json!({"type": "object", "properties": {"path": {}}}),
        )
        .unwrap();
        assert_ne!(left.fingerprint, right.fingerprint);
    }

    #[test]
    fn satisfaction_semantics() {
        let commit = native_causal_contract("commit").unwrap();
        let mut facts = BTreeSet::new();
        facts.insert(CausalStateFact::UnmetCommitAcceptance);
        assert!(commit.is_satisfied_by(&facts));
        facts.insert(CausalStateFact::UnresolvedError);
        assert!(!commit.is_satisfied_by(&facts));
        let update = native_causal_contract("work_plan_update_item").unwrap();
        let mut plan_facts = BTreeSet::new();
        plan_facts.insert(CausalStateFact::ActiveWorkPlan);
        assert!(!update.is_satisfied_by(&plan_facts));
        plan_facts.insert(CausalStateFact::BlockedWorkItem);
        assert!(update.is_satisfied_by(&plan_facts));
        let read = native_causal_contract("read").unwrap();
        assert!(read.is_satisfied_by(&BTreeSet::new()));
    }

    #[test]
    fn default_tool_trait_seam_is_none() {
        // The additive seam defaults to no contract: missing metadata is a
        // first-class state that implies nothing about authorization.
        let tool = crate::tool::bash::BashTool::new();
        assert!(crate::tool::Tool::causal_contract(&tool).is_none());
        let registry = crate::tool::ToolRegistry::with_defaults();
        assert!(registry.contains("bash"));
        assert!(registry.causal_contract_of("bash").is_none());
        assert!(registry.causal_contract_of("no_such_tool").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn registry_seam_matches_native_table() {
        // Every tool in the default registry agrees with the native table:
        // overrides cannot silently disagree with the frozen catalog.
        let registry = crate::tool::ToolRegistry::with_defaults();
        for tool in registry.list() {
            assert_eq!(
                tool.causal_contract().as_ref(),
                native_causal_contract(tool.name()).as_ref(),
                "seam mismatch for {}",
                tool.name()
            );
            assert_eq!(
                registry.causal_contract_of(tool.name()).as_ref(),
                native_causal_contract(tool.name()).as_ref(),
                "registry seam mismatch for {}",
                tool.name()
            );
        }
        // Session-gated pilot tools are wired to the same table. The lazy
        // pool never connects: construction alone exercises the seam.
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy pool connects without I/O");
        let mut gated = crate::tool::ToolRegistry::new();
        gated.register(crate::tool::goal::GoalGetTool::new(
            pool.clone(),
            "session_test".to_string(),
        ));
        gated.register(crate::tool::goal::GoalUpdateProgressTool::new(
            pool.clone(),
            "session_test".to_string(),
        ));
        gated.register(crate::tool::work_plan::WorkPlanGetTool::new(
            pool.clone(),
            "session_test".to_string(),
        ));
        gated.register(crate::tool::work_plan::WorkPlanUpdateItemTool::new(
            pool.clone(),
            "session_test".to_string(),
        ));
        gated.register(crate::context::ContextReadTool::new(
            std::sync::Arc::new(crate::context::InMemoryArtifactStore::new()),
            "session_test".to_string(),
        ));
        let preview_registry: crate::tool::LspPreviewRegistryHandle = std::sync::Arc::new(
            parking_lot::Mutex::new(egglsp::preview_registry::PreviewArtifactRegistry::new()),
        );
        let lsp_service = crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        );
        gated.register(crate::tool::lsp_preview_apply::LspPreviewApplyTool::new(
            pool,
            std::path::PathBuf::from("/tmp"),
            "workspace_test".to_string(),
            "session_test".to_string(),
            None,
            std::sync::Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
            lsp_service,
            preview_registry,
        ));
        for name in NATIVE_PILOT_TOOLS {
            assert!(
                gated.contains(name) || registry.contains(name),
                "pilot tool {name} registered nowhere"
            );
            let source = if gated.contains(name) {
                &gated
            } else {
                &registry
            };
            assert_eq!(
                source.causal_contract_of(name).as_ref(),
                native_causal_contract(name).as_ref(),
                "seam mismatch for {name}"
            );
        }
    }

    // ── Benchmark and preregistration ────────────────────────────────

    #[test]
    fn benchmark_loads_and_validates() {
        let cases = load_causal_benchmark(BENCHMARK_JSONL).expect("benchmark loads");
        assert!(cases.len() >= 160, "need >=160 cases, got {}", cases.len());
        let mut families = BTreeSet::new();
        for case in &cases {
            families.insert(case.family.clone());
        }
        let expected: BTreeSet<String> = CAUSAL_BENCHMARK_FAMILIES
            .iter()
            .map(|family| (*family).to_string())
            .collect();
        assert_eq!(families, expected);
    }

    #[test]
    fn benchmark_gold_consistency_against_live_contracts() {
        let cases = load_causal_benchmark(BENCHMARK_JSONL).expect("benchmark loads");
        let catalog = causal_catalog();
        // Every contracted gold current-step tool is satisfied (preservation
        // precondition); every contracted premature tool is not (reduction
        // precondition). Case validation already enforces both; re-derive
        // here as an independent cross-check.
        for case in &cases {
            for tool in &case.gold_current {
                if let Some(contract) = catalog.get(tool) {
                    assert!(
                        contract.is_satisfied_by(&case.facts),
                        "{}: gold {tool} unsatisfied",
                        case.case_id
                    );
                }
            }
            for tool in &case.gold_premature {
                let contract = catalog.get(tool).unwrap();
                assert!(
                    !contract.is_satisfied_by(&case.facts),
                    "{}: premature {tool} satisfied",
                    case.case_id
                );
            }
        }
    }

    #[test]
    fn benchmark_withheld_never_in_frontier_universe() {
        let cases = load_causal_benchmark(BENCHMARK_JSONL).expect("benchmark loads");
        for case in &cases {
            let universe = benchmark_frontier_universe(case);
            for tool in &case.withheld {
                assert!(!universe.contains(tool), "{} leaks {tool}", case.case_id);
            }
            for tool in case.eligible.iter().chain(case.required_visible.iter()) {
                assert!(universe.contains(tool));
            }
        }
    }

    #[test]
    fn preregistration_matches_recomputation() {
        let cases = load_causal_benchmark(BENCHMARK_JSONL).expect("benchmark loads");
        let prereg: CausalM001Preregistration =
            serde_json::from_str(PREREG_JSON).expect("prereg parses");
        prereg.verify_against(&cases).expect("prereg verifies");
        let benchmark_fp = sha256_hex(BENCHMARK_JSONL.as_bytes());
        assert_eq!(prereg.benchmark_fingerprint, benchmark_fp);
        assert_eq!(prereg.gates, CausalM002Gates::m001_frozen());
        assert_eq!(prereg.gates.gold_current_step_preservation, 1.0);
        assert_eq!(prereg.gates.authority_violations, 0);
        assert_eq!(prereg.gates.uncontracted_fallback_preservation, 1.0);
        assert_eq!(prereg.gates.premature_exposure_reduction_min, 0.5);
        assert_eq!(prereg.gates.median_promotion_set_max_structured, 4);
        assert_eq!(prereg.gates.pure_frontier_eval_p95_ms_max, 5.0);
        assert!(!prereg.metric_formulas.is_empty());
        assert!(!prereg.tie_breaking.is_empty());
        assert!(!prereg.contracts.is_empty());
        // Every frozen contract payload matches live recomputation.
        let catalog = causal_catalog();
        assert_eq!(prereg.contracts.len(), catalog.len());
        for (name, contract) in &catalog {
            let frozen = prereg
                .contracts
                .get(name)
                .unwrap_or_else(|| panic!("prereg missing contract for {name}"));
            assert_eq!(
                frozen,
                &contract.canonical_json(),
                "prereg contract drift for {name}"
            );
        }
    }

    // ─── M002 regression tests ───────────────────────────────────────────

    fn m002_test_contract(
        requires_all: &[CausalStateFact],
        forbids: &[CausalStateFact],
    ) -> ToolCausalContract {
        ToolCausalContract {
            schema_version: CAUSAL_CONTRACT_SCHEMA_VERSION,
            requires_all: requires_all.iter().copied().collect(),
            requires_any: Vec::new(),
            forbids: forbids.iter().copied().collect(),
            produces: BTreeSet::new(),
            provenance: CausalContractProvenance::new("test:m002", "m002 regression fixture"),
        }
    }

    fn m002_frontier_inputs<'a>(
        eligible: &'a BTreeSet<String>,
        facts: &'a BTreeSet<CausalStateFact>,
        catalog: &'a BTreeMap<String, ToolCausalContract>,
    ) -> FrontierInputs<'a> {
        static EMPTY: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
        let empty = EMPTY.get_or_init(BTreeSet::new);
        FrontierInputs {
            eligible,
            facts,
            required_visible: empty,
            never_reduce: empty,
            withheld: empty,
            snapshot_fingerprint: "test-snapshot",
            catalog,
            contract_catalog_fingerprint: "test-catalog",
        }
    }

    fn m002_benchmark_inputs() -> (
        Vec<CausalBenchmarkCase>,
        BTreeMap<String, ToolCausalContract>,
        String,
    ) {
        let cases =
            load_causal_benchmark(BENCHMARK_JSONL).expect("frozen benchmark loads for M002");
        let catalog = causal_catalog();
        let fingerprint = causal_catalog_fingerprint();
        (cases, catalog, fingerprint)
    }

    fn m002_split(
        cases: &[CausalBenchmarkCase],
    ) -> (Vec<&CausalBenchmarkCase>, Vec<&CausalBenchmarkCase>) {
        let prereg: CausalM001Preregistration =
            serde_json::from_str(PREREG_JSON).expect("prereg parses for M002");
        let dev_ids: BTreeSet<&str> = prereg.dev_case_ids.iter().map(String::as_str).collect();
        let mut dev = Vec::new();
        let mut qual = Vec::new();
        for case in cases {
            if dev_ids.contains(case.case_id.as_str()) {
                dev.push(case);
            } else {
                qual.push(case);
            }
        }
        (dev, qual)
    }

    #[test]
    fn m002_required_tool_bypasses_causal_suppression() {
        // A required tool stays visible even when its contract is
        // unsatisfied by the state; it never counts as premature exposure.
        let mut catalog = BTreeMap::new();
        catalog.insert(
            "commit".to_string(),
            m002_test_contract(&[CausalStateFact::UnmetCommitAcceptance], &[]),
        );
        let eligible: BTreeSet<String> = ["commit".to_string()].into_iter().collect();
        let facts = BTreeSet::new();
        let required: BTreeSet<String> = ["commit".to_string()].into_iter().collect();
        static EMPTY: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
        let empty = EMPTY.get_or_init(BTreeSet::new);
        let inputs = FrontierInputs {
            eligible: &eligible,
            facts: &facts,
            required_visible: &required,
            never_reduce: empty,
            withheld: empty,
            snapshot_fingerprint: "test-snapshot",
            catalog: &catalog,
            contract_catalog_fingerprint: "test-catalog",
        };
        let frontier = CausalFrontier::evaluate(&inputs).expect("required bypass evaluates");
        assert!(frontier.required_visible.contains("commit"));
        assert!(!frontier.admissible_contracted.contains("commit"));
        assert!(!frontier.inadmissible_contracted.contains_key("commit"));
        assert!(frontier.visible_union().contains("commit"));
        assert!(frontier.deferred_promotion().is_empty());
    }

    #[test]
    fn m002_uncontracted_tool_stays_in_fallback() {
        // Tools without contracts never enter promotion yet remain
        // discoverable in the fallback universe.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let case = cases
            .iter()
            .find(|case| !case.uncontracted.is_empty())
            .expect("benchmark has uncontracted tools");
        let frontier = evaluate_benchmark_case(case, &catalog, &fingerprint)
            .expect("benchmark case evaluates");
        for tool in &case.uncontracted {
            // Required-bypassed tools stay visible via the bypass rather
            // than the fallback; everything else must be in fallback.
            if case.required_visible.iter().any(|name| name == tool)
                || case.never_reduce.iter().any(|name| name == tool)
            {
                assert!(frontier.required_visible.contains(tool));
            } else {
                assert!(
                    frontier.uncontracted_fallback.contains(tool),
                    "{tool} must stay in fallback"
                );
            }
            assert!(!frontier.admissible_contracted.contains(tool));
            assert!(frontier.visible_union().contains(tool));
        }
    }

    #[test]
    fn m002_missing_fact_inadmissibility_is_deterministic() {
        // Same inputs always produce the same frontier; uncontracted tools
        // are unaffected by missing facts.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let case = &cases[0];
        let first = evaluate_benchmark_case(case, &catalog, &fingerprint).expect("evaluates");
        let second = evaluate_benchmark_case(case, &catalog, &fingerprint).expect("evaluates");
        assert_eq!(first, second);
        let reason = first.inadmissible_contracted.get("goal_get");
        if case.facts.contains(&CausalStateFact::ActiveGoal) {
            assert!(reason.is_none());
        } else if case.eligible.iter().any(|tool| tool == "goal_get") {
            assert_eq!(
                reason,
                Some(&CausalInadmissibilityReason::MissingRequired(
                    CausalStateFact::ActiveGoal
                ))
            );
        }
    }

    #[test]
    fn m002_contradictory_state_does_not_panic() {
        // All 17 facts at once (including mutually tense acceptance and
        // error facts) evaluates without panic and stays within authority.
        let all_facts: BTreeSet<CausalStateFact> = CausalStateFact::all().into_iter().collect();
        let eligible: BTreeSet<String> = causal_catalog().keys().cloned().collect();
        let catalog = causal_catalog();
        let inputs = m002_frontier_inputs(&eligible, &all_facts, &catalog);
        let frontier = CausalFrontier::evaluate(&inputs).expect("contradictory state evaluates");
        for tool in frontier
            .admissible_contracted
            .iter()
            .chain(frontier.uncontracted_fallback.iter())
        {
            assert!(eligible.contains(tool));
        }
    }

    #[test]
    fn m002_surface_order_does_not_affect_frontier() {
        // Frontier input is set-ordered: shuffling the eligible Vec cannot
        // change the result.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let case = &cases[0];
        let ordered = evaluate_benchmark_case(case, &catalog, &fingerprint).expect("evaluates");
        let mut shuffled = case.clone();
        shuffled.eligible.reverse();
        let from_shuffled =
            evaluate_benchmark_case(&shuffled, &catalog, &fingerprint).expect("evaluates");
        assert_eq!(ordered, from_shuffled);
    }

    #[test]
    fn m002_fingerprint_drift_invalidates_frontier() {
        // State or catalog drift invalidates the cached frontier instead of
        // silently reusing it.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let case = &cases[0];
        let frontier = evaluate_benchmark_case(case, &catalog, &fingerprint).expect("evaluates");
        let state_fp = benchmark_case_state_fingerprint(case);
        assert!(frontier.is_fresh_against(&state_fp, &fingerprint));
        assert!(!frontier.is_fresh_against("drifted-state", &fingerprint));
        assert!(!frontier.is_fresh_against(&state_fp, "drifted-catalog"));
    }

    #[test]
    fn m002_withheld_tool_fails_closed() {
        // A withheld tool reaching frontier input is an error, never a
        // promotion; frontier output never contains withheld names.
        let mut case = load_causal_benchmark(BENCHMARK_JSONL).expect("loads")[0].clone();
        case.withheld.push("read".to_string());
        let catalog = causal_catalog();
        let fingerprint = causal_catalog_fingerprint();
        assert_eq!(
            evaluate_benchmark_case(&case, &catalog, &fingerprint),
            Err(CausalFrontierError::WithheldInSurface("read".to_string()))
        );
    }

    #[test]
    fn m002_frontier_carries_no_provider_definitions() {
        // The frontier serializes to canonical names and fingerprints only:
        // no definitions, descriptions, scores, or provider payloads.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let case = &cases[0];
        let frontier = evaluate_benchmark_case(case, &catalog, &fingerprint).expect("evaluates");
        let value = serde_json::to_value(&frontier).expect("frontier serializes");
        let keys: BTreeSet<&str> = value
            .as_object()
            .expect("frontier is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "admissible_contracted",
                "contract_catalog_fingerprint",
                "inadmissible_contracted",
                "required_visible",
                "schema_version",
                "snapshot_fingerprint",
                "uncontracted_fallback",
            ]
            .into_iter()
            .collect::<BTreeSet<&str>>()
        );
    }

    #[test]
    fn m002_dev_baselines_report() {
        // Dev baselines (arm 1 core-palette projection, arm 2 full eligible
        // universe) plus the M002 arm reproduce the frozen expectations:
        // full gold preservation, full premature reduction, median deferred
        // promotion 3.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let (dev, _) = m002_split(&cases);
        assert_eq!(dev.len(), 112);
        let report = score_partition(&dev, &catalog, &fingerprint).expect("dev scores");
        assert_eq!(report.cases, 112);
        assert_eq!(report.gold_current_visible, report.gold_current_total);
        assert_eq!(report.gold_current_step_preservation, 1.0);
        assert_eq!(report.premature_remaining, 0);
        assert_eq!(report.premature_exposure_reduction, 1.0);
        assert_eq!(
            report.uncontracted_gold_retained,
            report.uncontracted_gold_total
        );
        assert_eq!(report.authority_violations, 0);
        assert_eq!(report.abstention_violations, 0);
        assert_eq!(report.median_promotion_structured, 3);
        // Every dev family preserves its gold tools and exposes no
        // premature tools after filtering.
        for family in &report.families {
            assert_eq!(family.gold_current_visible, family.gold_current_total);
            assert_eq!(
                family.premature_remaining, 0,
                "family {} exposes premature tools",
                family.family
            );
        }
        // Core-palette projection is populated (baseline arm 1 measures a
        // real visible-tool count, not an empty scope).
        assert!(report.core_projection_sizes.iter().all(|size| *size > 0));
    }

    #[test]
    fn m002_qualification_gates_freeze_disposition_a() {
        // The untouched qualification partition scores once, through the
        // same deterministic evaluation, and every frozen gate passes:
        // disposition A.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let (_, qual) = m002_split(&cases);
        assert_eq!(qual.len(), 56);
        let report = score_partition(&qual, &catalog, &fingerprint).expect("qual scores");
        assert_eq!(report.gold_current_step_preservation, 1.0);
        assert_eq!(report.authority_violations, 0);
        assert_eq!(report.uncontracted_fallback_preservation, 1.0);
        assert_eq!(report.premature_exposure_reduction, 1.0);
        assert!(report.median_promotion_structured <= 4);
        assert_eq!(report.abstention_violations, 0);
        assert_eq!(
            decide_disposition(&report, &CausalM002Gates::m001_frozen(), 0.01),
            CausalM002Disposition::Positive
        );
        // No state family loses a gold tool on qualification either.
        for family in &report.families {
            assert_eq!(
                family.gold_current_visible, family.gold_current_total,
                "family {} loses gold on qual",
                family.family
            );
        }
    }

    #[test]
    fn m002_frontier_eval_p95_within_budget() {
        // Pure frontier evaluation over every qualification surface, 1001
        // iterations each, stays within the 5 ms p95 budget.
        let (cases, catalog, fingerprint) = m002_benchmark_inputs();
        let (_, qual) = m002_split(&cases);
        let samples = measure_frontier_latency_ms(
            &qual,
            &catalog,
            &fingerprint,
            CAUSAL_M002_LATENCY_ITERATIONS,
        );
        assert_eq!(samples.len(), qual.len() * CAUSAL_M002_LATENCY_ITERATIONS);
        assert!(
            nearest_rank_percentile(&samples, 0.95) <= 5.0,
            "p95 latency exceeds the frozen 5 ms budget"
        );
    }

    #[test]
    fn m002_retrieval_label_diagnostic_reports_frozen_labels() {
        // Every distinct current-step label in the frozen relevance view is
        // classified; counts reconcile exactly.
        const RELEVANCE_JSON: &str =
            include_str!("../../assets/tool-advisor/retrieval-relevance-v1.json");
        let candidates =
            historical_label_candidates(RELEVANCE_JSON).expect("relevance view parses");
        assert!(!candidates.is_empty());
        let registry = crate::tool::ToolRegistry::with_defaults();
        let known: BTreeSet<String> = registry
            .list()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect();
        let diagnostic = diagnose_retrieval_labels(&candidates, |name| known.contains(name));
        assert_eq!(
            diagnostic.admissible_any_state
                + diagnostic.contracted_state_gated
                + diagnostic.uncontracted_known
                + diagnostic.unavailable_unknown,
            candidates.len()
        );
        assert_eq!(diagnostic.labels.len(), candidates.len());
        // Spot checks against the live native table. Corpus labels use
        // historical tool identities, so the unknown-identity path is
        // exercised directly with a synthetic name.
        assert_eq!(
            diagnostic.labels.get("read"),
            Some(&HistoricalLabelClass::AdmissibleAnyState)
        );
        assert_eq!(
            diagnostic.labels.get("goal_get"),
            Some(&HistoricalLabelClass::ContractedStateGated)
        );
        assert_eq!(
            classify_historical_label("bash", |name| known.contains(name)),
            HistoricalLabelClass::UncontractedKnown
        );
        assert_eq!(
            classify_historical_label("tool_x99_synthetic", |name| known.contains(name)),
            HistoricalLabelClass::UnavailableUnknown
        );
    }

    #[test]
    fn m002_qualify_reports_disposition_a() {
        // End-to-end offline qualification over the frozen assets decides
        // disposition A with every gate result true.
        const RELEVANCE_JSON: &str =
            include_str!("../../assets/tool-advisor/retrieval-relevance-v1.json");
        let report = qualify_m002(
            BENCHMARK_JSONL,
            PREREG_JSON,
            RELEVANCE_JSON,
            CAUSAL_M002_LATENCY_ITERATIONS,
        )
        .expect("m002 qualifies");
        assert_eq!(report.disposition, "A");
        assert_eq!(report.dev.cases, 112);
        assert_eq!(report.qualification.cases, 56);
        assert!(report.gate_results.values().all(|passed| *passed));
        assert!(report.latency_p95_ms <= report.gates.pure_frontier_eval_p95_ms_max);
        assert_eq!(report.latency_samples, 56 * CAUSAL_M002_LATENCY_ITERATIONS);
    }

    #[test]
    fn m002_checked_in_receipt_matches_recomputation() {
        // The checked-in receipt is byte-identical in every deterministic
        // field to live recomputation; only the environment-sensitive
        // latency measurements are compared by gate rather than equality.
        const RELEVANCE_JSON: &str =
            include_str!("../../assets/tool-advisor/retrieval-relevance-v1.json");
        const RECEIPT_JSON: &str =
            include_str!("../../assets/tool-advisor/causal-frontier-m002-result.json");
        let stored: CausalM002Report =
            serde_json::from_str(RECEIPT_JSON).expect("stored receipt parses");
        let fresh = qualify_m002(
            BENCHMARK_JSONL,
            PREREG_JSON,
            RELEVANCE_JSON,
            CAUSAL_M002_LATENCY_ITERATIONS,
        )
        .expect("m002 requalifies");
        assert_eq!(stored.schema_version, fresh.schema_version);
        assert_eq!(stored.protocol, fresh.protocol);
        assert_eq!(stored.ontology_version, fresh.ontology_version);
        assert_eq!(
            stored.contract_schema_version,
            fresh.contract_schema_version
        );
        assert_eq!(stored.benchmark_fingerprint, fresh.benchmark_fingerprint);
        assert_eq!(
            stored.contract_catalog_fingerprint,
            fresh.contract_catalog_fingerprint
        );
        assert_eq!(stored.palette_fingerprint, fresh.palette_fingerprint);
        assert_eq!(stored.dev_fingerprint, fresh.dev_fingerprint);
        assert_eq!(
            stored.qualification_fingerprint,
            fresh.qualification_fingerprint
        );
        assert_eq!(stored.dev, fresh.dev);
        assert_eq!(stored.qualification, fresh.qualification);
        assert_eq!(stored.gates, fresh.gates);
        assert_eq!(stored.gate_results, fresh.gate_results);
        assert_eq!(stored.disposition, fresh.disposition);
        assert_eq!(
            stored.retrieval_label_diagnostic,
            fresh.retrieval_label_diagnostic
        );
        assert_eq!(
            stored.latency_iterations_per_surface,
            fresh.latency_iterations_per_surface
        );
        assert_eq!(stored.latency_samples, fresh.latency_samples);
        assert!(stored.latency_p95_ms <= stored.gates.pure_frontier_eval_p95_ms_max);
        assert!(fresh.latency_p95_ms <= fresh.gates.pure_frontier_eval_p95_ms_max);
    }

    /// Regenerate the checked-in M002 receipt. Ignored by default: run
    /// explicitly after reviewing contract, benchmark, or palette changes.
    #[test]
    #[ignore]
    fn m002_regenerate_checked_in_receipt() {
        const RELEVANCE_JSON: &str =
            include_str!("../../assets/tool-advisor/retrieval-relevance-v1.json");
        let report = qualify_m002(
            BENCHMARK_JSONL,
            PREREG_JSON,
            RELEVANCE_JSON,
            CAUSAL_M002_LATENCY_ITERATIONS,
        )
        .expect("m002 qualifies for receipt regeneration");
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CAUSAL_M002_RESULT_ASSET);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&report).expect("receipt serializes"),
        )
        .expect("receipt writes");
        eprintln!("regenerated {}", path.display());
    }

    // ─── M003 structured effect-path frontier ────────────────────────────

    fn m003_structured_cases() -> Vec<CausalBenchmarkCase> {
        load_causal_benchmark(BENCHMARK_JSONL)
            .expect("frozen benchmark loads")
            .into_iter()
            .filter(|case| case.desired_outcome.is_some())
            .collect()
    }

    fn m003_synth_contract(
        requires_all: &[CausalStateFact],
        produces: &[CausalOutcome],
    ) -> ToolCausalContract {
        ToolCausalContract {
            schema_version: CAUSAL_CONTRACT_SCHEMA_VERSION,
            requires_all: requires_all.iter().copied().collect(),
            requires_any: Vec::new(),
            forbids: BTreeSet::new(),
            produces: produces.iter().copied().collect(),
            provenance: CausalContractProvenance::new(
                "static:test-synthetic",
                "synthetic planner bound test",
            ),
        }
    }

    #[test]
    fn m003_demand_mapping_covers_typed_sources() {
        // Every allowed host-owned demand source derives its outcome; kinds
        // without an outcome mapping abstain.
        let valid = [
            (
                WorkEvidenceKind::TestJob,
                CausalOutcome::TestEvidenceProduced,
            ),
            (
                WorkEvidenceKind::Commit,
                CausalOutcome::CommitEvidenceProduced,
            ),
            (WorkEvidenceKind::Artifact, CausalOutcome::ArtifactExpanded),
            (
                WorkEvidenceKind::DelegatedRun,
                CausalOutcome::DelegatedRunProduced,
            ),
            (
                WorkEvidenceKind::AgentRun,
                CausalOutcome::DelegatedRunProduced,
            ),
        ];
        let mut template = m003_structured_cases()
            .into_iter()
            .next()
            .expect("structured cases exist");
        for (evidence, outcome) in valid {
            template.desired_outcome = Some(CausalDesiredOutcome {
                kind: outcome,
                evidence_kind: Some(evidence),
            });
            assert_eq!(
                derive_structured_demand(&template).expect("typed demand derives"),
                CausalDesiredOutcome {
                    kind: outcome,
                    evidence_kind: Some(evidence),
                }
            );
        }
        template.desired_outcome = Some(CausalDesiredOutcome {
            kind: CausalOutcome::DelegatedRunProduced,
            evidence_kind: Some(WorkEvidenceKind::SchedulerJob),
        });
        assert_eq!(
            derive_structured_demand(&template),
            Err(NoStructuredDemandReason::UnsupportedEvidenceKind {
                evidence: "scheduler_job".to_string(),
            })
        );
        template.desired_outcome = Some(CausalDesiredOutcome {
            kind: CausalOutcome::WorkspaceMutationProduced,
            evidence_kind: None,
        });
        assert_eq!(
            derive_structured_demand(&template),
            Err(NoStructuredDemandReason::UnsupportedEvidenceKind {
                evidence: "none".to_string(),
            })
        );
        template.desired_outcome = Some(CausalDesiredOutcome {
            kind: CausalOutcome::CommitEvidenceProduced,
            evidence_kind: Some(WorkEvidenceKind::TestJob),
        });
        assert_eq!(
            derive_structured_demand(&template),
            Err(NoStructuredDemandReason::EvidenceMappingMismatch {
                outcome: "commit_evidence_produced".to_string(),
                evidence: "test_job".to_string(),
            })
        );
    }

    #[test]
    fn m003_demand_ignores_free_text() {
        // The no-goal/plan rationale mentions a "test surface", but without
        // a typed demand M003 must abstain: prose never creates a demand.
        let case = load_causal_benchmark(BENCHMARK_JSONL)
            .expect("frozen benchmark loads")
            .into_iter()
            .find(|case| case.case_id == "cf-v1-no_goal_plan-001")
            .expect("no-goal case exists");
        assert!(case.desired_outcome.is_none());
        assert!(case.rationale.contains("test"));
        assert_eq!(
            derive_structured_demand(&case),
            Err(NoStructuredDemandReason::NoDesiredOutcome)
        );
    }

    #[test]
    fn m003_frozen_demands_all_validate() {
        // Every frozen structured demand in the M001 benchmark carries a
        // consistent typed outcome/evidence pair.
        let cases = m003_structured_cases();
        assert_eq!(cases.len(), 54);
        for case in &cases {
            derive_structured_demand(case).expect("frozen demand must validate");
        }
    }

    #[test]
    fn m003_derive_unmet_demands_from_inputs() {
        let empty = CausalStateInputs::default();
        assert!(derive_unmet_demands(&empty).is_empty());
        let inputs = CausalStateInputs {
            unmet_test_acceptance: true,
            unmet_commit_acceptance: true,
            unmet_delegated_run_acceptance: true,
            unmet_artifact_acceptance: true,
            ..Default::default()
        };
        let demands = derive_unmet_demands(&inputs);
        let kinds: Vec<CausalOutcome> = demands.iter().map(|demand| demand.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CausalOutcome::TestEvidenceProduced,
                CausalOutcome::CommitEvidenceProduced,
                CausalOutcome::DelegatedRunProduced,
                CausalOutcome::ArtifactExpanded,
            ]
        );
        let order: Vec<CausalOutcome> = CausalOutcome::all().into_iter().collect();
        let mut positions: Vec<usize> = kinds
            .iter()
            .map(|kind| order.iter().position(|o| o == kind).expect("known outcome"))
            .collect();
        let sorted = {
            let mut ascending = positions.clone();
            ascending.sort_unstable();
            ascending
        };
        assert_eq!(positions, sorted);
        positions.clear();
    }

    #[test]
    fn m003_preview_demand_armed_only() {
        assert_eq!(derive_preview_demand(false), None);
        assert_eq!(
            derive_preview_demand(true),
            Some(CausalDesiredOutcome {
                kind: CausalOutcome::WorkspaceMutationProduced,
                evidence_kind: None,
            })
        );
    }

    #[test]
    fn m003_effect_catalog_binds_implementation_identity() {
        // The effect fingerprint is stable, differs from the payload-only
        // catalog fingerprint, and moves with implementation/schema
        // identity through the bound-contract seam.
        assert_eq!(effect_catalog_fingerprint(), effect_catalog_fingerprint());
        assert_ne!(effect_catalog_fingerprint(), causal_catalog_fingerprint());
        let contract = native_causal_contract("test").expect("pilot contract exists");
        let schema = serde_json::json!({"type": "object"});
        let first =
            bind_causal_contract("test", &contract, "codegg/test", "1", &schema).expect("binds");
        let impl_drift =
            bind_causal_contract("test", &contract, "codegg/test", "2", &schema).expect("binds");
        assert_ne!(first.fingerprint, impl_drift.fingerprint);
        let schema_drift = bind_causal_contract(
            "test",
            &contract,
            "codegg/test",
            "1",
            &serde_json::json!({"type": "object", "extra": true}),
        )
        .expect("binds");
        assert_ne!(first.fingerprint, schema_drift.fingerprint);
    }

    #[test]
    fn m003_all_pilot_tools_resolve_live() {
        assert!(effect_catalog_binding_complete());
    }

    #[test]
    fn m003_planner_finds_direct_producer() {
        let catalog = causal_catalog();
        let admissible: BTreeSet<String> = NATIVE_PILOT_TOOLS
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        let demand = |kind, evidence| CausalDesiredOutcome {
            kind,
            evidence_kind: evidence,
        };
        assert_eq!(
            plan_effect_path(
                &demand(
                    CausalOutcome::TestEvidenceProduced,
                    Some(WorkEvidenceKind::TestJob)
                ),
                &BTreeSet::new(),
                &admissible,
                &catalog,
            ),
            Some(vec!["test".to_string()])
        );
        let mut facts = BTreeSet::new();
        facts.insert(CausalStateFact::UnmetCommitAcceptance);
        assert_eq!(
            plan_effect_path(
                &demand(
                    CausalOutcome::CommitEvidenceProduced,
                    Some(WorkEvidenceKind::Commit)
                ),
                &facts,
                &admissible,
                &catalog,
            ),
            Some(vec!["commit".to_string()])
        );
        let mut artifact_facts = BTreeSet::new();
        artifact_facts.insert(CausalStateFact::ArtifactHandleAvailable);
        assert_eq!(
            plan_effect_path(
                &demand(
                    CausalOutcome::ArtifactExpanded,
                    Some(WorkEvidenceKind::Artifact)
                ),
                &artifact_facts,
                &admissible,
                &catalog,
            ),
            Some(vec!["context_read".to_string()])
        );
        assert_eq!(
            plan_effect_path(
                &demand(
                    CausalOutcome::DelegatedRunProduced,
                    Some(WorkEvidenceKind::DelegatedRun)
                ),
                &BTreeSet::new(),
                &admissible,
                &catalog,
            ),
            Some(vec!["task".to_string()])
        );
    }

    #[test]
    fn m003_planner_bounds_depth_no_repeat_lexicographic() {
        use CausalOutcome::{
            CommitEvidenceProduced, FilesInspected, PathsDiscovered, SymbolFactsProduced,
            TestEvidenceProduced, TextMatchesProduced,
        };
        use CausalStateFact::{
            ActiveGoal, FailedTestEvidence, TestEvidenceAvailable, TouchedFilesAvailable,
        };
        // Lexicographic tie-break between two direct producers.
        let mut tied = BTreeMap::new();
        tied.insert(
            "beta".to_string(),
            m003_synth_contract(&[], &[FilesInspected]),
        );
        tied.insert(
            "alpha".to_string(),
            m003_synth_contract(&[], &[FilesInspected]),
        );
        assert_eq!(
            find_minimal_effect_path(
                FilesInspected,
                &BTreeSet::new(),
                &tied,
                &BTreeMap::new(),
                CAUSAL_M003_MAX_PATH_DEPTH,
            ),
            Some(vec!["alpha".to_string()])
        );
        // Two-step chain through synthetic enablement; depth 1 is blind.
        let mut chain = BTreeMap::new();
        chain.insert(
            "t_first".to_string(),
            m003_synth_contract(&[ActiveGoal], &[TestEvidenceProduced]),
        );
        chain.insert(
            "t_second".to_string(),
            m003_synth_contract(&[TouchedFilesAvailable], &[CommitEvidenceProduced]),
        );
        let mut enablement: BTreeMap<CausalOutcome, BTreeSet<CausalStateFact>> = BTreeMap::new();
        enablement.insert(
            TestEvidenceProduced,
            BTreeSet::from([TouchedFilesAvailable]),
        );
        let mut start = BTreeSet::new();
        start.insert(ActiveGoal);
        assert_eq!(
            find_minimal_effect_path(
                CommitEvidenceProduced,
                &start,
                &chain,
                &enablement,
                CAUSAL_M003_MAX_PATH_DEPTH,
            ),
            Some(vec!["t_first".to_string(), "t_second".to_string()])
        );
        assert_eq!(
            find_minimal_effect_path(CommitEvidenceProduced, &start, &chain, &enablement, 1,),
            None
        );
        // Four-step chain exceeds the frozen depth cap but not a deeper
        // one; cycles terminate without repeating a tool.
        let mut long = BTreeMap::new();
        long.insert(
            "c1".to_string(),
            m003_synth_contract(&[ActiveGoal], &[FilesInspected]),
        );
        long.insert(
            "c2".to_string(),
            m003_synth_contract(&[TouchedFilesAvailable], &[PathsDiscovered]),
        );
        long.insert(
            "c3".to_string(),
            m003_synth_contract(&[TestEvidenceAvailable], &[TextMatchesProduced]),
        );
        long.insert(
            "c4".to_string(),
            m003_synth_contract(&[FailedTestEvidence], &[SymbolFactsProduced]),
        );
        let mut long_enable: BTreeMap<CausalOutcome, BTreeSet<CausalStateFact>> = BTreeMap::new();
        long_enable.insert(FilesInspected, BTreeSet::from([TouchedFilesAvailable]));
        long_enable.insert(PathsDiscovered, BTreeSet::from([TestEvidenceAvailable]));
        long_enable.insert(TextMatchesProduced, BTreeSet::from([FailedTestEvidence]));
        long_enable.insert(SymbolFactsProduced, BTreeSet::from([ActiveGoal]));
        assert_eq!(
            find_minimal_effect_path(
                SymbolFactsProduced,
                &start,
                &long,
                &long_enable,
                CAUSAL_M003_MAX_PATH_DEPTH,
            ),
            None
        );
        assert_eq!(
            find_minimal_effect_path(SymbolFactsProduced, &start, &long, &long_enable, 4,),
            Some(vec![
                "c1".to_string(),
                "c2".to_string(),
                "c3".to_string(),
                "c4".to_string(),
            ])
        );
        assert_eq!(
            find_minimal_effect_path(
                FilesInspected,
                &BTreeSet::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                CAUSAL_M003_MAX_PATH_DEPTH,
            ),
            None
        );
        assert_eq!(
            find_minimal_effect_path(FilesInspected, &BTreeSet::new(), &tied, &BTreeMap::new(), 0,),
            None
        );
    }

    #[test]
    fn m003_unknown_tools_outside_graph() {
        // Uncontracted tools can never appear on a path, even when they
        // are admissible members of the eligible surface.
        let catalog = causal_catalog();
        let demand = CausalDesiredOutcome {
            kind: CausalOutcome::TestEvidenceProduced,
            evidence_kind: Some(WorkEvidenceKind::TestJob),
        };
        let admissible: BTreeSet<String> = ["bash".to_string(), "test".to_string()]
            .into_iter()
            .collect();
        assert_eq!(
            plan_effect_path(&demand, &BTreeSet::new(), &admissible, &catalog),
            Some(vec!["test".to_string()])
        );
        let uncontracted_only: BTreeSet<String> = ["bash".to_string()].into_iter().collect();
        assert_eq!(
            plan_effect_path(&demand, &BTreeSet::new(), &uncontracted_only, &catalog),
            None
        );
        // No frozen benchmark path ever leaves the native pilot catalog.
        for case in m003_structured_cases() {
            let frontier = evaluate_benchmark_case(&case, &catalog, &causal_catalog_fingerprint())
                .expect("benchmark surfaces evaluate");
            if let Some(path) = plan_effect_path(
                &derive_structured_demand(&case).expect("frozen demand validates"),
                &case.facts,
                &frontier.admissible_contracted,
                &catalog,
            ) {
                for step in path {
                    assert!(
                        NATIVE_PILOT_TOOLS.contains(&step.as_str()),
                        "path tool {step} is outside the native catalog"
                    );
                }
            }
        }
    }

    #[test]
    fn m003_no_admissible_producer_falls_back() {
        // A typed commit demand with a recorded unresolved error makes the
        // only producer inadmissible: M003 abstains instead of hiding M002.
        let catalog = causal_catalog();
        let mut case = m003_structured_cases()
            .into_iter()
            .find(|case| {
                case.desired_outcome
                    .as_ref()
                    .is_some_and(|desired| desired.kind == CausalOutcome::CommitEvidenceProduced)
            })
            .expect("commit-demand case exists");
        case.facts.insert(CausalStateFact::UnresolvedError);
        let (status, _) =
            evaluate_effect_path(&case, &catalog, "contract-fp", "effect-fp", "effect-fp")
                .expect("evaluates");
        match status {
            EffectPathStatus::NoAdmissiblePath { demand } => {
                assert_eq!(demand.kind, CausalOutcome::CommitEvidenceProduced);
            }
            other => panic!(
                "expected no-admissible-path abstention, got {}",
                other.code()
            ),
        }
    }

    #[test]
    fn m003_stale_catalog_fails_closed() {
        let catalog = causal_catalog();
        let case = m003_structured_cases()
            .into_iter()
            .next()
            .expect("structured cases exist");
        let (status, _) =
            evaluate_effect_path(&case, &catalog, "contract-fp", "live-fp", "expected-fp")
                .expect("evaluates");
        assert_eq!(status.code(), "stale_catalog_fallback");
        assert_eq!(status.used_size(7), 7);
    }

    #[test]
    fn m003_withheld_fails_closed() {
        let catalog = causal_catalog();
        let mut case = m003_structured_cases()
            .into_iter()
            .next()
            .expect("structured cases exist");
        case.eligible.push("denied_shell_tool".to_string());
        let err = evaluate_effect_path(&case, &catalog, "contract-fp", "fp", "fp")
            .expect_err("withheld input must fail");
        assert_eq!(
            err,
            EffectPathError::WithheldInSurface("denied_shell_tool".to_string())
        );
    }

    #[test]
    fn m003_result_carries_provenance_and_fingerprints() {
        // The advisory result binds the demand, the contract provenance of
        // every path tool, and both fingerprints for freshness checks.
        let catalog = causal_catalog();
        let case = m003_structured_cases()
            .into_iter()
            .find(|case| {
                case.desired_outcome
                    .as_ref()
                    .is_some_and(|desired| desired.kind == CausalOutcome::TestEvidenceProduced)
            })
            .expect("test-demand case exists");
        let (status, _) =
            evaluate_effect_path(&case, &catalog, "contract-fp", "effect-fp", "effect-fp")
                .expect("evaluates");
        match status {
            EffectPathStatus::Path(frontier) => {
                assert_eq!(frontier.path, vec!["test".to_string()]);
                assert_eq!(
                    frontier.tool_provenance.get("test").map(String::as_str),
                    Some("static:M001-pilot-native")
                );
                assert_eq!(
                    frontier.snapshot_fingerprint,
                    benchmark_case_state_fingerprint(&case)
                );
                assert_eq!(frontier.contract_catalog_fingerprint, "contract-fp");
                assert_eq!(frontier.effect_catalog_fingerprint, "effect-fp");
                assert!(frontier
                    .is_fresh_against(&benchmark_case_state_fingerprint(&case), "effect-fp"));
                assert!(!frontier
                    .is_fresh_against(&benchmark_case_state_fingerprint(&case), "other-fp"));
            }
            other => panic!("expected a path, got {}", other.code()),
        }
    }

    #[test]
    fn m003_end_to_end_negative_on_frozen_benchmark() {
        // Full offline qualification over the frozen assets: every
        // structured demand finds a length-1 path, the caller-visible
        // median drops from 3 to 1, premature exposure stays at zero, but
        // narrowing to demand producers hides plan/goal-state gold tools,
        // so M003 closes negative and M004 keeps the M002 frontier.
        let report = qualify_m003(BENCHMARK_JSONL, PREREG_JSON, CAUSAL_M003_LATENCY_ITERATIONS)
            .expect("m003 qualifies");
        assert_eq!(report.structured_dev_cases, 36);
        assert_eq!(report.structured_qualification_cases, 18);
        assert_eq!(report.dev.cases, 36);
        assert_eq!(report.qualification.cases, 18);
        assert_eq!(report.dev.path_found, 36);
        assert_eq!(report.qualification.path_found, 18);
        assert_eq!(report.qualification.no_admissible_path, 0);
        assert_eq!(report.qualification.abstentions, 0);
        assert_eq!(report.qualification.median_path_length, 1);
        assert_eq!(report.qualification.max_path_length, 1);
        assert_eq!(report.qualification.median_m002_promotion, 3);
        assert_eq!(report.qualification.median_m003_used, 1);
        assert!(report.reduction_holds_exact);
        assert!(report.qualification.gold_current_step_preservation_m003 < 1.0);
        assert!(!report.qualification.families_losing_gold_tool.is_empty());
        assert!(!report
            .qualification
            .families_losing_gold_tool
            .contains(&"artifact_recovery".to_string()));
        assert_eq!(report.qualification.premature_m003_remaining, 0);
        assert!(
            report.qualification.premature_m003_remaining
                <= report.qualification.premature_m002_remaining
        );
        assert_eq!(
            report.qualification.contract_demands_covered,
            report.qualification.contract_demands_total
        );
        assert!(report.catalog_binding_complete);
        assert_eq!(report.disposition, "D");
        assert!(!report.gate_results["gold_current_step_preservation"]);
        assert!(!report.gate_results["no_family_loses_gold_tool"]);
        assert!(report.gate_results["median_used_reduction"]);
        assert!(report.gate_results["premature_non_increasing"]);
        assert!(report.gate_results["contract_coverage"]);
        assert!(report.gate_results["authority_violations"]);
        assert!(report.gate_results["uncontracted_fallback_preservation"]);
        assert!(report.gate_results["pure_effect_path_p95_ms"]);
        assert!(report.latency_p95_ms <= report.gates.pure_effect_path_p95_ms_max);
        assert_eq!(report.latency_samples, 18 * CAUSAL_M003_LATENCY_ITERATIONS);
    }

    #[test]
    fn m003_checked_in_receipt_matches_recomputation() {
        // The checked-in receipt is identical in every deterministic field
        // to live recomputation; only the environment-sensitive latency
        // measurements are compared by gate rather than equality.
        const RECEIPT_JSON: &str =
            include_str!("../../assets/tool-advisor/causal-frontier-m003-result.json");
        let stored: CausalM003Report =
            serde_json::from_str(RECEIPT_JSON).expect("stored receipt parses");
        let fresh = qualify_m003(BENCHMARK_JSONL, PREREG_JSON, CAUSAL_M003_LATENCY_ITERATIONS)
            .expect("m003 requalifies");
        assert_eq!(stored.schema_version, fresh.schema_version);
        assert_eq!(stored.protocol, fresh.protocol);
        assert_eq!(stored.ontology_version, fresh.ontology_version);
        assert_eq!(
            stored.contract_schema_version,
            fresh.contract_schema_version
        );
        assert_eq!(
            stored.effect_path_schema_version,
            fresh.effect_path_schema_version
        );
        assert_eq!(stored.benchmark_fingerprint, fresh.benchmark_fingerprint);
        assert_eq!(
            stored.contract_catalog_fingerprint,
            fresh.contract_catalog_fingerprint
        );
        assert_eq!(
            stored.effect_catalog_fingerprint,
            fresh.effect_catalog_fingerprint
        );
        assert_eq!(
            stored.catalog_binding_complete,
            fresh.catalog_binding_complete
        );
        assert_eq!(stored.dev_fingerprint, fresh.dev_fingerprint);
        assert_eq!(
            stored.qualification_fingerprint,
            fresh.qualification_fingerprint
        );
        assert_eq!(stored.structured_dev_cases, fresh.structured_dev_cases);
        assert_eq!(
            stored.structured_qualification_cases,
            fresh.structured_qualification_cases
        );
        assert_eq!(stored.dev, fresh.dev);
        assert_eq!(stored.qualification, fresh.qualification);
        assert_eq!(stored.reduction_holds_exact, fresh.reduction_holds_exact);
        assert_eq!(stored.gates, fresh.gates);
        assert_eq!(stored.gate_results, fresh.gate_results);
        assert_eq!(stored.disposition, fresh.disposition);
        assert_eq!(
            stored.latency_iterations_per_surface,
            fresh.latency_iterations_per_surface
        );
        assert_eq!(stored.latency_samples, fresh.latency_samples);
        assert!(stored.latency_p95_ms <= stored.gates.pure_effect_path_p95_ms_max);
        assert!(fresh.latency_p95_ms <= fresh.gates.pure_effect_path_p95_ms_max);
    }

    /// Regenerate the checked-in M003 receipt. Ignored by default: run
    /// explicitly after reviewing the frozen benchmark or catalog inputs.
    #[test]
    #[ignore]
    fn m003_regenerate_checked_in_receipt() {
        let report = qualify_m003(BENCHMARK_JSONL, PREREG_JSON, CAUSAL_M003_LATENCY_ITERATIONS)
            .expect("m003 qualifies for receipt regeneration");
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CAUSAL_M003_RESULT_ASSET);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&report).expect("receipt serializes"),
        )
        .expect("receipt writes");
        eprintln!("regenerated {}", path.display());
    }
}
