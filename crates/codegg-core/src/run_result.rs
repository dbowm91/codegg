//! Bounded, machine-derived results for durable delegated agent runs.
//!
//! The human-readable subagent transcript remains useful for explanation, but
//! it is deliberately not the authority for repository state or integration.
//! This DTO is the small durable contract consumed by the parent and operator
//! surfaces.

use crate::identity::{AgentRunId, WorktreeId};
use serde::{Deserialize, Serialize};

pub const MAX_RESULT_BYTES: usize = 32 * 1024;
pub const MAX_RESULT_PATHS: usize = 256;
pub const MAX_RESULT_PATH_BYTES: usize = 1024;
pub const MAX_RESULT_SUMMARY_BYTES: usize = 4096;
pub const MAX_RESULT_ITEMS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunResultStatus {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    Conflicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Passed,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationEvidence {
    pub kind: String,
    pub status: ValidationStatus,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunFinding {
    pub severity: String,
    pub title: String,
    pub rationale: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunArtifact {
    pub kind: String,
    pub label: String,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryState {
    Clean,
    Dirty,
    Conflicted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retryability {
    Retryable,
    NotRetryable,
    RequiresRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub run_id: AgentRunId,
    pub status: AgentRunResultStatus,
    pub summary: String,
    pub worktree_id: Option<WorktreeId>,
    pub base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub changed_paths: Vec<String>,
    pub validation: Vec<ValidationEvidence>,
    pub findings: Vec<AgentRunFinding>,
    pub artifacts: Vec<AgentRunArtifact>,
    pub repository_state: RepositoryState,
    pub retryability: Retryability,
    pub recovery_hint: Option<String>,
}

impl AgentRunResult {
    /// Enforce collection and string bounds before any result crosses a
    /// persistence or protocol boundary.
    pub fn bounded(mut self) -> Self {
        self.summary = bound(&self.summary, MAX_RESULT_SUMMARY_BYTES);
        self.base_commit = self.base_commit.map(|v| bound(&v, 128));
        self.result_commit = self.result_commit.map(|v| bound(&v, 128));
        self.recovery_hint = self.recovery_hint.map(|v| bound(&v, 1024));
        self.changed_paths = self
            .changed_paths
            .into_iter()
            .take(MAX_RESULT_PATHS)
            .map(|v| bound(&v, MAX_RESULT_PATH_BYTES))
            .collect();
        self.validation = self
            .validation
            .into_iter()
            .take(MAX_RESULT_ITEMS)
            .map(|mut v| {
                v.kind = bound(&v.kind, 128);
                v.summary = bound(&v.summary, 1024);
                v
            })
            .collect();
        self.findings = self
            .findings
            .into_iter()
            .take(MAX_RESULT_ITEMS)
            .map(|mut v| {
                v.severity = bound(&v.severity, 32);
                v.title = bound(&v.title, 512);
                v.rationale = bound(&v.rationale, 2048);
                v.file = v.file.map(|p| bound(&p, MAX_RESULT_PATH_BYTES));
                v
            })
            .collect();
        self.artifacts = self
            .artifacts
            .into_iter()
            .take(MAX_RESULT_ITEMS)
            .map(|mut v| {
                v.kind = bound(&v.kind, 128);
                v.label = bound(&v.label, 512);
                v.reference = v.reference.map(|r| bound(&r, 1024));
                v
            })
            .collect();
        // The per-field bounds above protect individual values. Keep the
        // serialized envelope bounded as well; large finding/path collections
        // must never make persistence or protocol publication unbounded.
        while serde_json::to_vec(&self)
            .map(|bytes| bytes.len() > MAX_RESULT_BYTES)
            .unwrap_or(true)
        {
            if self.artifacts.pop().is_some()
                || self.findings.pop().is_some()
                || self.validation.pop().is_some()
                || self.changed_paths.pop().is_some()
            {
                continue;
            }
            if self.summary.is_empty() {
                break;
            }
            let next = self.summary.len().saturating_sub(256);
            self.summary = bound(&self.summary, next);
        }
        self
    }

    pub fn encode_bounded(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.clone().bounded())
    }
}

fn bound(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = 0;
    for ch in value.chars() {
        if end + ch.len_utf8() > max {
            break;
        }
        end += ch.len_utf8();
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::AgentRunId;

    #[test]
    fn result_is_bounded_before_serialization() {
        let result = AgentRunResult {
            run_id: AgentRunId::new(),
            status: AgentRunResultStatus::Succeeded,
            summary: "x".repeat(MAX_RESULT_SUMMARY_BYTES + 10),
            worktree_id: None,
            base_commit: None,
            result_commit: None,
            changed_paths: (0..MAX_RESULT_PATHS + 10)
                .map(|_| "a/long-file".to_string())
                .collect(),
            validation: Vec::new(),
            findings: Vec::new(),
            artifacts: Vec::new(),
            repository_state: RepositoryState::Clean,
            retryability: Retryability::NotRetryable,
            recovery_hint: None,
        };
        let encoded = result.encode_bounded().unwrap();
        assert!(encoded.len() <= MAX_RESULT_BYTES);
        let decoded: AgentRunResult = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.changed_paths.len(), MAX_RESULT_PATHS);
        assert_eq!(decoded.summary.len(), MAX_RESULT_SUMMARY_BYTES);
    }
}
