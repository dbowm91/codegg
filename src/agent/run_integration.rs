//! Explicit parent-side integration for isolated agent results.
//!
//! Completing a child never changes the parent repository. A caller must
//! present a durable run and a target repository and choose one typed Git
//! operation. This service validates lineage/base identity before dispatching
//! through the canonical mutation executor.

use crate::git_mutations::{resolve_repo_root, GitMutationExecutor};
use crate::git_mutations_ops as git_ops;
use codegg_core::agent_run::AgentRunStore;
use codegg_core::run_result::AgentRunResult;
use codegg_core::worktree_service::WorktreeService;
use codegg_git::MutationOutcome;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationOperation {
    Merge,
    CherryPick,
    Rebase,
}

#[derive(Debug, Clone)]
pub struct AgentRunIntegrationRequest {
    pub run_id: codegg_core::identity::AgentRunId,
    pub target_root: PathBuf,
    pub operation: IntegrationOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunIntegrationResult {
    pub run_id: codegg_core::identity::AgentRunId,
    pub worktree_id: codegg_core::identity::WorktreeId,
    pub base_commit: String,
    pub result_commit: String,
    pub target_root: PathBuf,
    pub operation: IntegrationOperation,
    pub outcome: String,
    pub success: bool,
    pub conflict: bool,
    pub summary: String,
}

#[derive(Debug, Error)]
pub enum AgentRunIntegrationError {
    #[error("agent run lookup failed: {0}")]
    Store(String),
    #[error("agent run has no structured result")]
    MissingResult,
    #[error("agent run has no isolated worktree result")]
    MissingWorktree,
    #[error("integration precondition failed: {0}")]
    Precondition(String),
    #[error("typed integration failed: {0}")]
    Git(String),
}

pub struct AgentRunIntegrationService {
    runs: Arc<dyn AgentRunStore>,
    worktrees: Arc<WorktreeService>,
}

impl AgentRunIntegrationService {
    pub fn new(runs: Arc<dyn AgentRunStore>, worktrees: Arc<WorktreeService>) -> Self {
        Self { runs, worktrees }
    }

    pub async fn integrate(
        &self,
        request: AgentRunIntegrationRequest,
    ) -> Result<AgentRunIntegrationResult, AgentRunIntegrationError> {
        let run = self
            .runs
            .get_run(&request.run_id)
            .await
            .map_err(|e| AgentRunIntegrationError::Store(e.to_string()))?
            .ok_or_else(|| AgentRunIntegrationError::Store("run not found".into()))?;
        let result: AgentRunResult = self
            .runs
            .get_result(&request.run_id)
            .await
            .map_err(|e| AgentRunIntegrationError::Store(e.to_string()))?
            .ok_or(AgentRunIntegrationError::MissingResult)?;
        let worktree_id = result
            .worktree_id
            .clone()
            .or(run.worktree_id.clone())
            .ok_or(AgentRunIntegrationError::MissingWorktree)?;
        let worktree = self
            .worktrees
            .get(&worktree_id)
            .await
            .map_err(|e| AgentRunIntegrationError::Precondition(e.to_string()))?;
        let base_commit = result.base_commit.clone().ok_or_else(|| {
            AgentRunIntegrationError::Precondition("result has no base commit".into())
        })?;
        let result_commit = result.result_commit.clone().ok_or_else(|| {
            AgentRunIntegrationError::Precondition("result has no result commit".into())
        })?;
        if worktree.base_commit != base_commit {
            return Err(AgentRunIntegrationError::Precondition(
                "result base commit does not match managed worktree".into(),
            ));
        }
        let target = resolve_repo_root(&request.target_root)
            .map_err(|e| AgentRunIntegrationError::Precondition(e.to_string()))?;
        if target.as_path() != worktree.repository_root.as_path() {
            return Err(AgentRunIntegrationError::Precondition(
                "integration target is not the source repository".into(),
            ));
        }
        let target_status = egggit::status_v2::rich_repo_status(target.as_path())
            .await
            .map_err(|e| AgentRunIntegrationError::Precondition(e.to_string()))?;
        if target_status.head.as_deref() != Some(base_commit.as_str()) {
            return Err(AgentRunIntegrationError::Precondition(
                "parent repository moved since the child base was captured".into(),
            ));
        }
        if !target_status.is_clean {
            return Err(AgentRunIntegrationError::Precondition(
                "parent repository must be clean before integration".into(),
            ));
        }

        let executor = GitMutationExecutor::new();
        let mutation = match &request.operation {
            IntegrationOperation::Merge => {
                git_ops::merge(
                    &executor,
                    target.as_path(),
                    vec![result_commit.clone()],
                    true,
                    None,
                )
                .await
            }
            IntegrationOperation::CherryPick => {
                git_ops::cherry_pick(&executor, target.as_path(), vec![result_commit.clone()]).await
            }
            IntegrationOperation::Rebase => {
                git_ops::rebase(&executor, target.as_path(), Some(&result_commit), None).await
            }
        }
        .map_err(|e| AgentRunIntegrationError::Git(e.to_string()))?;
        let conflict = matches!(mutation.outcome, MutationOutcome::Conflict);
        Ok(AgentRunIntegrationResult {
            run_id: request.run_id,
            worktree_id,
            base_commit,
            result_commit,
            target_root: target.as_path().to_path_buf(),
            operation: request.operation,
            outcome: mutation.outcome.label().into(),
            success: mutation.success,
            conflict,
            summary: if conflict {
                "integration produced a recoverable conflict; parent repository retained for typed recovery".into()
            } else {
                "isolated agent result integrated into the parent repository".into()
            },
        })
    }
}
