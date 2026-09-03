//! Durable, host-owned state for bounded produce/verify convergence cycles.
//!
//! This module is deliberately a coordination record, not an executor.  It
//! stores the accepted convergence specification and references to existing
//! agent runs/groups, while admission, worktrees, run results, and goal
//! completion remain owned by their existing services.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::fmt;
use thiserror::Error;
use tokio::sync::Mutex;

use super::agent_run::AgentRunStatus;
use super::agent_run_group::RunGroupStatus;
use crate::identity::{AgentRunGroupId, AgentRunId};
use crate::run_result::{
    AgentRunArtifact, AgentRunFinding, AgentRunResult, AgentRunResultStatus, RepositoryState,
    ValidationEvidence,
};

pub const MAX_OBJECTIVE_BYTES: usize = 8 * 1024;
pub const MAX_CRITERIA: usize = 32;
pub const MAX_CRITERION_BYTES: usize = 1024;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 512;
pub const MAX_CONVERGENCE_STRING_BYTES: usize = 4096;
pub const MAX_SPEC_BYTES: usize = 64 * 1024;
pub const MAX_DIGEST_BYTES: usize = 64;
pub const MAX_CYCLES: u8 = 4;
pub const MAX_PRODUCER_RUNS: usize = 16;
pub const MAX_EVIDENCE_REFS: usize = 64;
pub const MAX_REPAIR_REQUESTS: usize = 32;
pub const MAX_VERIFIER_PACKET_BYTES: usize = 1024 * 1024;
pub const MAX_CONVERGENCE_LIST: usize = 100;
/// Delimiter required around the verifier's host-owned structured result.
/// Plain prose is never interpreted as a semantic pass.
pub const VERDICT_MARKER: &str = "convergence_verdict";

/// A service-local durable convergence handle.  It is not interchangeable
/// with an agent run, run group, job, turn, or goal identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConvergenceId(String);

/// Alias retained for callers that use the subsystem's longer domain name.
pub type AgentConvergenceId = ConvergenceId;

impl ConvergenceId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn parse(value: &str) -> Result<Self, ConvergenceError> {
        if value.is_empty()
            || value.len() > 128
            || value.contains('/')
            || value.contains('\\')
            || value.bytes().any(|byte| byte == 0)
            || value.chars().any(char::is_control)
            || value.chars().any(char::is_whitespace)
            || value
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
        {
            return Err(ConvergenceError::InvalidInput(
                "convergence id is not a bounded opaque identity".into(),
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ConvergenceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConvergenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ConvergenceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConvergenceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// The exact caller-owned orchestration scope.  A turn-owned convergence does
/// not manufacture a root AgentRun merely to fill this field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AgentOrchestrationOwner {
    Turn { session_id: String, turn_id: String },
    Run { run_id: AgentRunId },
}

impl AgentOrchestrationOwner {
    fn validate(&self) -> Result<(), ConvergenceError> {
        match self {
            Self::Turn {
                session_id,
                turn_id,
            } => {
                validate_string(session_id, "owner session id", 128)?;
                validate_string(turn_id, "owner turn id", 128)?;
            }
            Self::Run { .. } => {}
        }
        Ok(())
    }

    fn fingerprint_fragment(&self) -> String {
        match self {
            Self::Turn {
                session_id,
                turn_id,
            } => format!("turn:{session_id}:{turn_id}"),
            Self::Run { run_id } => format!("run:{run_id}"),
        }
    }

    fn sqlite_parts(&self) -> (&'static str, Option<&str>, Option<&str>, Option<&str>) {
        match self {
            Self::Turn {
                session_id,
                turn_id,
            } => ("turn", Some(session_id), Some(turn_id), None),
            Self::Run { run_id } => ("run", None, None, Some(run_id.as_str())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceSpec {
    pub objective: String,
    pub criteria: Vec<String>,
    pub objective_digest: String,
    pub criteria_digest: String,
}

impl ConvergenceSpec {
    pub fn new(
        objective: impl Into<String>,
        criteria: Vec<String>,
    ) -> Result<Self, ConvergenceError> {
        let objective = objective.into();
        validate_string(&objective, "objective", MAX_OBJECTIVE_BYTES)?;
        if objective.trim().is_empty() {
            return Err(ConvergenceError::InvalidInput(
                "objective is required".into(),
            ));
        }
        if criteria.len() > MAX_CRITERIA {
            return Err(ConvergenceError::LimitExceeded(
                "too many convergence criteria".into(),
            ));
        }
        for criterion in &criteria {
            validate_string(criterion, "criterion", MAX_CRITERION_BYTES)?;
            if criterion.trim().is_empty() {
                return Err(ConvergenceError::InvalidInput(
                    "criteria must not contain empty entries".into(),
                ));
            }
        }
        let objective_digest = digest(objective.as_bytes());
        let criteria_bytes = serde_json::to_vec(&criteria)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        let criteria_digest = digest(&criteria_bytes);
        Ok(Self {
            objective,
            criteria,
            objective_digest,
            criteria_digest,
        })
    }

    pub fn validate(&self) -> Result<(), ConvergenceError> {
        let rebuilt = Self::new(self.objective.clone(), self.criteria.clone())?;
        if rebuilt.objective_digest != self.objective_digest
            || rebuilt.criteria_digest != self.criteria_digest
        {
            return Err(ConvergenceError::InvalidInput(
                "convergence specification digest does not match its text".into(),
            ));
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        if encoded.len() > MAX_SPEC_BYTES {
            return Err(ConvergenceError::LimitExceeded(
                "convergence specification envelope is too large".into(),
            ));
        }
        Ok(())
    }

    fn fingerprint_fragment(&self) -> String {
        format!("{}:{}", self.objective_digest, self.criteria_digest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceStatus {
    Pending,
    Producing,
    Verifying,
    AwaitingDecision,
    Repairing,
    Replanning,
    Completed,
    Failed,
    Cancelled,
    Exhausted,
}

impl ConvergenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Producing => "producing",
            Self::Verifying => "verifying",
            Self::AwaitingDecision => "awaiting_decision",
            Self::Repairing => "repairing",
            Self::Replanning => "replanning",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Exhausted => "exhausted",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Exhausted
        )
    }

    fn parse(value: &str) -> Result<Self, ConvergenceError> {
        match value {
            "pending" => Ok(Self::Pending),
            "producing" => Ok(Self::Producing),
            "verifying" => Ok(Self::Verifying),
            "awaiting_decision" => Ok(Self::AwaitingDecision),
            "repairing" => Ok(Self::Repairing),
            "replanning" => Ok(Self::Replanning),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "exhausted" => Ok(Self::Exhausted),
            other => Err(ConvergenceError::Storage(format!(
                "unknown convergence status '{other}'"
            ))),
        }
    }
}

/// Validate the host-owned lifecycle graph.  Terminal states never reopen.
pub fn validate_transition(
    from: ConvergenceStatus,
    to: ConvergenceStatus,
) -> Result<(), ConvergenceError> {
    let valid = match from {
        ConvergenceStatus::Pending => matches!(
            to,
            ConvergenceStatus::Producing | ConvergenceStatus::Cancelled
        ),
        ConvergenceStatus::Producing => matches!(
            to,
            ConvergenceStatus::Verifying | ConvergenceStatus::Failed | ConvergenceStatus::Cancelled
        ),
        ConvergenceStatus::Verifying => matches!(
            to,
            ConvergenceStatus::AwaitingDecision
                | ConvergenceStatus::Failed
                | ConvergenceStatus::Cancelled
        ),
        ConvergenceStatus::AwaitingDecision => matches!(
            to,
            ConvergenceStatus::Completed
                | ConvergenceStatus::Repairing
                | ConvergenceStatus::Replanning
                | ConvergenceStatus::Failed
                | ConvergenceStatus::Cancelled
                | ConvergenceStatus::Exhausted
        ),
        ConvergenceStatus::Repairing => matches!(
            to,
            ConvergenceStatus::Producing
                | ConvergenceStatus::Failed
                | ConvergenceStatus::Cancelled
                | ConvergenceStatus::Exhausted
        ),
        ConvergenceStatus::Replanning => matches!(
            to,
            ConvergenceStatus::Producing
                | ConvergenceStatus::Failed
                | ConvergenceStatus::Cancelled
                | ConvergenceStatus::Exhausted
        ),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ConvergenceError::InvalidTransition { from, to })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceRecord {
    pub id: ConvergenceId,
    pub owner: AgentOrchestrationOwner,
    pub spec: ConvergenceSpec,
    pub status: ConvergenceStatus,
    pub current_cycle: u8,
    pub max_cycles: u8,
    pub created_at: i64,
    pub updated_at: i64,
    pub terminal_at: Option<i64>,
    pub revision: u64,
    pub idempotency_key: String,
    pub request_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConvergence {
    pub id: ConvergenceId,
    pub owner: AgentOrchestrationOwner,
    pub spec: ConvergenceSpec,
    pub max_cycles: u8,
    pub idempotency_key: String,
}

impl NewConvergence {
    fn validate(&self) -> Result<(), ConvergenceError> {
        ConvergenceId::parse(self.id.as_str())?;
        self.spec.validate()?;
        self.owner.validate()?;
        if self.max_cycles == 0 || self.max_cycles > MAX_CYCLES {
            return Err(ConvergenceError::LimitExceeded(
                "max_cycles must be between 1 and 4".into(),
            ));
        }
        validate_string(
            &self.idempotency_key,
            "idempotency key",
            MAX_IDEMPOTENCY_KEY_BYTES,
        )?;
        if self.idempotency_key.is_empty() {
            return Err(ConvergenceError::InvalidInput(
                "idempotency key is required".into(),
            ));
        }
        Ok(())
    }

    fn fingerprint(&self) -> String {
        digest(
            format!(
                "{}:{}:{}:{}",
                self.owner.fingerprint_fragment(),
                self.spec.fingerprint_fragment(),
                self.max_cycles,
                self.idempotency_key
            )
            .as_bytes(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceCycleRecord {
    pub convergence_id: ConvergenceId,
    pub ordinal: u8,
    pub producer_group_id: Option<AgentRunGroupId>,
    pub producer_run_ids: Vec<AgentRunId>,
    pub verifier_run_id: Option<AgentRunId>,
    pub verdict: Option<SemanticVerificationVerdict>,
    pub decision: Option<ConvergenceDecision>,
    pub source_base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SemanticVerificationVerdict {
    /// Advisory semantic review only. This is deliberately not
    /// `GoalVerificationVerdict::Met` and cannot complete a goal.
    Pass {
        summary: String,
        evidence_refs: Vec<String>,
    },
    Revise {
        findings: Vec<AgentRunFinding>,
        repair_requests: Vec<String>,
    },
    Inconclusive {
        reason: String,
        missing_evidence: Vec<String>,
    },
}

impl SemanticVerificationVerdict {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Pass { .. } => "pass",
            Self::Revise { .. } => "revise",
            Self::Inconclusive { .. } => "inconclusive",
        }
    }

    pub fn bounded(self) -> Self {
        match self {
            Self::Pass {
                summary,
                evidence_refs,
            } => Self::Pass {
                summary: bound(&summary, MAX_CONVERGENCE_STRING_BYTES),
                evidence_refs: evidence_refs
                    .into_iter()
                    .take(MAX_EVIDENCE_REFS)
                    .map(|item| bound(&item, MAX_CONVERGENCE_STRING_BYTES))
                    .collect(),
            },
            Self::Revise {
                findings,
                repair_requests,
            } => Self::Revise {
                findings: bound_findings(findings),
                repair_requests: repair_requests
                    .into_iter()
                    .take(MAX_REPAIR_REQUESTS)
                    .map(|item| bound(&item, MAX_CONVERGENCE_STRING_BYTES))
                    .collect(),
            },
            Self::Inconclusive {
                reason,
                missing_evidence,
            } => Self::Inconclusive {
                reason: bound(&reason, MAX_CONVERGENCE_STRING_BYTES),
                missing_evidence: missing_evidence
                    .into_iter()
                    .take(MAX_EVIDENCE_REFS)
                    .map(|item| bound(&item, MAX_CONVERGENCE_STRING_BYTES))
                    .collect(),
            },
        }
    }

    /// Parse the verifier's explicit structured result. The verifier must
    /// return exactly one marked JSON object; arbitrary final prose is not a
    /// verdict and therefore cannot accidentally become `Pass`.
    pub fn parse_marked(input: &str) -> Result<Self, ConvergenceError> {
        let trimmed = input.trim();
        let prefix = format!("<{VERDICT_MARKER}>");
        let suffix = format!("</{VERDICT_MARKER}>");
        let payload = trimmed
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix(&suffix))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ConvergenceError::InvalidInput(
                    "verifier output must contain one marked convergence verdict".into(),
                )
            })?;
        let verdict: Self = serde_json::from_str(payload).map_err(|error| {
            ConvergenceError::Serialization(format!("invalid verifier verdict: {error}"))
        })?;
        let verdict = verdict.bounded();
        let encoded = serde_json::to_vec(&verdict)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        if encoded.len() > MAX_SPEC_BYTES {
            return Err(ConvergenceError::LimitExceeded(
                "verifier verdict exceeds its envelope bound".into(),
            ));
        }
        Ok(verdict)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceDecision {
    Accept,
    Repair,
    Replan,
    Stop,
    Escalate,
}

fn decision_allowed(
    verdict: Option<&SemanticVerificationVerdict>,
    decision: ConvergenceDecision,
) -> bool {
    match verdict {
        Some(SemanticVerificationVerdict::Pass { .. }) => matches!(
            decision,
            ConvergenceDecision::Accept | ConvergenceDecision::Stop | ConvergenceDecision::Escalate
        ),
        Some(SemanticVerificationVerdict::Revise { .. }) => matches!(
            decision,
            ConvergenceDecision::Repair
                | ConvergenceDecision::Replan
                | ConvergenceDecision::Stop
                | ConvergenceDecision::Escalate
        ),
        Some(SemanticVerificationVerdict::Inconclusive { .. }) => matches!(
            decision,
            ConvergenceDecision::Replan | ConvergenceDecision::Stop | ConvergenceDecision::Escalate
        ),
        None => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerEvidence {
    pub run_id: AgentRunId,
    pub status: AgentRunResultStatus,
    pub summary: String,
    pub base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub changed_paths: Vec<String>,
    pub validation: Vec<ValidationEvidence>,
    pub findings: Vec<AgentRunFinding>,
    pub artifacts: Vec<AgentRunArtifact>,
    pub repository_state: RepositoryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierEvidencePacket {
    pub objective: String,
    pub criteria: Vec<String>,
    pub producer_runs: Vec<ProducerEvidence>,
    pub base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub changed_paths: Vec<String>,
    pub validation: Vec<ValidationEvidence>,
    pub findings: Vec<AgentRunFinding>,
    pub artifacts: Vec<AgentRunArtifact>,
    pub repository_state: RepositoryState,
    pub digest: String,
}

impl VerifierEvidencePacket {
    pub fn encode_bounded(&self) -> Result<String, ConvergenceError> {
        let encoded = serde_json::to_string(self)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        if encoded.len() > MAX_VERIFIER_PACKET_BYTES {
            return Err(ConvergenceError::LimitExceeded(
                "verifier evidence packet exceeds its envelope bound".into(),
            ));
        }
        Ok(encoded)
    }
}

/// Assemble only bounded, authoritative run-result fields.  No transcript,
/// hidden reasoning, tool arguments, environment, or credentials can enter
/// this DTO because none is accepted by the function.
pub fn assemble_verifier_evidence(
    spec: &ConvergenceSpec,
    results: &[AgentRunResult],
) -> Result<VerifierEvidencePacket, ConvergenceError> {
    spec.validate()?;
    if results.is_empty() {
        return Err(ConvergenceError::MissingEvidence(
            "at least one producer result is required".into(),
        ));
    }
    if results.len() > MAX_PRODUCER_RUNS {
        return Err(ConvergenceError::LimitExceeded(
            "too many producer results".into(),
        ));
    }

    let bounded_results: Vec<AgentRunResult> = results
        .iter()
        .cloned()
        .map(AgentRunResult::bounded)
        .collect();
    let mut packet = VerifierEvidencePacket {
        objective: spec.objective.clone(),
        criteria: spec.criteria.clone(),
        producer_runs: bounded_results.iter().map(producer_evidence).collect(),
        base_commit: bounded_results
            .iter()
            .find_map(|result| result.base_commit.clone()),
        result_commit: bounded_results
            .iter()
            .rev()
            .find_map(|result| result.result_commit.clone()),
        changed_paths: bounded_results
            .iter()
            .flat_map(|result| result.changed_paths.iter().cloned())
            .take(crate::run_result::MAX_RESULT_PATHS)
            .collect(),
        validation: bounded_results
            .iter()
            .flat_map(|result| result.validation.iter().cloned())
            .take(crate::run_result::MAX_RESULT_ITEMS)
            .collect(),
        findings: bounded_results
            .iter()
            .flat_map(|result| result.findings.iter().cloned())
            .take(crate::run_result::MAX_RESULT_ITEMS)
            .collect(),
        artifacts: bounded_results
            .iter()
            .flat_map(|result| result.artifacts.iter().cloned())
            .take(crate::run_result::MAX_RESULT_ITEMS)
            .collect(),
        repository_state: if bounded_results
            .iter()
            .any(|result| result.repository_state == RepositoryState::Conflicted)
        {
            RepositoryState::Conflicted
        } else if bounded_results
            .iter()
            .any(|result| result.repository_state == RepositoryState::Dirty)
        {
            RepositoryState::Dirty
        } else if bounded_results
            .iter()
            .any(|result| result.repository_state == RepositoryState::Unknown)
        {
            RepositoryState::Unknown
        } else {
            RepositoryState::Clean
        },
        digest: String::new(),
    };
    bound_packet(&mut packet);
    let canonical = serde_json::to_vec(&packet_without_digest(&packet))
        .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
    packet.digest = digest(&canonical);
    packet.encode_bounded()?;
    Ok(packet)
}

fn producer_evidence(result: &AgentRunResult) -> ProducerEvidence {
    ProducerEvidence {
        run_id: result.run_id.clone(),
        status: result.status,
        summary: result.summary.clone(),
        base_commit: result.base_commit.clone(),
        result_commit: result.result_commit.clone(),
        changed_paths: result.changed_paths.clone(),
        validation: result.validation.clone(),
        findings: result.findings.clone(),
        artifacts: result.artifacts.clone(),
        repository_state: result.repository_state,
    }
}

fn packet_without_digest(packet: &VerifierEvidencePacket) -> impl Serialize + '_ {
    (
        &packet.objective,
        &packet.criteria,
        &packet.producer_runs,
        &packet.base_commit,
        &packet.result_commit,
        &packet.changed_paths,
        &packet.validation,
        &packet.findings,
        &packet.artifacts,
        &packet.repository_state,
    )
}

fn bound_packet(packet: &mut VerifierEvidencePacket) {
    packet.objective = bound(&packet.objective, MAX_OBJECTIVE_BYTES);
    packet.criteria = packet
        .criteria
        .drain(..)
        .take(MAX_CRITERIA)
        .map(|item| bound(&item, MAX_CRITERION_BYTES))
        .collect();
    for producer in &mut packet.producer_runs {
        producer.summary = bound(
            &producer.summary,
            crate::run_result::MAX_RESULT_SUMMARY_BYTES,
        );
        producer.base_commit = producer.base_commit.take().map(|value| bound(&value, 128));
        producer.result_commit = producer
            .result_commit
            .take()
            .map(|value| bound(&value, 128));
        producer.changed_paths = producer
            .changed_paths
            .drain(..)
            .take(crate::run_result::MAX_RESULT_PATHS)
            .map(|value| bound(&value, crate::run_result::MAX_RESULT_PATH_BYTES))
            .collect();
        producer.validation = producer
            .validation
            .drain(..)
            .take(crate::run_result::MAX_RESULT_ITEMS)
            .map(|mut value| {
                value.kind = bound(&value.kind, 128);
                value.summary = bound(&value.summary, 1024);
                value
            })
            .collect();
        producer.findings = bound_findings(std::mem::take(&mut producer.findings));
        producer.artifacts = producer
            .artifacts
            .drain(..)
            .take(crate::run_result::MAX_RESULT_ITEMS)
            .map(|mut value| {
                value.kind = bound(&value.kind, 128);
                value.label = bound(&value.label, 512);
                value.reference = value.reference.map(|item| bound(&item, 1024));
                value
            })
            .collect();
    }
    packet.changed_paths = packet
        .changed_paths
        .drain(..)
        .take(crate::run_result::MAX_RESULT_PATHS)
        .map(|value| bound(&value, crate::run_result::MAX_RESULT_PATH_BYTES))
        .collect();
    packet.validation = packet
        .validation
        .drain(..)
        .take(crate::run_result::MAX_RESULT_ITEMS)
        .map(|mut value| {
            value.kind = bound(&value.kind, 128);
            value.summary = bound(&value.summary, 1024);
            value
        })
        .collect();
    packet.findings = bound_findings(std::mem::take(&mut packet.findings));
    packet.artifacts = packet
        .artifacts
        .drain(..)
        .take(crate::run_result::MAX_RESULT_ITEMS)
        .map(|mut value| {
            value.kind = bound(&value.kind, 128);
            value.label = bound(&value.label, 512);
            value.reference = value.reference.map(|item| bound(&item, 1024));
            value
        })
        .collect();
}

fn bound_findings(findings: Vec<AgentRunFinding>) -> Vec<AgentRunFinding> {
    findings
        .into_iter()
        .take(crate::run_result::MAX_RESULT_ITEMS)
        .map(|mut value| {
            value.severity = bound(&value.severity, 32);
            value.title = bound(&value.title, 512);
            value.rationale = bound(&value.rationale, 2048);
            value.file = value
                .file
                .map(|item| bound(&item, crate::run_result::MAX_RESULT_PATH_BYTES));
            value
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceSummary {
    pub convergence_id: String,
    pub owner_summary: String,
    pub status: ConvergenceStatus,
    pub cycle_ordinal: u8,
    pub max_cycles: u8,
    pub remaining_cycles: u8,
    pub producer_run_ids: Vec<String>,
    pub producer_status_counts: ProducerStatusCounts,
    pub verifier_run_id: Option<String>,
    pub verdict_kind: Option<String>,
    pub verdict_summary: Option<String>,
    pub awaiting_decision: bool,
    pub terminal_reason_class: Option<String>,
    pub selected_run_id: Option<String>,
    pub selected_result_commit: Option<String>,
    pub last_finding_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerStatusCounts {
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub active: usize,
}

impl ConvergenceRecord {
    pub fn summary(
        &self,
        cycle: Option<&ConvergenceCycleRecord>,
        producer_statuses: &[AgentRunStatus],
    ) -> ConvergenceSummary {
        let counts =
            producer_statuses
                .iter()
                .fold(ProducerStatusCounts::default(), |mut counts, status| {
                    match status {
                        AgentRunStatus::Completed => counts.completed += 1,
                        AgentRunStatus::Failed | AgentRunStatus::Interrupted => counts.failed += 1,
                        AgentRunStatus::Cancelled => counts.cancelled += 1,
                        _ => counts.active += 1,
                    }
                    counts
                });
        let owner_summary = match &self.owner {
            AgentOrchestrationOwner::Turn {
                session_id,
                turn_id,
            } => {
                format!("turn:{session_id}:{turn_id}")
            }
            AgentOrchestrationOwner::Run { run_id } => format!("run:{run_id}"),
        };
        let (
            producer_run_ids,
            verifier_run_id,
            verdict_kind,
            verdict_summary,
            selected_run_id,
            selected_result_commit,
            last_finding_count,
        ) = cycle
            .map(|cycle| {
                let (kind, summary) = cycle.verdict.as_ref().map_or((None, None), |verdict| {
                    let summary = match verdict {
                        SemanticVerificationVerdict::Pass { summary, .. } => Some(summary.clone()),
                        SemanticVerificationVerdict::Revise { .. } => None,
                        SemanticVerificationVerdict::Inconclusive { reason, .. } => {
                            Some(reason.clone())
                        }
                    };
                    (Some(verdict.kind().to_owned()), summary)
                });
                (
                    cycle
                        .producer_run_ids
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    cycle.verifier_run_id.as_ref().map(ToString::to_string),
                    kind,
                    summary,
                    (self.status == ConvergenceStatus::Completed)
                        .then(|| cycle.producer_run_ids.first().map(ToString::to_string))
                        .flatten(),
                    (self.status == ConvergenceStatus::Completed)
                        .then(|| cycle.result_commit.clone())
                        .flatten(),
                    cycle
                        .verdict
                        .as_ref()
                        .map(|verdict| match verdict {
                            SemanticVerificationVerdict::Revise { findings, .. } => findings.len(),
                            _ => 0,
                        })
                        .unwrap_or_default(),
                )
            })
            .unwrap_or((Vec::new(), None, None, None, None, None, 0));
        ConvergenceSummary {
            convergence_id: self.id.to_string(),
            owner_summary,
            status: self.status,
            cycle_ordinal: self.current_cycle,
            max_cycles: self.max_cycles,
            remaining_cycles: if self.status.is_terminal() {
                0
            } else {
                self.max_cycles.saturating_sub(self.current_cycle + 1)
            },
            producer_run_ids,
            producer_status_counts: counts,
            verifier_run_id,
            verdict_kind,
            verdict_summary: verdict_summary.map(|value| bound(&value, 512)),
            awaiting_decision: self.status == ConvergenceStatus::AwaitingDecision,
            terminal_reason_class: self
                .status
                .is_terminal()
                .then(|| self.status.as_str().to_owned()),
            selected_run_id,
            selected_result_commit,
            last_finding_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationAction {
    NoChange,
    AdvanceToVerifying,
    AdvanceToAwaitingDecision,
    MarkFailed,
    MarkCancelled,
    NeedsExecutionResume { phase: ConvergenceStatus },
    NeedsAttention { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationInput<'a> {
    pub record: &'a ConvergenceRecord,
    pub cycle: Option<&'a ConvergenceCycleRecord>,
    pub producer_group_status: Option<RunGroupStatus>,
    pub producer_run_statuses: &'a [AgentRunStatus],
    pub verifier_status: Option<AgentRunStatus>,
}

/// Pure, repeatable restart classification.  It never schedules missing work
/// and never treats an absent verdict as a pass.
pub fn classify_reconciliation(input: ReconciliationInput<'_>) -> ReconciliationAction {
    let record = input.record;
    if record.status.is_terminal() || record.status == ConvergenceStatus::AwaitingDecision {
        return ReconciliationAction::NoChange;
    }
    match record.status {
        ConvergenceStatus::Pending => ReconciliationAction::NeedsExecutionResume {
            phase: ConvergenceStatus::Producing,
        },
        ConvergenceStatus::Producing => match input.producer_group_status {
            Some(RunGroupStatus::Completed) => ReconciliationAction::AdvanceToVerifying,
            Some(RunGroupStatus::Failed) => ReconciliationAction::MarkFailed,
            Some(RunGroupStatus::Cancelled) => ReconciliationAction::MarkCancelled,
            Some(RunGroupStatus::Pending | RunGroupStatus::Running) => {
                ReconciliationAction::NoChange
            }
            None if !input.producer_run_statuses.is_empty()
                && input
                    .producer_run_statuses
                    .iter()
                    .all(|status| status.is_terminal()) =>
            {
                if input
                    .producer_run_statuses
                    .iter()
                    .all(|status| *status == AgentRunStatus::Cancelled)
                {
                    ReconciliationAction::MarkCancelled
                } else {
                    ReconciliationAction::MarkFailed
                }
            }
            None => ReconciliationAction::NeedsExecutionResume {
                phase: ConvergenceStatus::Producing,
            },
        },
        ConvergenceStatus::Verifying => match input.verifier_status {
            Some(AgentRunStatus::Completed)
                if input
                    .cycle
                    .and_then(|cycle| cycle.verdict.as_ref())
                    .is_some() =>
            {
                ReconciliationAction::AdvanceToAwaitingDecision
            }
            Some(status) if status.is_terminal() => ReconciliationAction::NeedsAttention {
                reason: "verifier is terminal without a persisted parseable verdict".into(),
            },
            Some(_) => ReconciliationAction::NoChange,
            None => ReconciliationAction::NeedsExecutionResume {
                phase: ConvergenceStatus::Verifying,
            },
        },
        ConvergenceStatus::Repairing | ConvergenceStatus::Replanning => {
            ReconciliationAction::NeedsExecutionResume {
                phase: record.status,
            }
        }
        ConvergenceStatus::AwaitingDecision
        | ConvergenceStatus::Completed
        | ConvergenceStatus::Failed
        | ConvergenceStatus::Cancelled
        | ConvergenceStatus::Exhausted => ReconciliationAction::NoChange,
    }
}

#[derive(Debug, Error)]
pub enum ConvergenceError {
    #[error("invalid convergence input: {0}")]
    InvalidInput(String),
    #[error("convergence bound exceeded: {0}")]
    LimitExceeded(String),
    #[error("invalid convergence transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: ConvergenceStatus,
        to: ConvergenceStatus,
    },
    #[error("convergence '{0}' not found")]
    NotFound(String),
    #[error("convergence idempotency key conflicts with an existing request")]
    IdempotencyConflict,
    #[error("convergence revision conflict")]
    RevisionConflict,
    #[error("convergence cycle already exists")]
    CycleConflict,
    #[error("convergence evidence is missing: {0}")]
    MissingEvidence(String),
    #[error("convergence storage failure: {0}")]
    Storage(String),
    #[error("convergence serialization failure: {0}")]
    Serialization(String),
}

#[async_trait]
pub trait ConvergenceStore: Send + Sync {
    async fn create_or_get(
        &self,
        input: NewConvergence,
    ) -> Result<ConvergenceRecord, ConvergenceError>;
    async fn get(&self, id: &ConvergenceId) -> Result<Option<ConvergenceRecord>, ConvergenceError>;
    async fn list_by_owner(
        &self,
        owner: &AgentOrchestrationOwner,
        limit: usize,
    ) -> Result<Vec<ConvergenceRecord>, ConvergenceError>;
    async fn transition(
        &self,
        id: &ConvergenceId,
        expected_revision: u64,
        next: ConvergenceStatus,
    ) -> Result<ConvergenceRecord, ConvergenceError>;
    async fn create_cycle(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError>;
    async fn get_cycle(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
    ) -> Result<Option<ConvergenceCycleRecord>, ConvergenceError>;
    async fn set_producer_references(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        group_id: Option<AgentRunGroupId>,
        run_ids: Vec<AgentRunId>,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError>;
    async fn set_verifier_run(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        run_id: AgentRunId,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError>;
    async fn set_cycle_commits(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        source_base_commit: Option<String>,
        result_commit: Option<String>,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError>;
    async fn set_verdict(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        verdict: SemanticVerificationVerdict,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError>;
    async fn set_decision(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        expected_revision: u64,
        decision: ConvergenceDecision,
    ) -> Result<ConvergenceRecord, ConvergenceError>;
    async fn list_nonterminal(
        &self,
        limit: usize,
    ) -> Result<Vec<ConvergenceRecord>, ConvergenceError>;
}

#[derive(Default)]
struct MemoryState {
    records: HashMap<ConvergenceId, ConvergenceRecord>,
    cycles: HashMap<(ConvergenceId, u8), ConvergenceCycleRecord>,
    by_key: HashMap<String, ConvergenceId>,
}

#[derive(Default)]
pub struct InMemoryConvergenceStore {
    state: Mutex<MemoryState>,
}

impl InMemoryConvergenceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn new_record(input: &NewConvergence) -> ConvergenceRecord {
    let now = Utc::now().timestamp_millis();
    ConvergenceRecord {
        id: input.id.clone(),
        owner: input.owner.clone(),
        spec: input.spec.clone(),
        status: ConvergenceStatus::Pending,
        current_cycle: 0,
        max_cycles: input.max_cycles,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        revision: 0,
        idempotency_key: input.idempotency_key.clone(),
        request_fingerprint: input.fingerprint(),
    }
}

fn validate_cycle_ordinal(record: &ConvergenceRecord, ordinal: u8) -> Result<(), ConvergenceError> {
    if ordinal >= record.max_cycles {
        return Err(ConvergenceError::LimitExceeded(
            "cycle ordinal exceeds max_cycles".into(),
        ));
    }
    Ok(())
}

fn empty_cycle(id: &ConvergenceId, ordinal: u8) -> ConvergenceCycleRecord {
    ConvergenceCycleRecord {
        convergence_id: id.clone(),
        ordinal,
        producer_group_id: None,
        producer_run_ids: Vec::new(),
        verifier_run_id: None,
        verdict: None,
        decision: None,
        source_base_commit: None,
        result_commit: None,
        created_at: Utc::now().timestamp_millis(),
        completed_at: None,
    }
}

fn validate_run_ids(run_ids: &[AgentRunId]) -> Result<(), ConvergenceError> {
    if run_ids.is_empty() || run_ids.len() > MAX_PRODUCER_RUNS {
        return Err(ConvergenceError::LimitExceeded(
            "producer run count is out of bounds".into(),
        ));
    }
    if run_ids.iter().collect::<HashSet<_>>().len() != run_ids.len() {
        return Err(ConvergenceError::InvalidInput(
            "producer run ids must be unique".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl ConvergenceStore for InMemoryConvergenceStore {
    async fn create_or_get(
        &self,
        input: NewConvergence,
    ) -> Result<ConvergenceRecord, ConvergenceError> {
        input.validate()?;
        let mut state = self.state.lock().await;
        if let Some(existing_id) = state.by_key.get(&input.idempotency_key) {
            let existing = state
                .records
                .get(existing_id)
                .ok_or_else(|| ConvergenceError::NotFound(existing_id.to_string()))?;
            if existing.request_fingerprint != input.fingerprint() {
                return Err(ConvergenceError::IdempotencyConflict);
            }
            return Ok(existing.clone());
        }
        let record = new_record(&input);
        state
            .by_key
            .insert(input.idempotency_key, record.id.clone());
        state.records.insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn get(&self, id: &ConvergenceId) -> Result<Option<ConvergenceRecord>, ConvergenceError> {
        Ok(self.state.lock().await.records.get(id).cloned())
    }

    async fn list_by_owner(
        &self,
        owner: &AgentOrchestrationOwner,
        limit: usize,
    ) -> Result<Vec<ConvergenceRecord>, ConvergenceError> {
        owner.validate()?;
        let limit = limit.min(MAX_CONVERGENCE_LIST);
        let mut records: Vec<_> = self
            .state
            .lock()
            .await
            .records
            .values()
            .filter(|record| &record.owner == owner)
            .cloned()
            .collect();
        records.sort_by_key(|record| (record.created_at, record.id.clone()));
        records.truncate(limit);
        Ok(records)
    }

    async fn transition(
        &self,
        id: &ConvergenceId,
        expected_revision: u64,
        next: ConvergenceStatus,
    ) -> Result<ConvergenceRecord, ConvergenceError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(id)
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
        if record.revision != expected_revision {
            return Err(ConvergenceError::RevisionConflict);
        }
        validate_transition(record.status, next)?;
        if matches!(
            next,
            ConvergenceStatus::Repairing | ConvergenceStatus::Replanning
        ) && record.current_cycle.saturating_add(1) >= record.max_cycles
        {
            return Err(ConvergenceError::LimitExceeded(
                "convergence cycle budget is exhausted".into(),
            ));
        }
        record.status = next;
        if matches!(
            next,
            ConvergenceStatus::Repairing | ConvergenceStatus::Replanning
        ) {
            record.current_cycle = record.current_cycle.saturating_add(1);
        }
        record.revision = record.revision.saturating_add(1);
        record.updated_at = Utc::now().timestamp_millis();
        record.terminal_at = next.is_terminal().then_some(record.updated_at);
        Ok(record.clone())
    }

    async fn create_cycle(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get(id)
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
        validate_cycle_ordinal(record, ordinal)?;
        if state.cycles.contains_key(&(id.clone(), ordinal)) {
            return Err(ConvergenceError::CycleConflict);
        }
        let cycle = empty_cycle(id, ordinal);
        state.cycles.insert((id.clone(), ordinal), cycle.clone());
        Ok(cycle)
    }

    async fn get_cycle(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
    ) -> Result<Option<ConvergenceCycleRecord>, ConvergenceError> {
        Ok(self
            .state
            .lock()
            .await
            .cycles
            .get(&(id.clone(), ordinal))
            .cloned())
    }

    async fn set_producer_references(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        group_id: Option<AgentRunGroupId>,
        run_ids: Vec<AgentRunId>,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        validate_run_ids(&run_ids)?;
        let mut state = self.state.lock().await;
        let cycle = state
            .cycles
            .get_mut(&(id.clone(), ordinal))
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if !cycle.producer_run_ids.is_empty()
            && (cycle.producer_group_id != group_id || cycle.producer_run_ids != run_ids)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        cycle.producer_group_id = group_id;
        cycle.producer_run_ids = run_ids;
        Ok(cycle.clone())
    }

    async fn set_verifier_run(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        run_id: AgentRunId,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        let mut state = self.state.lock().await;
        let cycle = state
            .cycles
            .get_mut(&(id.clone(), ordinal))
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if cycle
            .verifier_run_id
            .as_ref()
            .is_some_and(|existing| existing != &run_id)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        cycle.verifier_run_id = Some(run_id);
        Ok(cycle.clone())
    }

    async fn set_cycle_commits(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        source_base_commit: Option<String>,
        result_commit: Option<String>,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        validate_commit(source_base_commit.as_deref(), "source base commit")?;
        validate_commit(result_commit.as_deref(), "result commit")?;
        let mut state = self.state.lock().await;
        let cycle = state
            .cycles
            .get_mut(&(id.clone(), ordinal))
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if (cycle.source_base_commit.is_some() && cycle.source_base_commit != source_base_commit)
            || (cycle.result_commit.is_some() && cycle.result_commit != result_commit)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        cycle.source_base_commit = source_base_commit;
        cycle.result_commit = result_commit;
        Ok(cycle.clone())
    }

    async fn set_verdict(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        verdict: SemanticVerificationVerdict,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        let mut state = self.state.lock().await;
        let cycle = state
            .cycles
            .get_mut(&(id.clone(), ordinal))
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        let verdict = verdict.bounded();
        if cycle
            .verdict
            .as_ref()
            .is_some_and(|existing| existing != &verdict)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        cycle.verdict = Some(verdict);
        Ok(cycle.clone())
    }

    async fn set_decision(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        expected_revision: u64,
        decision: ConvergenceDecision,
    ) -> Result<ConvergenceRecord, ConvergenceError> {
        let mut state = self.state.lock().await;
        let (status, revision, current_cycle, max_cycles) = {
            let record = state
                .records
                .get(id)
                .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
            (
                record.status,
                record.revision,
                record.current_cycle,
                record.max_cycles,
            )
        };
        if revision != expected_revision {
            return Err(ConvergenceError::RevisionConflict);
        }
        if status != ConvergenceStatus::AwaitingDecision {
            return Err(ConvergenceError::InvalidInput(
                "owner decisions require awaiting_decision".into(),
            ));
        }
        let cycle = state
            .cycles
            .get_mut(&(id.clone(), ordinal))
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if cycle.decision.is_some() {
            return Err(ConvergenceError::RevisionConflict);
        }
        if !decision_allowed(cycle.verdict.as_ref(), decision) {
            return Err(ConvergenceError::InvalidInput(
                "owner decision is not valid for the persisted verdict".into(),
            ));
        }
        if matches!(
            decision,
            ConvergenceDecision::Repair | ConvergenceDecision::Replan
        ) && current_cycle.saturating_add(1) >= max_cycles
        {
            return Err(ConvergenceError::LimitExceeded(
                "convergence cycle budget is exhausted".into(),
            ));
        }
        cycle.decision = Some(decision);
        let next = match decision {
            ConvergenceDecision::Accept | ConvergenceDecision::Stop => ConvergenceStatus::Completed,
            ConvergenceDecision::Repair => ConvergenceStatus::Repairing,
            ConvergenceDecision::Replan => ConvergenceStatus::Replanning,
            ConvergenceDecision::Escalate => ConvergenceStatus::Exhausted,
        };
        validate_transition(status, next)?;
        let record = state
            .records
            .get_mut(id)
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
        record.status = next;
        if matches!(
            next,
            ConvergenceStatus::Repairing | ConvergenceStatus::Replanning
        ) {
            record.current_cycle = record.current_cycle.saturating_add(1);
        }
        record.revision = record.revision.saturating_add(1);
        record.updated_at = Utc::now().timestamp_millis();
        record.terminal_at = next.is_terminal().then_some(record.updated_at);
        Ok(record.clone())
    }

    async fn list_nonterminal(
        &self,
        limit: usize,
    ) -> Result<Vec<ConvergenceRecord>, ConvergenceError> {
        let limit = limit.min(MAX_CONVERGENCE_LIST);
        Ok(self
            .state
            .lock()
            .await
            .records
            .values()
            .filter(|record| !record.status.is_terminal())
            .take(limit)
            .cloned()
            .collect())
    }
}

pub struct SqliteConvergenceStore {
    pool: SqlitePool,
}

impl SqliteConvergenceStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn sqlite_error(error: impl std::fmt::Display) -> ConvergenceError {
    ConvergenceError::Storage(error.to_string())
}

async fn load_record(
    pool: &SqlitePool,
    id: &ConvergenceId,
) -> Result<Option<ConvergenceRecord>, ConvergenceError> {
    let row = sqlx::query("SELECT id, owner_kind, owner_session_id, owner_turn_id, owner_run_id, objective, criteria_json, objective_digest, criteria_digest, status, current_cycle, max_cycles, created_at, updated_at, terminal_at, revision, idempotency_key, request_fingerprint FROM agent_convergence WHERE id = ?")
        .bind(id.as_str()).fetch_optional(pool).await.map_err(sqlite_error)?;
    row.map(|row| {
        let owner = if row.get::<String, _>("owner_kind") == "turn" {
            AgentOrchestrationOwner::Turn {
                session_id: row.get("owner_session_id"),
                turn_id: row.get("owner_turn_id"),
            }
        } else {
            AgentOrchestrationOwner::Run {
                run_id: AgentRunId::parse(&row.get::<String, _>("owner_run_id"))
                    .map_err(sqlite_error)?,
            }
        };
        owner.validate()?;
        let criteria: Vec<String> =
            serde_json::from_str(&row.get::<String, _>("criteria_json")).map_err(sqlite_error)?;
        let spec = ConvergenceSpec {
            objective: row.get("objective"),
            criteria,
            objective_digest: row.get("objective_digest"),
            criteria_digest: row.get("criteria_digest"),
        };
        spec.validate()?;
        Ok(ConvergenceRecord {
            id: ConvergenceId::parse(&row.get::<String, _>("id"))?,
            owner,
            spec,
            status: ConvergenceStatus::parse(&row.get::<String, _>("status"))?,
            current_cycle: row.get::<i64, _>("current_cycle") as u8,
            max_cycles: row.get::<i64, _>("max_cycles") as u8,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            terminal_at: row.get("terminal_at"),
            revision: row.get::<i64, _>("revision") as u64,
            idempotency_key: row.get("idempotency_key"),
            request_fingerprint: row.get("request_fingerprint"),
        })
    })
    .transpose()
}

async fn load_cycle(
    pool: &SqlitePool,
    id: &ConvergenceId,
    ordinal: u8,
) -> Result<Option<ConvergenceCycleRecord>, ConvergenceError> {
    let row = sqlx::query("SELECT convergence_id, ordinal, producer_group_id, producer_run_ids_json, verifier_run_id, verdict_json, decision, source_base_commit, result_commit, created_at, completed_at FROM agent_convergence_cycle WHERE convergence_id = ? AND ordinal = ?")
        .bind(id.as_str()).bind(i64::from(ordinal)).fetch_optional(pool).await.map_err(sqlite_error)?;
    row.map(|row| {
        let producer_run_ids =
            serde_json::from_str::<Vec<AgentRunId>>(&row.get::<String, _>("producer_run_ids_json"))
                .map_err(sqlite_error)?;
        let verdict = row
            .get::<Option<String>, _>("verdict_json")
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(sqlite_error)?;
        let decision = row
            .get::<Option<String>, _>("decision")
            .map(|value| match value.as_str() {
                "accept" => Ok(ConvergenceDecision::Accept),
                "repair" => Ok(ConvergenceDecision::Repair),
                "replan" => Ok(ConvergenceDecision::Replan),
                "stop" => Ok(ConvergenceDecision::Stop),
                "escalate" => Ok(ConvergenceDecision::Escalate),
                _ => Err(ConvergenceError::Storage(
                    "unknown convergence decision".into(),
                )),
            })
            .transpose()?;
        Ok(ConvergenceCycleRecord {
            convergence_id: ConvergenceId::parse(&row.get::<String, _>("convergence_id"))?,
            ordinal: row.get::<i64, _>("ordinal") as u8,
            producer_group_id: row
                .get::<Option<String>, _>("producer_group_id")
                .map(|value| AgentRunGroupId::parse(&value).map_err(sqlite_error))
                .transpose()?,
            producer_run_ids,
            verifier_run_id: row
                .get::<Option<String>, _>("verifier_run_id")
                .map(|value| AgentRunId::parse(&value).map_err(sqlite_error))
                .transpose()?,
            verdict,
            decision,
            source_base_commit: row.get("source_base_commit"),
            result_commit: row.get("result_commit"),
            created_at: row.get("created_at"),
            completed_at: row.get("completed_at"),
        })
    })
    .transpose()
}

#[async_trait]
impl ConvergenceStore for SqliteConvergenceStore {
    async fn create_or_get(
        &self,
        input: NewConvergence,
    ) -> Result<ConvergenceRecord, ConvergenceError> {
        input.validate()?;
        if let Some(existing) = sqlx::query(
            "SELECT id, request_fingerprint FROM agent_convergence WHERE idempotency_key = ?",
        )
        .bind(&input.idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlite_error)?
        {
            let id = ConvergenceId::parse(&existing.get::<String, _>("id"))?;
            if existing.get::<String, _>("request_fingerprint") != input.fingerprint() {
                return Err(ConvergenceError::IdempotencyConflict);
            }
            return load_record(&self.pool, &id)
                .await?
                .ok_or_else(|| ConvergenceError::NotFound(id.to_string()));
        }
        let record = new_record(&input);
        let criteria_json = serde_json::to_string(&record.spec.criteria)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        let (owner_kind, owner_session_id, owner_turn_id, owner_run_id) =
            record.owner.sqlite_parts();
        let result = sqlx::query("INSERT INTO agent_convergence (id, owner_kind, owner_session_id, owner_turn_id, owner_run_id, objective, criteria_json, objective_digest, criteria_digest, status, current_cycle, max_cycles, created_at, updated_at, terminal_at, revision, idempotency_key, request_fingerprint) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(record.id.as_str()).bind(owner_kind).bind(owner_session_id).bind(owner_turn_id).bind(owner_run_id).bind(&record.spec.objective).bind(criteria_json).bind(&record.spec.objective_digest).bind(&record.spec.criteria_digest).bind(record.status.as_str()).bind(i64::from(record.current_cycle)).bind(i64::from(record.max_cycles)).bind(record.created_at).bind(record.updated_at).bind(record.terminal_at).bind(record.revision as i64).bind(&record.idempotency_key).bind(&record.request_fingerprint).execute(&self.pool).await;
        if let Err(error) = result {
            if error
                .to_string()
                .contains("UNIQUE constraint failed: agent_convergence.idempotency_key")
            {
                return self.create_or_get(input).await;
            }
            return Err(sqlite_error(error));
        }
        Ok(record)
    }

    async fn get(&self, id: &ConvergenceId) -> Result<Option<ConvergenceRecord>, ConvergenceError> {
        load_record(&self.pool, id).await
    }

    async fn list_by_owner(
        &self,
        owner: &AgentOrchestrationOwner,
        limit: usize,
    ) -> Result<Vec<ConvergenceRecord>, ConvergenceError> {
        owner.validate()?;
        let limit = limit.min(MAX_CONVERGENCE_LIST) as i64;
        let records = match owner {
            AgentOrchestrationOwner::Turn { session_id, turn_id } => sqlx::query("SELECT id FROM agent_convergence WHERE owner_kind = 'turn' AND owner_session_id = ? AND owner_turn_id = ? ORDER BY created_at, id LIMIT ?").bind(session_id).bind(turn_id).bind(limit).fetch_all(&self.pool).await,
            AgentOrchestrationOwner::Run { run_id } => sqlx::query("SELECT id FROM agent_convergence WHERE owner_kind = 'run' AND owner_run_id = ? ORDER BY created_at, id LIMIT ?").bind(run_id.as_str()).bind(limit).fetch_all(&self.pool).await,
        }.map_err(sqlite_error)?;
        let mut output = Vec::with_capacity(records.len());
        for row in records {
            let id = ConvergenceId::parse(&row.get::<String, _>("id"))?;
            if let Some(record) = load_record(&self.pool, &id).await? {
                output.push(record);
            }
        }
        Ok(output)
    }

    async fn transition(
        &self,
        id: &ConvergenceId,
        expected_revision: u64,
        next: ConvergenceStatus,
    ) -> Result<ConvergenceRecord, ConvergenceError> {
        let record = load_record(&self.pool, id)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
        if record.revision != expected_revision {
            return Err(ConvergenceError::RevisionConflict);
        }
        validate_transition(record.status, next)?;
        if matches!(
            next,
            ConvergenceStatus::Repairing | ConvergenceStatus::Replanning
        ) && record.current_cycle.saturating_add(1) >= record.max_cycles
        {
            return Err(ConvergenceError::LimitExceeded(
                "convergence cycle budget is exhausted".into(),
            ));
        }
        let now = Utc::now().timestamp_millis();
        let cycle_increment = i64::from(matches!(
            next,
            ConvergenceStatus::Repairing | ConvergenceStatus::Replanning
        ));
        let result = sqlx::query("UPDATE agent_convergence SET status = ?, current_cycle = current_cycle + ?, updated_at = ?, terminal_at = ?, revision = revision + 1 WHERE id = ? AND revision = ?")
            .bind(next.as_str()).bind(cycle_increment).bind(now).bind(next.is_terminal().then_some(now)).bind(id.as_str()).bind(expected_revision as i64).execute(&self.pool).await.map_err(sqlite_error)?;
        if result.rows_affected() == 0 {
            return Err(ConvergenceError::RevisionConflict);
        }
        load_record(&self.pool, id)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))
    }

    async fn create_cycle(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        let record = load_record(&self.pool, id)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
        validate_cycle_ordinal(&record, ordinal)?;
        let cycle = empty_cycle(id, ordinal);
        let runs = serde_json::to_string(&cycle.producer_run_ids)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        let result = sqlx::query("INSERT INTO agent_convergence_cycle (convergence_id, ordinal, producer_run_ids_json, created_at) VALUES (?, ?, ?, ?)").bind(id.as_str()).bind(i64::from(ordinal)).bind(runs).bind(cycle.created_at).execute(&self.pool).await;
        match result {
            Ok(_) => Ok(cycle),
            Err(error) if error.to_string().contains("UNIQUE constraint failed") => {
                Err(ConvergenceError::CycleConflict)
            }
            Err(error) => Err(sqlite_error(error)),
        }
    }

    async fn get_cycle(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
    ) -> Result<Option<ConvergenceCycleRecord>, ConvergenceError> {
        load_cycle(&self.pool, id, ordinal).await
    }

    async fn set_producer_references(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        group_id: Option<AgentRunGroupId>,
        run_ids: Vec<AgentRunId>,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        validate_run_ids(&run_ids)?;
        let current = load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if !current.producer_run_ids.is_empty()
            && (current.producer_group_id != group_id || current.producer_run_ids != run_ids)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        let runs = serde_json::to_string(&run_ids)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        sqlx::query("UPDATE agent_convergence_cycle SET producer_group_id = ?, producer_run_ids_json = ? WHERE convergence_id = ? AND ordinal = ?").bind(group_id.as_ref().map(AgentRunGroupId::as_str)).bind(runs).bind(id.as_str()).bind(i64::from(ordinal)).execute(&self.pool).await.map_err(sqlite_error)?;
        load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))
    }

    async fn set_verifier_run(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        run_id: AgentRunId,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        let current = load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if current
            .verifier_run_id
            .as_ref()
            .is_some_and(|existing| existing != &run_id)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        sqlx::query("UPDATE agent_convergence_cycle SET verifier_run_id = ? WHERE convergence_id = ? AND ordinal = ?").bind(run_id.as_str()).bind(id.as_str()).bind(i64::from(ordinal)).execute(&self.pool).await.map_err(sqlite_error)?;
        load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))
    }

    async fn set_cycle_commits(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        source_base_commit: Option<String>,
        result_commit: Option<String>,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        validate_commit(source_base_commit.as_deref(), "source base commit")?;
        validate_commit(result_commit.as_deref(), "result commit")?;
        let current = load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if (current.source_base_commit.is_some()
            && current.source_base_commit != source_base_commit)
            || (current.result_commit.is_some() && current.result_commit != result_commit)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        sqlx::query("UPDATE agent_convergence_cycle SET source_base_commit = ?, result_commit = ? WHERE convergence_id = ? AND ordinal = ?")
            .bind(source_base_commit)
            .bind(result_commit)
            .bind(id.as_str())
            .bind(i64::from(ordinal))
            .execute(&self.pool)
            .await
            .map_err(sqlite_error)?;
        load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))
    }

    async fn set_verdict(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        verdict: SemanticVerificationVerdict,
    ) -> Result<ConvergenceCycleRecord, ConvergenceError> {
        let current = load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        let verdict = verdict.bounded();
        if current
            .verdict
            .as_ref()
            .is_some_and(|existing| existing != &verdict)
        {
            return Err(ConvergenceError::IdempotencyConflict);
        }
        let encoded = serde_json::to_string(&verdict)
            .map_err(|error| ConvergenceError::Serialization(error.to_string()))?;
        sqlx::query("UPDATE agent_convergence_cycle SET verdict_json = ? WHERE convergence_id = ? AND ordinal = ?").bind(encoded).bind(id.as_str()).bind(i64::from(ordinal)).execute(&self.pool).await.map_err(sqlite_error)?;
        load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))
    }

    async fn set_decision(
        &self,
        id: &ConvergenceId,
        ordinal: u8,
        expected_revision: u64,
        decision: ConvergenceDecision,
    ) -> Result<ConvergenceRecord, ConvergenceError> {
        let record = load_record(&self.pool, id)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))?;
        if record.revision != expected_revision
            || record.status != ConvergenceStatus::AwaitingDecision
        {
            return Err(ConvergenceError::RevisionConflict);
        }
        let cycle = load_cycle(&self.pool, id, ordinal)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(format!("{id}:{ordinal}")))?;
        if cycle.decision.is_some() {
            return Err(ConvergenceError::RevisionConflict);
        }
        if !decision_allowed(cycle.verdict.as_ref(), decision) {
            return Err(ConvergenceError::InvalidInput(
                "owner decision is not valid for the persisted verdict".into(),
            ));
        }
        if matches!(
            decision,
            ConvergenceDecision::Repair | ConvergenceDecision::Replan
        ) && record.current_cycle.saturating_add(1) >= record.max_cycles
        {
            return Err(ConvergenceError::LimitExceeded(
                "convergence cycle budget is exhausted".into(),
            ));
        }
        let next = match decision {
            ConvergenceDecision::Accept | ConvergenceDecision::Stop => ConvergenceStatus::Completed,
            ConvergenceDecision::Repair => ConvergenceStatus::Repairing,
            ConvergenceDecision::Replan => ConvergenceStatus::Replanning,
            ConvergenceDecision::Escalate => ConvergenceStatus::Exhausted,
        };
        validate_transition(record.status, next)?;
        let decision_str = match decision {
            ConvergenceDecision::Accept => "accept",
            ConvergenceDecision::Repair => "repair",
            ConvergenceDecision::Replan => "replan",
            ConvergenceDecision::Stop => "stop",
            ConvergenceDecision::Escalate => "escalate",
        };
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool.begin().await.map_err(sqlite_error)?;
        let cycle_update = sqlx::query("UPDATE agent_convergence_cycle SET decision = ?, completed_at = ? WHERE convergence_id = ? AND ordinal = ? AND decision IS NULL").bind(decision_str).bind(next.is_terminal().then_some(now)).bind(id.as_str()).bind(i64::from(ordinal)).execute(&mut *tx).await.map_err(sqlite_error)?;
        if cycle_update.rows_affected() == 0 {
            tx.rollback().await.map_err(sqlite_error)?;
            return Err(ConvergenceError::RevisionConflict);
        }
        let cycle_increment = i64::from(matches!(
            next,
            ConvergenceStatus::Repairing | ConvergenceStatus::Replanning
        ));
        let record_update = sqlx::query("UPDATE agent_convergence SET status = ?, current_cycle = current_cycle + ?, updated_at = ?, terminal_at = ?, revision = revision + 1 WHERE id = ? AND revision = ? AND status = 'awaiting_decision'").bind(next.as_str()).bind(cycle_increment).bind(now).bind(next.is_terminal().then_some(now)).bind(id.as_str()).bind(expected_revision as i64).execute(&mut *tx).await.map_err(sqlite_error)?;
        if record_update.rows_affected() == 0 {
            tx.rollback().await.map_err(sqlite_error)?;
            return Err(ConvergenceError::RevisionConflict);
        }
        tx.commit().await.map_err(sqlite_error)?;
        load_record(&self.pool, id)
            .await?
            .ok_or_else(|| ConvergenceError::NotFound(id.to_string()))
    }

    async fn list_nonterminal(
        &self,
        limit: usize,
    ) -> Result<Vec<ConvergenceRecord>, ConvergenceError> {
        let rows = sqlx::query("SELECT id FROM agent_convergence WHERE status NOT IN ('completed', 'failed', 'cancelled', 'exhausted') ORDER BY updated_at, id LIMIT ?").bind(limit.min(MAX_CONVERGENCE_LIST) as i64).fetch_all(&self.pool).await.map_err(sqlite_error)?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let id = ConvergenceId::parse(&row.get::<String, _>("id"))?;
            if let Some(record) = load_record(&self.pool, &id).await? {
                output.push(record);
            }
        }
        Ok(output)
    }
}

fn validate_string(value: &str, field: &str, max_bytes: usize) -> Result<(), ConvergenceError> {
    if value.len() > max_bytes {
        return Err(ConvergenceError::LimitExceeded(format!(
            "{field} exceeds {max_bytes} bytes"
        )));
    }
    if value.bytes().any(|byte| byte == 0) || value.chars().any(char::is_control) {
        return Err(ConvergenceError::InvalidInput(format!(
            "{field} contains control characters"
        )));
    }
    Ok(())
}

fn validate_commit(value: Option<&str>, field: &str) -> Result<(), ConvergenceError> {
    if let Some(value) = value {
        validate_string(value, field, 128)?;
    }
    Ok(())
}

fn bound(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn digest(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{AgentRunGroupId, AgentRunId};

    fn spec() -> ConvergenceSpec {
        ConvergenceSpec::new(
            "ship the bounded change",
            vec!["tests pass".into(), "review evidence".into()],
        )
        .unwrap()
    }
    fn input() -> NewConvergence {
        NewConvergence {
            id: ConvergenceId::new(),
            owner: AgentOrchestrationOwner::Turn {
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
            },
            spec: spec(),
            max_cycles: 2,
            idempotency_key: "invoke-1".into(),
        }
    }
    fn result(status: AgentRunResultStatus) -> AgentRunResult {
        AgentRunResult {
            run_id: AgentRunId::new(),
            status,
            summary: "bounded result".into(),
            worktree_id: None,
            base_commit: Some("base".into()),
            result_commit: Some("result".into()),
            changed_paths: vec!["src/lib.rs".into()],
            validation: vec![ValidationEvidence {
                kind: "test".into(),
                status: crate::run_result::ValidationStatus::Passed,
                summary: "ok".into(),
            }],
            findings: vec![],
            artifacts: vec![AgentRunArtifact {
                kind: "report".into(),
                label: "handle".into(),
                reference: Some("artifact-1".into()),
            }],
            repository_state: RepositoryState::Clean,
            retryability: crate::run_result::Retryability::NotRetryable,
            recovery_hint: None,
        }
    }

    #[test]
    fn state_machine_covers_valid_graph_and_terminal_monotonicity() {
        for (from, targets) in [
            (
                ConvergenceStatus::Pending,
                vec![ConvergenceStatus::Producing, ConvergenceStatus::Cancelled],
            ),
            (
                ConvergenceStatus::Producing,
                vec![
                    ConvergenceStatus::Verifying,
                    ConvergenceStatus::Failed,
                    ConvergenceStatus::Cancelled,
                ],
            ),
            (
                ConvergenceStatus::Verifying,
                vec![
                    ConvergenceStatus::AwaitingDecision,
                    ConvergenceStatus::Failed,
                    ConvergenceStatus::Cancelled,
                ],
            ),
            (
                ConvergenceStatus::AwaitingDecision,
                vec![
                    ConvergenceStatus::Completed,
                    ConvergenceStatus::Repairing,
                    ConvergenceStatus::Replanning,
                    ConvergenceStatus::Failed,
                    ConvergenceStatus::Cancelled,
                    ConvergenceStatus::Exhausted,
                ],
            ),
            (
                ConvergenceStatus::Repairing,
                vec![
                    ConvergenceStatus::Producing,
                    ConvergenceStatus::Failed,
                    ConvergenceStatus::Cancelled,
                    ConvergenceStatus::Exhausted,
                ],
            ),
            (
                ConvergenceStatus::Replanning,
                vec![
                    ConvergenceStatus::Producing,
                    ConvergenceStatus::Failed,
                    ConvergenceStatus::Cancelled,
                    ConvergenceStatus::Exhausted,
                ],
            ),
        ] {
            for target in targets {
                validate_transition(from, target).unwrap();
            }
        }
        assert!(
            validate_transition(ConvergenceStatus::Completed, ConvergenceStatus::Producing)
                .is_err()
        );
        assert!(
            validate_transition(ConvergenceStatus::Producing, ConvergenceStatus::Pending).is_err()
        );
    }

    #[test]
    fn marked_verdict_parser_is_strict_and_fails_closed() {
        let parsed = SemanticVerificationVerdict::parse_marked(
            "<convergence_verdict>{\"kind\":\"pass\",\"summary\":\"ok\",\"evidence_refs\":[\"run:1\"]}</convergence_verdict>",
        )
        .unwrap();
        assert!(matches!(parsed, SemanticVerificationVerdict::Pass { .. }));
        assert!(SemanticVerificationVerdict::parse_marked("pass").is_err());
        assert!(SemanticVerificationVerdict::parse_marked(
            "<convergence_verdict>{\"kind\":\"pass\",\"summary\":\"ok\",\"evidence_refs\":[]}</convergence_verdict> trailing"
        )
        .is_err());
    }

    #[test]
    fn verifier_packet_contains_only_bounded_run_result_evidence() {
        let packet =
            assemble_verifier_evidence(&spec(), &[result(AgentRunResultStatus::Succeeded)])
                .unwrap();
        let encoded = packet.encode_bounded().unwrap();
        assert!(encoded.contains("bounded result"));
        assert!(!encoded.contains("transcript"));
        assert!(!encoded.contains("reasoning"));
    }

    #[test]
    fn spec_digest_and_limits_are_host_owned() {
        let accepted = spec();
        assert_eq!(accepted.objective_digest.len(), MAX_DIGEST_BYTES);
        assert!(ConvergenceSpec::new("x".repeat(MAX_OBJECTIVE_BYTES + 1), vec![]).is_err());
        assert!(ConvergenceSpec::new("x", vec!["x".repeat(MAX_CRITERION_BYTES + 1)]).is_err());
        let mut tampered = accepted;
        tampered.criteria[0] = "tampered".into();
        assert!(tampered.validate().is_err());
    }

    #[tokio::test]
    async fn memory_store_is_idempotent_and_revision_checked() {
        let store = InMemoryConvergenceStore::new();
        let create = input();
        let first = store.create_or_get(create.clone()).await.unwrap();
        let retry = store.create_or_get(create).await.unwrap();
        assert_eq!(first, retry);
        let mut conflicting = input();
        conflicting.idempotency_key = first.idempotency_key.clone();
        conflicting.spec = ConvergenceSpec::new("different", vec![]).unwrap();
        assert!(matches!(
            store.create_or_get(conflicting).await,
            Err(ConvergenceError::IdempotencyConflict)
        ));
        let producing = store
            .transition(&first.id, 0, ConvergenceStatus::Producing)
            .await
            .unwrap();
        assert!(matches!(
            store
                .transition(&first.id, 0, ConvergenceStatus::Cancelled)
                .await,
            Err(ConvergenceError::RevisionConflict)
        ));
        let cycle = store.create_cycle(&first.id, 0).await.unwrap();
        let cycle = store
            .set_producer_references(
                &first.id,
                cycle.ordinal,
                Some(AgentRunGroupId::new()),
                vec![AgentRunId::new()],
            )
            .await
            .unwrap();
        assert_eq!(cycle.producer_run_ids.len(), 1);
        assert!(store.create_cycle(&first.id, 0).await.is_err());
        assert_eq!(producing.revision, 1);
    }

    #[tokio::test]
    async fn repair_decision_advances_exactly_one_cycle() {
        let store = InMemoryConvergenceStore::new();
        let created = store.create_or_get(input()).await.unwrap();
        let producing = store
            .transition(&created.id, created.revision, ConvergenceStatus::Producing)
            .await
            .unwrap();
        store.create_cycle(&created.id, 0).await.unwrap();
        let verifying = store
            .transition(
                &created.id,
                producing.revision,
                ConvergenceStatus::Verifying,
            )
            .await
            .unwrap();
        let awaiting = store
            .transition(
                &created.id,
                verifying.revision,
                ConvergenceStatus::AwaitingDecision,
            )
            .await
            .unwrap();
        store
            .set_verdict(
                &created.id,
                0,
                SemanticVerificationVerdict::Revise {
                    findings: vec![],
                    repair_requests: vec!["fix it".into()],
                },
            )
            .await
            .unwrap();
        let repairing = store
            .set_decision(
                &created.id,
                0,
                awaiting.revision,
                ConvergenceDecision::Repair,
            )
            .await
            .unwrap();
        assert_eq!(repairing.status, ConvergenceStatus::Repairing);
        assert_eq!(repairing.current_cycle, 1);
        assert_eq!(repairing.revision, awaiting.revision + 1);
        store.create_cycle(&created.id, 1).await.unwrap();
        let producing_again = store
            .transition(
                &created.id,
                repairing.revision,
                ConvergenceStatus::Producing,
            )
            .await
            .unwrap();
        assert_eq!(producing_again.current_cycle, 1);
    }

    #[tokio::test]
    async fn sqlite_store_round_trips_spec_cycles_and_terminal_decision() {
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        let store = SqliteConvergenceStore::new(pool.clone());
        let created = store.create_or_get(input()).await.unwrap();
        let reloaded = store.get(&created.id).await.unwrap().unwrap();
        assert_eq!(reloaded.spec, created.spec);
        assert_eq!(store.list_nonterminal(1).await.unwrap().len(), 1);

        let producing = store
            .transition(&created.id, created.revision, ConvergenceStatus::Producing)
            .await
            .unwrap();
        let cycle = store.create_cycle(&created.id, 0).await.unwrap();
        assert!(matches!(
            store.create_cycle(&created.id, 0).await,
            Err(ConvergenceError::CycleConflict)
        ));
        let cycle = store
            .set_producer_references(
                &created.id,
                cycle.ordinal,
                Some(AgentRunGroupId::new()),
                vec![AgentRunId::new()],
            )
            .await
            .unwrap();
        let verifier = AgentRunId::new();
        store
            .set_verifier_run(&created.id, cycle.ordinal, verifier)
            .await
            .unwrap();
        store
            .transition(
                &created.id,
                producing.revision,
                ConvergenceStatus::Verifying,
            )
            .await
            .unwrap();
        store
            .set_verdict(
                &created.id,
                cycle.ordinal,
                SemanticVerificationVerdict::Pass {
                    summary: "reviewed bounded evidence".into(),
                    evidence_refs: vec!["artifact-1".into()],
                },
            )
            .await
            .unwrap();
        let awaiting = store
            .transition(&created.id, 2, ConvergenceStatus::AwaitingDecision)
            .await
            .unwrap();
        let completed = store
            .set_decision(
                &created.id,
                cycle.ordinal,
                awaiting.revision,
                ConvergenceDecision::Accept,
            )
            .await
            .unwrap();
        assert_eq!(completed.status, ConvergenceStatus::Completed);
        assert_eq!(store.list_nonterminal(10).await.unwrap().len(), 0);

        crate::session::schema::migrate(&pool).await.unwrap();
        let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version, 49);

        sqlx::query("DROP TABLE agent_convergence_cycle")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DROP TABLE agent_convergence")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE migration_version SET version = 48 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        let migrated_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('agent_convergence', 'agent_convergence_cycle')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(migrated_tables, 2);
    }

    #[test]
    fn evidence_is_bounded_deterministic_and_keeps_failure_visible() {
        let failed = result(AgentRunResultStatus::Failed);
        let packet_a = assemble_verifier_evidence(&spec(), std::slice::from_ref(&failed)).unwrap();
        let packet_b = assemble_verifier_evidence(&spec(), std::slice::from_ref(&failed)).unwrap();
        assert_eq!(packet_a, packet_b);
        assert_eq!(
            packet_a.producer_runs[0].status,
            AgentRunResultStatus::Failed
        );
        assert_eq!(packet_a.repository_state, RepositoryState::Clean);
        assert!(packet_a.encode_bounded().unwrap().len() <= MAX_VERIFIER_PACKET_BYTES);
        assert!(assemble_verifier_evidence(&spec(), &[]).is_err());
    }

    #[test]
    fn reconciliation_never_invents_success_or_repeats_terminal_work() {
        let record = ConvergenceRecord {
            id: ConvergenceId::new(),
            owner: input().owner,
            spec: spec(),
            status: ConvergenceStatus::Producing,
            current_cycle: 0,
            max_cycles: 2,
            created_at: 0,
            updated_at: 0,
            terminal_at: None,
            revision: 0,
            idempotency_key: "key".into(),
            request_fingerprint: "fingerprint".into(),
        };
        let cycle = empty_cycle(&record.id, 0);
        assert_eq!(
            classify_reconciliation(ReconciliationInput {
                record: &record,
                cycle: Some(&cycle),
                producer_group_status: Some(RunGroupStatus::Completed),
                producer_run_statuses: &[],
                verifier_status: None
            }),
            ReconciliationAction::AdvanceToVerifying
        );
        let verifying = ConvergenceRecord {
            status: ConvergenceStatus::Verifying,
            ..record.clone()
        };
        assert!(matches!(
            classify_reconciliation(ReconciliationInput {
                record: &verifying,
                cycle: Some(&cycle),
                producer_group_status: None,
                producer_run_statuses: &[],
                verifier_status: Some(AgentRunStatus::Completed)
            }),
            ReconciliationAction::NeedsAttention { .. }
        ));
        let terminal = ConvergenceRecord {
            status: ConvergenceStatus::Completed,
            ..record
        };
        assert_eq!(
            classify_reconciliation(ReconciliationInput {
                record: &terminal,
                cycle: None,
                producer_group_status: None,
                producer_run_statuses: &[],
                verifier_status: None
            }),
            ReconciliationAction::NoChange
        );
    }
}
