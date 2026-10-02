//! Typed causal contracts, host-owned state projection, and the frozen
//! stateful benchmark for the tool-selection advisor causal-frontier
//! experiment (M001).
//!
//! This module is the smallest trustworthy substrate for causal tool-menu
//! experiments. It changes **no runtime disclosure behavior**: nothing here
//! filters provider definitions, alters broker authorization, or widens
//! execution authority. The causal frontier is a visibility-only planning
//! layer evaluated in M002 against the frozen benchmark preregistered here.
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
}
