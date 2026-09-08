//! Snapshot capture and security-review trigger evaluation.
//!
//! Physical decomposition of [`super::r#loop`] (M003): durable edit-snapshot
//! capture (full and incremental), file-change event draining, and the
//! heuristic security-review dispatch. Snapshot ownership stays with the
//! snapshot manager; security-review work dispatches through the scheduler
//! submission service with the standalone-compat pool fallback preserved.

use super::r#loop::AgentLoop;
use crate::bus::events::AppEvent;
use tokio::sync::broadcast::error::TryRecvError;

impl AgentLoop {
    /// Capture a snapshot of the project state if snapshot_manager is configured
    #[allow(dead_code)]
    pub(super) async fn capture_snapshot_if_needed(&mut self) {
        if let Some(ref mut snapshot_manager) = self.services.snapshot_manager {
            let session_id = self.session_id.clone();
            match snapshot_manager.capture(&session_id, None).await {
                Ok(snapshot) => {
                    tracing::info!(
                        "Snapshot captured: {} with {} files",
                        snapshot.id,
                        snapshot.files.len()
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to capture snapshot: {}", e);
                }
            }
        }
    }

    /// Evaluate heuristics and optionally spawn the security-review subagent.
    ///
    /// Triggers when:
    /// - A tool call is classified as high-risk by SecurityService
    /// - A file edit touches a sensitive path
    /// - `at_session_end` is true (pre-commit style review)
    ///
    /// Spawns as a background task — never blocks the main agent loop.
    pub(super) fn maybe_spawn_security_review(
        &self,
        triggered_findings: &[&crate::security::finding::SecurityFinding],
        edited_paths: &[String],
        at_session_end: bool,
    ) {
        let _sec_config = match self.services.config.security.as_ref() {
            Some(c) if c.auto_invoke_review_agent && c.enabled => c,
            _ => return,
        };

        if !at_session_end && triggered_findings.is_empty() && edited_paths.is_empty() {
            return;
        }

        let mut context_parts = Vec::new();

        if at_session_end {
            context_parts.push("Pre-commit security review requested.".to_string());
        }

        if !edited_paths.is_empty() {
            context_parts.push(format!(
                "Files modified this session:\n{}",
                edited_paths
                    .iter()
                    .map(|p| format!("- {}", p))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !triggered_findings.is_empty() {
            let finding_lines: Vec<String> = triggered_findings
                .iter()
                .take(10)
                .map(|f| format!("- {}", f.compact_summary()))
                .collect();
            context_parts.push(format!(
                "Security findings from tool classification:\n{}",
                finding_lines.join("\n")
            ));
        }

        if let Some(ref prompt) = self.original_user_prompt {
            context_parts.push(format!("Original user task: {}", prompt));
        }

        let prompt = format!(
            "Review the following changes and findings for realistic security regressions.\n\n{}",
            context_parts.join("\n\n")
        );

        let task_id = rand::random::<u64>();
        let session_id = self.session_id.clone();
        let agent = "security-review".to_string();
        let parent_model = self
            .services
            .agents
            .get(&self.state.current_agent)
            .and_then(|a| a.model.clone());
        if let Some(submission) = self.submission.clone() {
            let workspace_root = self.workspace_root.clone();
            // scheduler-owned: daemon-mode security-review path
            // dispatches through JobSubmissionService.
            tokio::spawn(async move {
                let workspace_id = match submission.workspace_id_for_root(&workspace_root).await {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to resolve security-review workspace");
                        return;
                    }
                };
                let spec = codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: Some(session_id.clone()),
                    turn_id: None,
                    kind: codegg_core::jobs::JobKind::Subagent,
                    source: codegg_core::jobs::JobSource::AgentDelegated,
                    priority: codegg_core::jobs::JobPriority::Background,
                    payload: codegg_core::jobs::JobPayload::Subagent {
                        prompt,
                        agent,
                        model: None,
                        parent_id: Some(session_id),
                        denied_tools: Vec::new(),
                        allowed_paths: vec![workspace_root.to_string_lossy().into_owned()],
                        max_tool_calls: None,
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Subagent,
                    ),
                    timeout: None,
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
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
                };
                if let Err(e) = submission.submit(None, spec).await {
                    tracing::warn!(error = %e, "failed to submit security-review subagent");
                }
            });
            return;
        }
        let Some(pool) = self.subagent_pool.clone() else {
            return;
        };
        // scheduler-audit: standalone-compat
        // security-review fallback when the daemon is not wired with
        // a JobSubmissionService (explicit --standalone / test harness).
        let request = crate::agent::worker::SubAgentRequest {
            task_id,
            run_id: None,
            prompt,
            agent,
            parent_id: Some(session_id),
            parent_run_id: None,
            denied_tools: Vec::new(),
            allowed_paths: Vec::new(),
            description: "Auto-triggered security review".to_string(),
            depth: 1,
            max_tool_calls: None,
            parent_model,
            workspace_root: Some(self.workspace_root.clone()),
            workspace_locks: self.workspace_locks.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = pool.spawner().send(request).await {
                tracing::warn!("Failed to spawn security-review subagent: {}", e);
            }
        });
    }

    pub(super) fn drain_file_change_events(&mut self) -> Vec<(String, Option<String>)> {
        let mut changes = Vec::new();
        loop {
            match self.services.file_change_rx.try_recv() {
                Ok(AppEvent::FileChanged {
                    path, old_content, ..
                }) => {
                    changes.push((path, old_content));
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Lagged(skipped)) => {
                    tracing::warn!("FileChanged stream lagged, skipped {skipped} events");
                }
                Err(TryRecvError::Closed) => break,
            }
        }
        changes
    }

    #[allow(dead_code)]
    pub(super) async fn capture_incremental_snapshot_if_needed(&mut self, label: Option<String>) {
        if self.services.snapshot_manager.is_none() {
            return;
        }

        let changes = self.drain_file_change_events();
        if changes.is_empty() {
            return;
        }

        if let Some(ref snapshot_manager) = self.services.snapshot_manager {
            match snapshot_manager
                .capture_incremental(&self.session_id, label, changes)
                .await
            {
                Ok(Some(snapshot)) => {
                    tracing::info!(
                        "Incremental snapshot captured: {} with {} files",
                        snapshot.id,
                        snapshot.files.len()
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Failed to capture incremental snapshot: {}", e);
                }
            }
        }
    }
}
