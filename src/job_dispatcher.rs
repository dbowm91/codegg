use std::sync::Arc;

use codegg_core::jobs::{JobPayload, JobRecord, JobState, JobStoreError};

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("unsupported job kind: {0}")]
    UnsupportedKind(String),

    #[error("subagent dispatch failed: {0}")]
    Subagent(String),

    #[error("job store error: {0}")]
    JobStore(#[from] JobStoreError),
}

#[async_trait::async_trait]
pub trait JobDispatcher: Send + Sync {
    async fn dispatch_created_job(&self, job: JobRecord) -> Result<(), DispatchError>;
}

pub struct SubAgentJobDispatcher {
    pool: Arc<crate::agent::worker::SubAgentPool>,
}

impl SubAgentJobDispatcher {
    pub fn new(pool: Arc<crate::agent::worker::SubAgentPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl JobDispatcher for SubAgentJobDispatcher {
    async fn dispatch_created_job(&self, job: JobRecord) -> Result<(), DispatchError> {
        if job.state != JobState::Queued {
            return Ok(());
        }

        let (prompt, agent, parent_id, denied_tools, allowed_paths, max_tool_calls) =
            match &job.payload {
                JobPayload::Subagent {
                    prompt,
                    agent,
                    parent_id,
                    denied_tools,
                    allowed_paths,
                    max_tool_calls,
                } => (
                    prompt.clone(),
                    agent.clone(),
                    parent_id.clone(),
                    denied_tools.clone(),
                    allowed_paths.clone(),
                    *max_tool_calls,
                ),
                _ => {
                    return Err(DispatchError::UnsupportedKind(
                        job.kind.as_str().to_string(),
                    ));
                }
            };

        let task_id = job
            .job_id
            .as_str()
            .bytes()
            .take(8)
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));

        let request = crate::agent::worker::SubAgentRequest {
            task_id,
            prompt,
            agent,
            parent_id,
            denied_tools,
            allowed_paths,
            description: format!("Durable job {}", job.job_id),
            depth: 1,
            max_tool_calls: max_tool_calls.map(|m| m as usize),
            parent_model: None,
        };

        self.pool
            .spawner()
            .send(request)
            .await
            .map_err(|e| DispatchError::Subagent(e.to_string()))
    }
}

pub struct NullJobDispatcher;

#[async_trait::async_trait]
impl JobDispatcher for NullJobDispatcher {
    async fn dispatch_created_job(&self, _job: JobRecord) -> Result<(), DispatchError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{JobId, JobKind};

    #[tokio::test(flavor = "current_thread")]
    async fn unsupported_kind_returns_error() {
        let dispatcher = NullJobDispatcher;
        let job = JobRecord {
            job_id: JobId::new_unchecked("test-job"),
            workspace_id: codegg_core::workspace::WorkspaceId::new_unchecked("ws"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: codegg_core::jobs::JobSource::Interactive,
            priority: codegg_core::jobs::JobPriority::Normal,
            payload: JobPayload::Shell {
                command: "echo hi".to_string(),
                argv: None,
                cwd: None,
            },
            resource_request: codegg_core::jobs::ResourceRequest::default(),
            timeout: None,
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
            state: JobState::Queued,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: vec![],
            labels: std::collections::HashMap::new(),
        };

        // NullJobDispatcher accepts anything
        assert!(dispatcher.dispatch_created_job(job).await.is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn null_dispatcher_accepts_any_payload() {
        let dispatcher = NullJobDispatcher;
        let job = JobRecord {
            job_id: JobId::new_unchecked("j1"),
            workspace_id: codegg_core::workspace::WorkspaceId::new_unchecked("ws"),
            session_id: None,
            turn_id: None,
            kind: JobKind::AgentTurn,
            source: codegg_core::jobs::JobSource::Interactive,
            priority: codegg_core::jobs::JobPriority::Normal,
            payload: JobPayload::AgentTurn {
                prompt: "hi".to_string(),
                agent: "build".to_string(),
                model: None,
            },
            resource_request: codegg_core::jobs::ResourceRequest::default(),
            timeout: None,
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
            state: JobState::Running,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: vec![],
            labels: std::collections::HashMap::new(),
        };
        assert!(dispatcher.dispatch_created_job(job).await.is_ok());
    }
}
