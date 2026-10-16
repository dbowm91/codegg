use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::agent::r#loop::AgentLoop;
use crate::agent::Agent;
use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::config::schema::Config;
use crate::permission::PermissionChecker;
use crate::provider::ProviderRegistry;
use crate::session::SessionStore;
use crate::tool::task::TaskStore;
use crate::tool::ToolRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentFinding {
    pub severity: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub title: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubAgentReport {
    pub summary: String,
    pub files_examined: Vec<String>,
    pub commands_run: Vec<String>,
    pub findings: Vec<SubAgentFinding>,
    pub next_steps: Vec<String>,
    pub confidence: Option<String>,
}

impl SubAgentReport {
    pub fn to_compact_text(&self) -> String {
        let mut lines = vec![self.summary.clone()];
        if !self.files_examined.is_empty() {
            lines.push(format!("Files: {}", self.files_examined.join(", ")));
        }
        if !self.commands_run.is_empty() {
            lines.push(format!("Commands: {}", self.commands_run.join(", ")));
        }
        if !self.findings.is_empty() {
            for f in &self.findings {
                let loc = f
                    .file
                    .as_ref()
                    .map(|file| {
                        format!(
                            " ({}{})",
                            file,
                            f.line.map(|l| format!(":{}", l)).unwrap_or_default()
                        )
                    })
                    .unwrap_or_default();
                lines.push(format!(
                    "[{}] {}{}: {}",
                    f.severity.as_deref().unwrap_or("info"),
                    f.title,
                    loc,
                    f.rationale
                ));
            }
        }
        if !self.next_steps.is_empty() {
            lines.push(format!("Next: {}", self.next_steps.join("; ")));
        }
        if let Some(ref conf) = self.confidence {
            lines.push(format!("Confidence: {}", conf));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct SubAgentRequest {
    pub task_id: u64,
    /// Durable identity of the run being executed, when scheduler-owned.
    pub run_id: Option<codegg_core::identity::AgentRunId>,
    pub prompt: String,
    pub agent: String,
    pub parent_id: Option<String>,
    pub parent_run_id: Option<codegg_core::identity::AgentRunId>,
    pub denied_tools: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub description: String,
    pub depth: usize,
    pub max_tool_calls: Option<usize>,
    pub parent_model: Option<String>,
    pub workspace_root: Option<PathBuf>,
    /// Shared with the owning turn when the child runs in the same workspace.
    pub workspace_locks: Option<Arc<codegg_core::workspace_services::WorkspaceLockTable>>,
    /// M005: parent turn sandbox ceiling (`None` = pre-M005 default
    /// `WorkspaceWrite`). The child effective profile is narrowed against
    /// this and can never broaden it.
    pub parent_sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
    /// M005: explicitly requested child profile (`None` = inherit parent).
    /// A request for `FullHost` under a constrained parent fails closed.
    pub sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
}

/// Stable, typed compatibility lineage for the pre-durable AgentRun path.
/// These identifiers are deliberately separate from display/session IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentLineageId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelegationId(pub String);

#[derive(Debug, Clone)]
pub struct AgentLineageContext {
    pub root_id: AgentLineageId,
    pub parent_id: Option<AgentLineageId>,
    pub delegation_id: DelegationId,
    pub depth: usize,
    pub cancellation: CancellationToken,
}

impl AgentLineageContext {
    pub fn for_request(request: &SubAgentRequest, cancellation: CancellationToken) -> Self {
        let root = request
            .parent_id
            .clone()
            .unwrap_or_else(|| format!("task-root-{}", request.task_id));
        Self {
            root_id: AgentLineageId(root.clone()),
            parent_id: request.parent_id.clone().map(AgentLineageId),
            delegation_id: DelegationId(delegation_key(request)),
            depth: request.depth,
            cancellation,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub task_id: u64,
    pub success: bool,
    pub result: String,
    pub report: Option<SubAgentReport>,
    pub validation: Vec<codegg_core::run_result::ValidationEvidence>,
    pub artifacts: Vec<codegg_core::run_result::AgentRunArtifact>,
}

impl SubAgentResult {
    pub fn success(task_id: u64, result: String) -> Self {
        Self {
            task_id,
            success: true,
            result,
            report: None,
            validation: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    pub fn success_with_report(task_id: u64, result: String, report: SubAgentReport) -> Self {
        Self {
            task_id,
            success: true,
            result,
            report: Some(report),
            validation: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    pub fn failure(task_id: u64, error: String) -> Self {
        Self {
            task_id,
            success: false,
            result: error,
            report: None,
            validation: Vec::new(),
            artifacts: Vec::new(),
        }
    }
}

struct WorkerRequest {
    request: SubAgentRequest,
    approval_mode: codegg_core::approval::ApprovalMode,
    response_tx: oneshot::Sender<SubAgentResult>,
    lease: Option<DescendantAdmissionLease>,
    lineage_token: CancellationToken,
    scheduler_cancel: CancellationToken,
    scheduled: bool,
}

#[derive(Default)]
struct AdmissionState {
    active: usize,
    accepted_tasks: HashSet<u64>,
    delegation_keys: HashMap<String, u64>,
    direct_child_counts: HashMap<String, usize>,
    total_child_tool_calls: usize,
}

struct AdmissionRegistry {
    state: Mutex<AdmissionState>,
    max_active: usize,
    max_direct_children: usize,
    max_total_child_tool_calls: usize,
}

struct DescendantAdmissionLease {
    registry: Arc<AdmissionRegistry>,
    active: bool,
}

impl Drop for DescendantAdmissionLease {
    fn drop(&mut self) {
        if self.active {
            let mut state = self
                .registry
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.active = state.active.saturating_sub(1);
            self.active = false;
        }
    }
}

impl AdmissionRegistry {
    fn admit(
        self: &Arc<Self>,
        request: &SubAgentRequest,
        tool_calls: usize,
    ) -> Result<DescendantAdmissionLease, String> {
        let key = delegation_key(request);
        let parent = request.parent_id.as_deref().unwrap_or("<root>").to_string();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active >= self.max_active {
            return Err("subagent active-descendant limit exceeded".into());
        }
        if let Some(existing) = state.delegation_keys.get(&key) {
            if *existing != request.task_id {
                return Err(format!(
                    "duplicate delegation identity already accepted as task {existing}"
                ));
            }
        }
        if state.accepted_tasks.contains(&request.task_id) {
            return Err(format!("duplicate task identity {}", request.task_id));
        }
        if self.max_direct_children != usize::MAX
            && state.direct_child_counts.get(&parent).copied().unwrap_or(0)
                >= self.max_direct_children
        {
            return Err("subagent direct-child limit exceeded".into());
        }
        if state.total_child_tool_calls.saturating_add(tool_calls) > self.max_total_child_tool_calls
        {
            return Err("subagent total child tool-call budget exceeded".into());
        }
        state.active += 1;
        state.accepted_tasks.insert(request.task_id);
        state.delegation_keys.insert(key, request.task_id);
        *state.direct_child_counts.entry(parent).or_default() += 1;
        state.total_child_tool_calls = state.total_child_tool_calls.saturating_add(tool_calls);
        Ok(DescendantAdmissionLease {
            registry: Arc::clone(self),
            active: true,
        })
    }

    fn active_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active
    }
}

pub struct SubAgentPool {
    max_concurrent: usize,
    max_depth: usize,
    task_store: Arc<TokioMutex<TaskStore>>,
    workers: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>,
    request_tx: mpsc::Sender<WorkerRequest>,
    agents: Arc<Vec<Agent>>,
    provider_registry: Arc<ProviderRegistry>,
    config: Arc<Config>,
    session_store: Arc<SessionStore>,
    cancel_token: CancellationToken,
    active_handles: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>,
    pool: Option<SqlitePool>,
    max_direct_children: usize,
    max_active_descendants: usize,
    max_total_child_tool_calls: usize,
    admission: Arc<AdmissionRegistry>,
    lineage_tokens: Arc<TokioMutex<HashMap<String, CancellationToken>>>,
    durable_submission: Arc<TokioMutex<Option<Arc<crate::scheduler::JobSubmissionService>>>>,
    durable_agent_runs: Arc<TokioMutex<Option<Arc<dyn codegg_core::agent_run::AgentRunStore>>>>,
    durable_run_control: Arc<TokioMutex<Option<Arc<crate::agent::run_control::RunControlService>>>>,
    durable_run_groups:
        Arc<TokioMutex<Option<Arc<codegg_core::agent_run_group::AgentRunGroupService>>>>,
}

impl SubAgentPool {
    pub async fn new(
        config: &Config,
        agents: Vec<Agent>,
        provider_registry: ProviderRegistry,
        session_store: Arc<SessionStore>,
        pool: Option<SqlitePool>,
    ) -> Self {
        let max_concurrent = config
            .subagent
            .as_ref()
            .and_then(|s| s.max_concurrent)
            .unwrap_or(5);
        let max_depth = config
            .subagent
            .as_ref()
            .and_then(|s| s.max_depth)
            .unwrap_or(3);
        let subagent_config = config.subagent.as_ref();
        let max_direct_children = subagent_config
            .and_then(|s| s.max_direct_children)
            .unwrap_or(usize::MAX);
        let max_active_descendants = subagent_config
            .and_then(|s| s.max_active_descendants)
            .unwrap_or(usize::MAX);
        let max_total_child_tool_calls = subagent_config
            .and_then(|s| s.max_total_child_tool_calls)
            .unwrap_or(usize::MAX);
        let (request_tx, request_rx) = mpsc::channel(max_concurrent * 2);
        let task_store = Arc::new(TokioMutex::new(TaskStore::new()));
        if let Some(ref p) = pool {
            task_store.lock().await.set_pool(p.clone());
        }
        let workers = Arc::new(TokioMutex::new(Vec::new()));
        let cancel_token = CancellationToken::new();
        let active_handles = Arc::new(TokioMutex::new(Vec::new()));

        let pool_inst = Self {
            max_concurrent,
            max_depth,
            task_store,
            workers,
            request_tx,
            agents: Arc::new(agents),
            provider_registry: Arc::new(provider_registry),
            config: Arc::new(config.clone()),
            session_store,
            cancel_token,
            active_handles,
            pool,
            max_direct_children,
            max_active_descendants,
            max_total_child_tool_calls,
            admission: Arc::new(AdmissionRegistry {
                state: Mutex::new(AdmissionState::default()),
                max_active: max_active_descendants,
                max_direct_children,
                max_total_child_tool_calls,
            }),
            lineage_tokens: Arc::new(TokioMutex::new(HashMap::new())),
            durable_submission: Arc::new(TokioMutex::new(None)),
            durable_agent_runs: Arc::new(TokioMutex::new(None)),
            durable_run_control: Arc::new(TokioMutex::new(None)),
            durable_run_groups: Arc::new(TokioMutex::new(None)),
        };

        let pool_clone = pool_inst.clone();
        pool_clone.start_worker_loop(request_rx);

        pool_inst
    }

    pub async fn new_with_store(
        config: &Config,
        task_store: Arc<TokioMutex<TaskStore>>,
        agents: Vec<Agent>,
        provider_registry: ProviderRegistry,
        session_store: Arc<SessionStore>,
        pool: Option<SqlitePool>,
    ) -> Self {
        let max_concurrent = config
            .subagent
            .as_ref()
            .and_then(|s| s.max_concurrent)
            .unwrap_or(5);
        let max_depth = config
            .subagent
            .as_ref()
            .and_then(|s| s.max_depth)
            .unwrap_or(3);
        let subagent_config = config.subagent.as_ref();
        let max_direct_children = subagent_config
            .and_then(|s| s.max_direct_children)
            .unwrap_or(usize::MAX);
        let max_active_descendants = subagent_config
            .and_then(|s| s.max_active_descendants)
            .unwrap_or(usize::MAX);
        let max_total_child_tool_calls = subagent_config
            .and_then(|s| s.max_total_child_tool_calls)
            .unwrap_or(usize::MAX);
        let (request_tx, request_rx) = mpsc::channel(max_concurrent * 2);
        let workers = Arc::new(TokioMutex::new(Vec::new()));
        let cancel_token = CancellationToken::new();
        let active_handles = Arc::new(TokioMutex::new(Vec::new()));
        if let Some(ref p) = pool {
            task_store.lock().await.set_pool(p.clone());
        }

        let pool_inst = Self {
            max_concurrent,
            max_depth,
            task_store,
            workers,
            request_tx,
            agents: Arc::new(agents),
            provider_registry: Arc::new(provider_registry),
            config: Arc::new(config.clone()),
            session_store,
            cancel_token,
            active_handles,
            pool,
            max_direct_children,
            max_active_descendants,
            max_total_child_tool_calls,
            admission: Arc::new(AdmissionRegistry {
                state: Mutex::new(AdmissionState::default()),
                max_active: max_active_descendants,
                max_direct_children,
                max_total_child_tool_calls,
            }),
            lineage_tokens: Arc::new(TokioMutex::new(HashMap::new())),
            durable_submission: Arc::new(TokioMutex::new(None)),
            durable_agent_runs: Arc::new(TokioMutex::new(None)),
            durable_run_control: Arc::new(TokioMutex::new(None)),
            durable_run_groups: Arc::new(TokioMutex::new(None)),
        };

        let pool_clone = pool_inst.clone();
        pool_clone.start_worker_loop(request_rx);

        pool_inst
    }

    fn start_worker_loop(&self, mut request_rx: mpsc::Receiver<WorkerRequest>) {
        let cancel_token = self.cancel_token.clone();
        let task_store = Arc::clone(&self.task_store);
        let max_concurrent = self.max_concurrent;
        let agents = Arc::clone(&self.agents);
        let provider_registry = Arc::clone(&self.provider_registry);
        let config = Arc::clone(&self.config);
        let session_store = Arc::clone(&self.session_store);
        let workers = Arc::clone(&self.workers);
        let active_handles = Arc::clone(&self.active_handles);
        let db_pool = self.pool.clone();
        let subagent_pool = Arc::new(self.clone());

        let handle = tokio::spawn(async move {
            let sem = Arc::new(Semaphore::new(max_concurrent));
            let mut cleanup_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            cleanup_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => {
                        tracing::info!("Worker loop received cancellation signal");
                        break;
                    }
                    _ = cleanup_interval.tick() => {
                        let mut handles = active_handles.lock().await;
                        handles.retain(|h| !h.is_finished());
                    }
                    Some(WorkerRequest { request, approval_mode, response_tx, lease, lineage_token, scheduler_cancel, scheduled }) = request_rx.recv() => {
                        if cancel_token.is_cancelled() {
                            drop(lease);
                            if response_tx
                                .send(SubAgentResult::failure(
                                    request.task_id,
                                    "pool shutting down".to_string(),
                                ))
                                .is_err()
                            {
                                tracing::debug!(task_id = request.task_id, "subagent result receiver closed");
                            }
                            continue;
                        }

                        let sem = Arc::clone(&sem);
                        let task_store = Arc::clone(&task_store);
                        let agents = Arc::clone(&agents);
                        let provider_registry = Arc::clone(&provider_registry);
                        let config = Arc::clone(&config);
                        let session_store = Arc::clone(&session_store);
                        let cancel_token = cancel_token.clone();
                        let db_pool = db_pool.clone();
                        let subagent_pool = Arc::clone(&subagent_pool);

                        let handle = tokio::spawn(async move {
                            let _lease = lease;

                            // Wait for semaphore permit, but also check for cancellation
                            let permit = if scheduled {
                                // Scheduler admission already owns the machine
                                // capacity for this execution. Waiting on the
                                // pool semaphore here would create a second
                                // daemon capacity queue.
                                None
                            } else { Some(tokio::select! {
                                biased;
                                _ = cancel_token.cancelled() => {
                                    if response_tx
                                        .send(SubAgentResult::failure(
                                            request.task_id,
                                            "pool shutting down".to_string(),
                                        ))
                                        .is_err()
                                    {
                                        tracing::debug!(task_id = request.task_id, "subagent result receiver closed");
                                    }
                                    return;
                                }
                                _ = lineage_token.cancelled() => {
                                    if response_tx
                                        .send(SubAgentResult::failure(
                                            request.task_id,
                                            "Task cancelled".to_string(),
                                        ))
                                        .is_err()
                                    {
                                        tracing::debug!(task_id = request.task_id, "subagent result receiver closed");
                                    }
                                    return;
                                }
                                result = sem.acquire() => {
                                    match result {
                                        Ok(p) => p,
                                        Err(e) => {
                                            tracing::error!("Failed to acquire semaphore: {}", e);
                                            if response_tx
                                                .send(SubAgentResult::failure(
                                                    request.task_id,
                                                    format!("Worker semaphore error: {}", e),
                                                ))
                                                .is_err()
                                            {
                                                tracing::debug!(task_id = request.task_id, "subagent result receiver closed");
                                            }
                                            return;
                                        }
                                    }
                                }
                            }) };

                            let task_id = request.task_id;
                            let result = run_subagent_task_with_cancel(
                                request,
                                approval_mode,
                                task_store,
                                agents,
                                provider_registry,
                                config,
                                session_store,
                                cancel_token,
                                lineage_token,
                                scheduler_cancel,
                                db_pool,
                                subagent_pool,
                            ).await;

                            if response_tx.send(result).is_err() {
                                tracing::debug!(task_id, "subagent result receiver closed");
                            }
                            drop(permit);
                        });

                        // Push handle immediately after spawn to avoid race with shutdown
                        active_handles.lock().await.push(handle);
                    }
                    else => break,
                }
            }
        });

        let workers = workers.clone();
        tokio::spawn(async move {
            workers.lock().await.push(handle);
        });
    }

    pub fn spawner(&self) -> SubAgentSpawner {
        SubAgentSpawner { pool: self.clone() }
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    pub fn active_count(&self) -> usize {
        self.admission.active_count()
    }

    /// Resolve the isolation policy from effective capability inputs before
    /// the child loop is constructed. Unknown/custom agents are conservative:
    /// an undeclared mutating tool is still potentially mutating and therefore
    /// requires a managed worktree.
    ///
    /// Every tool in this list must be one an agent can actually mutate
    /// through. A read-only agent has to deny *all* of them or it is
    /// classified as mutating: `git` was the gap that made `explore`, `plan`,
    /// and `research` demand isolation even though they deny every other
    /// mutating tool, and `lsp_preview_apply` mutates the buffer but was not
    /// listed at all. Keep this list and the `[agent.permissions]` blocks in
    /// `assets/agents/` in agreement.
    pub fn requires_isolation(&self, agent_name: &str, denied_tools: &[String]) -> bool {
        let Some(agent) = self.agents.iter().find(|agent| agent.name == agent_name) else {
            return true;
        };
        [
            "write",
            "edit",
            "apply_patch",
            "replace",
            "terminal",
            "bash",
            "git",
            "commit",
            "lsp_preview_apply",
        ]
        .into_iter()
        .any(|tool| {
            // `git_read_only` is inspection, not mutation: an agent that may
            // only run read-only git commands has not been granted the ability
            // to change the repository, so it must not be pushed back into
            // isolation for merely reading one. Without this the declaration
            // would reintroduce the very bug that made `explore` demand a
            // worktree — the same failure mode `git = "deny"` was added to fix.
            if tool == "git" && agent.git_read_only {
                return false;
            }
            !denied_tools.iter().any(|denied| denied == tool)
                && !agent
                    .permissions
                    .get(tool)
                    .is_some_and(|level| level.eq_ignore_ascii_case("deny"))
        })
    }

    pub fn task_store(&self) -> Arc<TokioMutex<TaskStore>> {
        self.task_store.clone()
    }

    /// Install the daemon-owned delegation boundary after scheduler
    /// construction. Standalone pools leave this unset and retain their
    /// explicit compatibility behavior.
    pub fn configure_durable_delegation(
        &self,
        submission: Arc<crate::scheduler::JobSubmissionService>,
        agent_runs: Arc<dyn codegg_core::agent_run::AgentRunStore>,
        run_control: Arc<crate::agent::run_control::RunControlService>,
        run_groups: Arc<codegg_core::agent_run_group::AgentRunGroupService>,
    ) {
        if let Ok(mut slot) = self.durable_submission.try_lock() {
            *slot = Some(submission);
        }
        if let Ok(mut slot) = self.durable_agent_runs.try_lock() {
            *slot = Some(agent_runs);
        }
        if let Ok(mut slot) = self.durable_run_control.try_lock() {
            *slot = Some(run_control);
        }
        if let Ok(mut slot) = self.durable_run_groups.try_lock() {
            *slot = Some(run_groups);
        }
    }

    /// Cancel one root lineage without affecting unrelated roots.
    pub async fn cancel_lineage(&self, root_id: &str) -> bool {
        self.lineage_tokens
            .lock()
            .await
            .get(root_id)
            .map(|token| token.cancel())
            .is_some()
    }

    pub async fn shutdown(&self) {
        tracing::info!("SubAgentPool initiating shutdown");
        self.cancel_token.cancel();

        // Wait briefly for cooperative cancellation to finish
        let mut attempts = 0;
        while self.admission.active_count() > 0 && attempts < 10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            attempts += 1;
        }

        // Abort only as a fallback if tasks haven't completed
        let mut active_handles = self.active_handles.lock().await;
        let remaining_count = active_handles.len();
        if remaining_count > 0 {
            tracing::warn!(
                "Aborting {} remaining active handles after waiting",
                remaining_count
            );
            for handle in active_handles.drain(..) {
                handle.abort();
            }
        }
        drop(active_handles);

        // Wait for worker loop to finish
        let workers = std::mem::take(&mut *self.workers.lock().await);
        for handle in workers {
            if let Err(error) = handle.await {
                tracing::warn!(error = %error, "subagent worker loop exited unexpectedly");
            }
        }

        // Wait for aborted tasks to complete (active_count to reach 0)
        let mut attempts = 0;
        while self.admission.active_count() > 0 && attempts < 10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            attempts += 1;
        }

        let final_count = self.admission.active_count();
        tracing::info!(
            "SubAgentPool shutdown complete, final active count: {}",
            final_count
        );
    }
}

impl Clone for SubAgentPool {
    fn clone(&self) -> Self {
        Self {
            max_concurrent: self.max_concurrent,
            max_depth: self.max_depth,
            task_store: Arc::clone(&self.task_store),
            workers: Arc::clone(&self.workers),
            request_tx: self.request_tx.clone(),
            agents: Arc::clone(&self.agents),
            provider_registry: Arc::clone(&self.provider_registry),
            config: Arc::clone(&self.config),
            session_store: Arc::clone(&self.session_store),
            cancel_token: self.cancel_token.clone(),
            active_handles: Arc::clone(&self.active_handles),
            pool: self.pool.clone(),
            max_direct_children: self.max_direct_children,
            max_active_descendants: self.max_active_descendants,
            max_total_child_tool_calls: self.max_total_child_tool_calls,
            admission: Arc::clone(&self.admission),
            lineage_tokens: Arc::clone(&self.lineage_tokens),
            durable_submission: Arc::clone(&self.durable_submission),
            durable_agent_runs: Arc::clone(&self.durable_agent_runs),
            durable_run_control: Arc::clone(&self.durable_run_control),
            durable_run_groups: Arc::clone(&self.durable_run_groups),
        }
    }
}

#[derive(Clone)]
pub struct SubAgentSpawner {
    pool: SubAgentPool,
}

impl SubAgentSpawner {
    async fn handle_response(
        task_id: u64,
        result: Result<SubAgentResult, tokio::sync::oneshot::error::RecvError>,
        task_store: Arc<TokioMutex<TaskStore>>,
    ) {
        match result {
            Ok(result) => {
                let display_result = if let Some(ref report) = result.report {
                    report.to_compact_text()
                } else {
                    result.result.clone()
                };
                if result.success {
                    task_store
                        .lock()
                        .await
                        .set_result(task_id, display_result)
                        .await;
                } else if result.result == "Task cancelled" {
                    // Cancelled during shutdown - set Interrupted status
                    task_store
                        .lock()
                        .await
                        .set_interrupted(task_id, result.result.clone())
                        .await;
                } else if result.result == "pool shutting down" {
                    // Pool shutting down before task started
                    task_store
                        .lock()
                        .await
                        .set_interrupted(task_id, result.result.clone())
                        .await;
                } else {
                    task_store
                        .lock()
                        .await
                        .set_failed(task_id, result.result.clone())
                        .await;
                }
            }
            Err(e) => {
                task_store
                    .lock()
                    .await
                    .set_interrupted(
                        task_id,
                        format!("Task cancelled (worker interrupted: {})", e),
                    )
                    .await;
            }
        }
    }

    async fn enqueue_request(
        &self,
        request: SubAgentRequest,
    ) -> Result<oneshot::Receiver<SubAgentResult>, String> {
        self.enqueue_request_with_mode(
            request,
            CancellationToken::new(),
            false,
            codegg_core::approval::ApprovalMode::Interactive,
        )
        .await
    }

    async fn enqueue_request_with_mode(
        &self,
        request: SubAgentRequest,
        scheduler_cancel: CancellationToken,
        scheduled: bool,
        approval_mode: codegg_core::approval::ApprovalMode,
    ) -> Result<oneshot::Receiver<SubAgentResult>, String> {
        if self.pool.config.subagent.as_ref().and_then(|s| s.enabled) == Some(false) {
            return Err("subagent delegation is disabled".to_string());
        }
        if request.depth >= self.pool.max_depth {
            return Err(format!(
                "subagent max depth {} exceeded (request depth: {})",
                self.pool.max_depth, request.depth
            ));
        }

        let cfg = self.pool.config.subagent.as_ref();
        if let Some(allowed) = cfg.and_then(|s| s.allowed_agents.as_ref()) {
            if !allowed.is_empty() && !allowed.iter().any(|name| name == &request.agent) {
                return Err(format!(
                    "subagent target '{}' is not allowed",
                    request.agent
                ));
            }
        }
        if cfg
            .and_then(|s| s.denied_agents.as_ref())
            .is_some_and(|denied| denied.iter().any(|name| name == &request.agent))
        {
            return Err(format!("subagent target '{}' is denied", request.agent));
        }

        let reserved_tool_calls = cfg
            .and_then(|settings| settings.max_total_child_tool_calls)
            .map(|budget| request.max_tool_calls.unwrap_or(budget))
            .unwrap_or(0);
        let lease = self.pool.admission.admit(&request, reserved_tool_calls)?;
        let root = request
            .parent_id
            .clone()
            .unwrap_or_else(|| format!("task-root-{}", request.task_id));
        let lineage_token = {
            let mut tokens = self.pool.lineage_tokens.lock().await;
            tokens
                .entry(root)
                .or_insert_with(CancellationToken::new)
                .clone()
        };

        let (response_tx, response_rx) = oneshot::channel();
        let worker_request = WorkerRequest {
            request,
            approval_mode,
            response_tx,
            lease: Some(lease),
            lineage_token,
            scheduler_cancel,
            scheduled,
        };

        if let Err(error) = self.pool.request_tx.send(worker_request).await {
            return Err(format!("failed to queue request: {}", error));
        }

        Ok(response_rx)
    }

    pub async fn send(&self, request: SubAgentRequest) -> Result<(), String> {
        let task_id = request.task_id;
        let response_rx = self.enqueue_request(request).await?;
        let task_store = Arc::clone(&self.pool.task_store);

        tokio::spawn(async move {
            Self::handle_response(task_id, response_rx.await, task_store).await;
        });

        Ok(())
    }

    pub async fn send_async(&self, request: SubAgentRequest) -> Result<(), String> {
        self.send(request).await
    }

    /// Enqueue a request and wait for the worker result. Scheduler-owned
    /// callers use this form so their durable attempt and admission permit
    /// remain active until the subagent actually finishes.
    pub async fn send_and_wait(&self, request: SubAgentRequest) -> Result<SubAgentResult, String> {
        self.send_and_wait_with_mode(
            request,
            CancellationToken::new(),
            false,
            codegg_core::approval::ApprovalMode::Interactive,
        )
        .await
    }

    /// Scheduler-owned execution path. Semantic descendant limits remain
    /// enforced by the pool, while machine-capacity admission is owned by
    /// the scheduler and therefore does not acquire the pool semaphore.
    pub async fn send_and_wait_scheduled(
        &self,
        request: SubAgentRequest,
        scheduler_cancel: CancellationToken,
    ) -> Result<SubAgentResult, String> {
        self.send_and_wait_scheduled_with_approval_mode(
            request,
            scheduler_cancel,
            codegg_core::approval::ApprovalMode::Interactive,
        )
        .await
    }

    /// Scheduler-owned execution with the approval mode captured from the
    /// authorized parent tool invocation. Sandbox and agent permission rules
    /// remain independently bounded in the child loop.
    pub async fn send_and_wait_scheduled_with_approval_mode(
        &self,
        request: SubAgentRequest,
        scheduler_cancel: CancellationToken,
        approval_mode: codegg_core::approval::ApprovalMode,
    ) -> Result<SubAgentResult, String> {
        self.send_and_wait_with_mode(request, scheduler_cancel, true, approval_mode)
            .await
    }

    async fn send_and_wait_with_mode(
        &self,
        request: SubAgentRequest,
        scheduler_cancel: CancellationToken,
        scheduled: bool,
        approval_mode: codegg_core::approval::ApprovalMode,
    ) -> Result<SubAgentResult, String> {
        let task_id = request.task_id;
        let response = self
            .enqueue_request_with_mode(request, scheduler_cancel, scheduled, approval_mode)
            .await?
            .await;
        let result = response.map_err(|e| format!("worker response error: {e}"))?;
        Self::handle_response(
            task_id,
            Ok(result.clone()),
            Arc::clone(&self.pool.task_store),
        )
        .await;
        Ok(result)
    }
}

#[expect(clippy::too_many_arguments)]
async fn run_subagent_task_with_cancel(
    request: SubAgentRequest,
    approval_mode: codegg_core::approval::ApprovalMode,
    task_store: Arc<TokioMutex<TaskStore>>,
    agents: Arc<Vec<Agent>>,
    provider_registry: Arc<ProviderRegistry>,
    config: Arc<Config>,
    session_store: Arc<SessionStore>,
    cancel_token: CancellationToken,
    lineage_token: CancellationToken,
    scheduler_cancel: CancellationToken,
    pool: Option<SqlitePool>,
    subagent_pool: Arc<SubAgentPool>,
) -> SubAgentResult {
    let task_id = request.task_id;
    let session_id = request.parent_id.clone().unwrap_or_default();

    GlobalEventBus::publish(AppEvent::SubagentStarted {
        session_id: session_id.clone(),
        task_id,
        agent: request.agent.clone(),
        description: request.description.clone(),
    });

    task_store
        .lock()
        .await
        .update_status(task_id, crate::tool::task::TaskStatus::Running)
        .await;

    GlobalEventBus::publish(AppEvent::SubagentProgress {
        session_id: session_id.clone(),
        task_id,
        agent: request.agent.clone(),
        message: "Task execution started".to_string(),
    });

    let result = tokio::select! {
        biased;
        _ = scheduler_cancel.cancelled() => {
            GlobalEventBus::publish(AppEvent::SubagentFailed {
                session_id: session_id.clone(), task_id, agent: request.agent.clone(),
                error: "Task cancelled by scheduler".to_string(),
            });
            SubAgentResult::failure(task_id, "Task cancelled".to_string())
        }
        _ = cancel_token.cancelled() => {
            let msg = "Task cancelled during shutdown".to_string();
            GlobalEventBus::publish(AppEvent::SubagentFailed {
                session_id: session_id.clone(),
                task_id,
                agent: request.agent.clone(),
                error: msg.clone(),
            });
            // Don't update task store here - let handle_response do it
            SubAgentResult::failure(task_id, "Task cancelled".to_string())
        }
        _ = lineage_token.cancelled() => {
            GlobalEventBus::publish(AppEvent::SubagentFailed {
                session_id: session_id.clone(), task_id, agent: request.agent.clone(),
                error: "Task cancelled by root lineage".to_string(),
            });
            SubAgentResult::failure(task_id, "Task cancelled".to_string())
        }
        result = async {
            let execution = execute_agent_task(
                &request,
                approval_mode,
                agents,
                provider_registry,
                Arc::clone(&config),
                session_store,
                pool,
                subagent_pool,
            );
            if let Some(seconds) = config
                .subagent
                .as_ref()
                .and_then(|settings| settings.wall_clock_timeout_secs)
            {
                match tokio::time::timeout(tokio::time::Duration::from_secs(seconds), execution).await {
                    Ok(result) => result,
                    Err(_) => Err("subagent wall-clock timeout exceeded".into()),
                }
            } else {
                execution.await
            }
        } => {
            match result {
                Ok((output, report, validation, artifacts)) => {
                    GlobalEventBus::publish(AppEvent::SubagentCompleted {
                        session_id: session_id.clone(),
                        task_id,
                        agent: request.agent.clone(),
                        result_summary: output.chars().take(200).collect(),
                    });
                    // Don't update task store here - let handle_response do it
                    let mut result = if let Some(report) = report {
                        SubAgentResult::success_with_report(task_id, output, report)
                    } else {
                        SubAgentResult::success(task_id, output)
                    };
                    result.validation = validation;
                    result.artifacts = artifacts;
                    result
                }
                Err(ref e) => {
                    let error_msg = format!("Subagent task failed: {}", e);
                    let agent_name = request.agent.clone();
                    let error_for_bus = error_msg.clone();
                    let session_id_for_bus = session_id.clone();
                    let _ = e;
                    GlobalEventBus::publish(AppEvent::SubagentFailed {
                        session_id: session_id_for_bus,
                        task_id,
                        agent: agent_name,
                        error: error_for_bus,
                    });
                    // Don't update task store here - let handle_response do it
                    SubAgentResult::failure(task_id, error_msg)
                }
            }
        }
    };

    result
}

async fn execute_agent_task(
    request: &SubAgentRequest,
    approval_mode: codegg_core::approval::ApprovalMode,
    agents: Arc<Vec<Agent>>,
    provider_registry: Arc<ProviderRegistry>,
    config: Arc<Config>,
    _session_store: Arc<SessionStore>,
    pool: Option<SqlitePool>,
    subagent_pool: Arc<SubAgentPool>,
) -> Result<
    (
        String,
        Option<SubAgentReport>,
        Vec<codegg_core::run_result::ValidationEvidence>,
        Vec<codegg_core::run_result::AgentRunArtifact>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let agent_name = &request.agent;
    let agent = agents
        .iter()
        .find(|a| a.name == *agent_name)
        .ok_or_else(|| format!("Agent '{}' not found", agent_name))?;

    // Phase 3: Apply safety envelope to prevent custom files from escalating
    // permissions beyond session/config/hard policy bounds.
    let session_rules = crate::permission::PermissionRuleset::default();
    let config_rules = crate::permission::config_ruleset(Some(&config));
    let mut hard_deny = vec!["todowrite".to_string(), "todoread".to_string()];
    let safe_without_commit =
        agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
    // A child may only commit after the scheduler has given it an owned
    // worktree. Pushes, history rewrites, and destructive cleanup remain
    // independently denied by the child tool surface and Git policy.
    if request.workspace_root.is_none() || !has_explicit_git_write(&safe_without_commit) {
        hard_deny.push("commit".to_string());
    }
    let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);

    // Build a fully resolved execution profile with model inheritance and alias resolution.
    let profile = crate::agent::ResolvedAgentExecutionProfile::resolve(
        &safe_agent,
        &config,
        request.parent_model.as_deref(),
    );

    tracing::debug!(
        "Subagent '{}': runtime_kind={}, resolved_model='{}'",
        agent_name,
        profile.runtime_kind,
        profile.resolved_model,
    );

    // Provider identity is resolved exactly as the parent turn resolves it
    // (`turn_runtime.rs`): one helper owns the runtime `<provider>/<model>`
    // grammar, and the provider half is a registry id while the model half is
    // the bare id the provider's wire profile actually knows.
    //
    // Two separate mistakes lived here. Splitting the string by hand made a
    // bare model id (`gpt-4o`) look like a provider name, and the `openai`
    // fallback below reinstated exactly the default `runtime_model_provider_
    // segment` exists to forbid — "an empty segment is simply unknown".
    // Passing the *prefixed* string on as the request model was the other:
    // `opencode_go` matches its wire mapping by exact model id, so a child
    // asking for `opencode_go/longcat-2.5-preview-free` failed closed with
    // "has no reviewed wire mapping" while the parent, sending the bare id
    // through the very same connection, succeeded.
    let provider_name =
        crate::core::session_selection::runtime_model_provider_segment(&profile.resolved_model)
            .to_string();
    let model_name = match profile.resolved_model.split_once('/') {
        Some((_, model_id)) => model_id.to_string(),
        None => profile.resolved_model.clone(),
    };
    if provider_name.is_empty() {
        return Err(format!(
            "provider '{}' is not a known runtime provider id",
            profile.resolved_model
        )
        .into());
    }

    // A delegated run is the parent turn's own continuation, so it authenticates
    // with the parent session's durable connection rather than the ambient
    // registry. The ambient registry is built once with no connection authority,
    // which is why a provider holding several stored accounts resolves as
    // permanently ambiguous there and no subagent could ever run on one.
    let credential_locator = match pool.as_ref() {
        Some(pool) => {
            crate::core::session_selection::session_connection_credential_locator(
                pool,
                request.parent_id.as_deref().unwrap_or_default(),
            )
            .await
        }
        None => None,
    };
    let mut turn_registry = crate::provider::ProviderRegistry::new();
    let turn_registration = crate::provider::register_builtin_with_config_for_turn(
        &mut turn_registry,
        &config,
        &provider_name,
        credential_locator.as_ref(),
        None,
    );
    if let Err(error) = turn_registration {
        tracing::warn!(
            target: "codegg::agent::worker",
            provider = %provider_name,
            code = error.code(),
            "delegated run credential resolution failed"
        );
    }
    // Prefer the connection-scoped registry; keep the ambient registry as a
    // fallback so a provider this path does not build (a plugin-registered one,
    // say) still behaves exactly as it did before.
    let provider = turn_registry
        .get(&provider_name)
        .or_else(|| provider_registry.get(&provider_name))
        .ok_or_else(|| format!("Provider '{}' not found", provider_name))?
        .clone_box();

    // Bootstraps the search backend (eggsearch by default) before the
    // agent loop starts, returning an explicit runtime context (M005).
    // The bootstrap is idempotent: if the parent process has already
    // populated the shared daemon `McpService`, this reuses the
    // existing service without spawning a new eggsearch connection. If
    // the backend is `disabled`, the context carries no service and
    // the loop runs without MCP tools. Wrappers below receive this
    // context explicitly and never consult process-global slots.
    let (search_runtime, _report) =
        crate::search_backend::bootstrap::bootstrap_search_runtime(&config).await;

    // M005: child sandbox ceiling is the intersection/no-broader-than
    // parent plus child worktree ownership. `None` parent means the
    // pre-M005 default (WorkspaceWrite, never FullHost). A requested
    // FullHost under a constrained parent fails closed here, before any
    // child loop or tool registry exists. The child's writable sandbox
    // root is its own workspace_root: for isolated worktree children that
    // is the leased worktree, for shared read-only children it is the
    // inherited parent root.
    let parent_profile = request
        .parent_sandbox_profile
        .unwrap_or(codegg_core::approval::SandboxProfile::WorkspaceWrite);
    let requested_child = request.sandbox_profile.unwrap_or(parent_profile);
    let mut effective_child =
        codegg_core::approval::resolve_child_sandbox(parent_profile, requested_child)
            .map_err(|e| format!("child sandbox exceeds parent ceiling: {e}"))?;
    // Read-only child agents are narrowed to ReadOnly (always ≤ parent
    // except when parent is already ReadOnly, where it is equal).
    if is_read_only_agent(agent) {
        effective_child = codegg_core::approval::resolve_child_sandbox(
            parent_profile,
            codegg_core::approval::SandboxProfile::ReadOnly,
        )
        .map_err(|e| format!("read-only child sandbox narrowing failed: {e}"))?;
    }

    let mut tool_registry = ToolRegistry::with_options(crate::tool::ToolRegistryOptions {
        workspace_root: request.workspace_root.clone(),
        tool_advisor: config.tool_advisor.clone(),
        decision_engine: config.decision_engine.clone(),
        command_intent: config.command_intent.clone(),
        child_git_policy: request
            .workspace_root
            .as_ref()
            .map(|_| crate::tool::git::ChildGitPolicy::LocalCommitOnly),
        // Read-only git is a grant to *inspect*, and only to the agent that
        // asked for it. The tool re-checks per invocation, because `git` is one
        // tool covering both `branch` and `branch -d`.
        git_read_only: safe_agent.git_read_only.then(|| agent.name.clone()),
        tool_backends: crate::tool::ToolBackendConfig::from_config(&config),
        lsp_cache_config: crate::tool::convert_lsp_cache_config(&config.lsp_semantic_cache),
        search_runtime: Some(search_runtime.clone()),
        sandbox_profile: Some(effective_child),
        ..Default::default()
    });
    // Subagents must NEVER have access to in-flight planning tools
    // (todowrite/todoread), long-horizon goal tools (goal_get,
    // goal_update_progress, goal_request_completion — not in
    // `with_defaults()` so already absent), or plan-mode control
    // (plan_enter, plan_exit). The parent's planning is the source of
    // truth; the subagent does the work.
    let subagent_blocked_tools: Vec<String> = vec![
        "todowrite".to_string(),
        "todoread".to_string(),
        "plan_enter".to_string(),
        "plan_exit".to_string(),
    ];
    tool_registry.filter_out(&subagent_blocked_tools);
    let parent_is_read_only = is_read_only_agent(agent);
    if parent_is_read_only {
        tool_registry.filter_out(&read_only_blocked_tools());
    }
    if !request.denied_tools.is_empty() {
        tool_registry.filter_out(&request.denied_tools);
    }

    // Nested delegation is opt-in at the resolved agent definition. The
    // shared pool is installed only when the target has an explicit `task`
    // permission and the request did not inherit a task denial.
    let can_delegate = agent
        .permissions
        .get("task")
        .is_some_and(|level| level.eq_ignore_ascii_case("allow"))
        && !request.denied_tools.iter().any(|tool| tool == "task")
        && config.subagent.as_ref().and_then(|s| s.enabled) != Some(false);
    if can_delegate {
        // Acquire the four option slots concurrently rather than in
        // sequence; each is uncontended and the clones are cheap Arcs.
        let (durable_submission, durable_agent_runs, durable_run_control, durable_run_groups) = tokio::join!(
            async { subagent_pool.durable_submission.lock().await.clone() },
            async { subagent_pool.durable_agent_runs.lock().await.clone() },
            async { subagent_pool.durable_run_control.lock().await.clone() },
            async { subagent_pool.durable_run_groups.lock().await.clone() },
        );
        let mut inherited_denied = request.denied_tools.clone();
        if parent_is_read_only {
            inherited_denied.extend(read_only_blocked_tools());
            inherited_denied.sort();
            inherited_denied.dedup();
        }
        let durable_context = match (&durable_agent_runs, &request.run_id) {
            (Some(store), Some(run_id)) => {
                let run = store
                    .get_run(run_id)
                    .await
                    .map_err(|error| format!("durable subagent run lookup failed: {error}"))?
                    .ok_or_else(|| "durable subagent run is missing".to_string())?;
                let task = store
                    .get_task(&run.task_id)
                    .await
                    .map_err(|error| format!("durable subagent task lookup failed: {error}"))?
                    .ok_or_else(|| "durable subagent task is missing".to_string())?;
                Some((run, task))
            }
            (None, Some(_)) => {
                return Err("durable subagent run has no run store".into());
            }
            _ => None,
        };
        let mut task_tool = crate::tool::task::TaskTool::new(
            subagent_pool.task_store(),
            Some(subagent_pool.spawner()),
            Some(request.parent_id.clone().unwrap_or_default()),
            inherited_denied,
        )
        .with_depth(request.depth)
        .with_parent_model(Some(profile.resolved_model.clone()))
        .with_workspace_root(request.workspace_root.clone())
        .with_workspace_locks(request.workspace_locks.clone())
        .with_parent_allowed_paths(request.allowed_paths.clone());
        if let Some(run_id) = request.run_id.clone() {
            task_tool = task_tool.with_run_owner(run_id);
        } else if let Some(turn_id) = durable_context
            .as_ref()
            .and_then(|(_, task)| task.originating_turn_id.clone())
        {
            task_tool = task_tool.with_turn_owner(
                durable_context
                    .as_ref()
                    .map(|(_, task)| task.originating_session_id.clone())
                    .unwrap_or_default(),
                turn_id,
            );
        }
        if let Some((run, task)) = durable_context {
            task_tool = task_tool
                .with_project_context(
                    Some(task.project_id),
                    task.repository_id,
                    task.originating_turn_id,
                )
                .with_workspace_id(Some(task.workspace_id))
                .with_max_depth(
                    run.budget
                        .max_depth
                        .unwrap_or(subagent_pool.max_depth as u32),
                );
        } else {
            task_tool = task_tool.with_max_depth(subagent_pool.max_depth as u32);
        }
        let task_tool = match (
            durable_submission,
            durable_agent_runs,
            request.workspace_root.clone(),
        ) {
            (Some(submission), Some(agent_runs), Some(root)) => task_tool
                .with_submission(submission, root)
                .with_agent_run_store(agent_runs),
            _ => task_tool,
        };
        let task_tool = match durable_run_control {
            Some(control) => task_tool.with_run_control_opt(Some(control)),
            _ => task_tool,
        };
        let task_tool = match durable_run_groups {
            Some(groups) => task_tool.with_run_group_service(Some(groups)),
            None => task_tool,
        };
        let task_tool =
            task_tool.with_workstream_claim_store(subagent_pool.pool.as_ref().map(|pool| {
                codegg_core::workstream_claim::WorkstreamClaimStore::new(pool.clone())
            }));
        tool_registry.register(task_tool);
    }

    let mut agent_rules = crate::permission::PermissionRuleset::default();
    if !request.allowed_paths.is_empty() {
        for path in &request.allowed_paths {
            // Allow the path itself and everything under it
            agent_rules.path_rules.push(crate::permission::PathRule {
                pattern: path.clone(),
                level: crate::permission::PermissionLevel::Allow,
            });
            if !path.ends_with('/') {
                agent_rules.path_rules.push(crate::permission::PathRule {
                    pattern: format!("{}/{}", path, "**"),
                    level: crate::permission::PermissionLevel::Allow,
                });
            } else {
                agent_rules.path_rules.push(crate::permission::PathRule {
                    pattern: format!("{}{}", path, "**"),
                    level: crate::permission::PermissionLevel::Allow,
                });
            }
        }
        // Deny everything else if specific paths are allowed
        agent_rules.path_rules.push(crate::permission::PathRule {
            pattern: "**".to_string(),
            level: crate::permission::PermissionLevel::Deny,
        });
    }

    let denied: std::collections::BTreeSet<String> = request.denied_tools.iter().cloned().collect();
    let disabled = std::collections::BTreeSet::new();
    // Resolved against the provider *and* the bare model id, exactly as the
    // parent turn does. Passing the prefixed string here made every adapter
    // lookup miss the profile's per-model entries.
    let adapter = codegg_core::model_profile::resolve_adapter(Some(&provider_name), &model_name);
    let wire_to_canonical: std::collections::BTreeMap<_, _> = adapter
        .tool_aliases
        .iter()
        .map(|(canonical, wire)| (wire.clone(), canonical.clone()))
        .collect();
    let surface = crate::agent::tool_surface::ResolvedToolSurface::from_registry_with_aliases(
        &tool_registry,
        &denied,
        &disabled,
        agent_name == "plan",
        None,
        &wire_to_canonical,
    )
    .map_err(|error| format!("invalid subagent tool surface: {error:?}"))?;
    let mut available_tools: Vec<String> = surface
        .tools
        .iter()
        .map(|tool| tool.canonical_name.clone())
        .collect();
    available_tools.sort();

    // M003: subagents receive an ephemeral (non-durable) decision store.
    // They must not get an independent global store; the parent ceiling
    // remains authoritative and subagent `Always` choices stay in-memory.
    // Subagent loops also default to `Interactive` so they never exceed a
    // parent Yolo/Automatic ceiling without explicit narrowing.
    let permission_checker =
        PermissionChecker::new(Some(&config), None).with_agent_rules(agent_rules);

    // The subagent loop receives the MCP handle from the explicit
    // runtime context bootstrapped above (no second bootstrap, no
    // global lookup at execution time).
    let mcp_service = search_runtime.mcp();

    let subagent_session_id = request
        .parent_id
        .as_ref()
        .map(|parent_id| format!("{}-sub-{}", parent_id, request.task_id))
        .unwrap_or_else(|| format!("subagent-{}", request.task_id));
    let subagent_session_id_for_context = subagent_session_id.clone();
    let workspace_root = request
        .workspace_root
        .clone()
        .ok_or_else(|| "subagent execution requires an explicit workspace root".to_string())?;
    let mut agent_loop = AgentLoop::new(
        agents.iter().cloned().collect(),
        provider,
        permission_checker,
        tool_registry,
        (*config).clone(),
        mcp_service,
        pool,
        std::sync::Arc::new(crate::context::InMemoryArtifactStore::new()),
        workspace_root,
        subagent_session_id,
    );
    if let Some(locks) = request.workspace_locks.clone() {
        agent_loop.set_workspace_locks(locks);
    }
    // M005: child loop carries the narrowed sandbox ceiling. Approval mode
    // was captured from the authorized parent invocation and is independent
    // of the sandbox; it never broadens the child agent's tool/path rules.
    agent_loop.set_sandbox_profile(effective_child);
    agent_loop.set_approval_mode(approval_mode);
    agent_loop.set_subagent_pool(Arc::clone(&subagent_pool));

    // The durable control service feeds these existing loop channels.  The
    // registration happens only after the loop owns the receivers, so a
    // persisted control can never race provider transcript mutation.
    let live_registration = if let Some(run_id) = request.run_id.clone() {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel(32);
        agent_loop.set_cancel_receiver(cancel_rx);
        agent_loop.set_steer_receiver(steer_rx);
        let control = subagent_pool.durable_run_control.lock().await.clone();
        if let Some(control) = control {
            agent_loop.set_run_control(control.clone(), run_id.clone());
            if let Some(runs) = subagent_pool.durable_agent_runs.lock().await.clone() {
                if let Ok(Some(run)) = runs.get_run(&run_id).await {
                    if let Ok(Some(task)) = runs.get_task(&run.task_id).await {
                        agent_loop.set_peer_observation_session_id(task.originating_session_id);
                    }
                }
            }
            if let Some(groups) = subagent_pool.durable_run_groups.lock().await.clone() {
                if let Ok(groups) = groups.groups_for_member(&run_id).await {
                    agent_loop.set_peer_group_memberships(
                        groups
                            .into_iter()
                            .take(16)
                            .map(|group| {
                                (
                                    group.group_id.to_string(),
                                    group
                                        .member_run_ids
                                        .into_iter()
                                        .map(|member| member.to_string())
                                        .collect(),
                                )
                            })
                            .collect(),
                    );
                }
            }
            control
                .register_live(
                    run_id.clone(),
                    crate::agent::run_control::LiveRunHandle {
                        follow_up_tx: agent_loop.follow_up_sender(),
                        steer_tx,
                        cancel_tx,
                        interrupt_flag: agent_loop.interrupt_flag(),
                    },
                )
                .await;
            Some((control, run_id))
        } else {
            None
        }
    } else {
        None
    };

    if agent_name == "plan" {
        agent_loop.enter_plan_mode(Some(request.description.clone()));
    }

    agent_loop.set_agent(agent_name)?;
    let configured_tool_limit = config
        .subagent
        .as_ref()
        .and_then(|settings| settings.max_total_child_tool_calls);
    agent_loop.set_max_tool_calls(match (request.max_tool_calls, configured_tool_limit) {
        (Some(requested), Some(configured)) => Some(requested.min(configured)),
        (Some(requested), None) => Some(requested),
        (None, Some(configured)) => Some(configured),
        (None, None) => None,
    });

    // Defense in depth: even if a todo/goal tool somehow gets registered,
    // the task state policy rejects writes. Subagents do not manage
    // in-flight todos or long-horizon goals — that's the parent's job.
    use crate::model_profile::types::{TaskStatePolicy, TodoMode};
    agent_loop.set_task_state_policy(TaskStatePolicy {
        mode: TodoMode::Disabled,
        allow_model_todo_read: false,
        allow_model_todo_write: false,
        ..Default::default()
    });

    let model_profile =
        crate::model_profile::ModelProfileResolver::new(&config).resolve(&model_name);
    let available_agents = agents.as_ref().clone();
    let compiled_prompt =
        crate::agent::prompt::PromptCompiler::compile(crate::agent::prompt::PromptCompilerInput {
            agent: &safe_agent,
            model_profile: &model_profile,
            config: &config,
            tools: &available_tools,
            skills: &[],
            agents: &available_agents,
            is_plan_mode: agent_name == "plan",
            snapshot: None,
            pin: None,
            execution: None,
            adapter_fingerprint: Some(&adapter.fingerprint),
            runtime_blocks: &[],
        });
    let mut messages = vec![crate::provider::Message::System {
        content: compiled_prompt.text.into(),
    }];

    messages.push(crate::provider::Message::User {
        content: vec![crate::provider::ContentPart::Text {
            text: request.prompt.clone().into(),
        }],
    });

    // The bare model id, never the `<provider>/<model>` string. This is the value
    // the provider's own wire profile matches against
    // (`opencode_go::resolve` → `resolve_route`), so sending the prefixed form
    // here is what made every delegated run on an adapted provider die with
    // "has no reviewed wire mapping".
    let model = model_name.clone();
    let request = crate::provider::ChatRequest {
        messages,
        model,
        tools: None,
        system: None,
        temperature: safe_agent.temperature,
        top_p: safe_agent.top_p,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: crate::provider::ProviderRequestContext {
            session_id: Some(std::sync::Arc::from(
                subagent_session_id_for_context.as_str(),
            )),
            ..Default::default()
        },
    };

    let events = agent_loop.run(request).await;
    if let Some((control, run_id)) = live_registration {
        control.unregister_live(&run_id).await;
    }
    let events = events?;

    let mut output = String::new();
    for event in &events {
        if let crate::provider::ChatEvent::TextDelta(text) = event {
            output.push_str(text);
        }
    }

    if output.is_empty() {
        output = format!(
            "Subagent '{}' completed with {} events (no text output)",
            agent_name,
            events.len()
        );
    }

    let report = serde_json::from_str::<SubAgentReport>(&output).ok();
    let (validation, artifacts) = extract_validation_evidence(&events);

    Ok((output, report, validation, artifacts))
}

const VALIDATION_RESULT_PREFIX: &str = "CODEGG_VALIDATION_RESULT:";

#[derive(Debug, Deserialize)]
struct ValidationResultMarker {
    job_id: String,
    kind: String,
    status: String,
    summary: String,
}

fn extract_validation_evidence(
    events: &[crate::provider::ChatEvent],
) -> (
    Vec<codegg_core::run_result::ValidationEvidence>,
    Vec<codegg_core::run_result::AgentRunArtifact>,
) {
    let mut validation = Vec::new();
    let mut artifacts = Vec::new();
    for event in events {
        let crate::provider::ChatEvent::ToolResult { content, .. } = event else {
            continue;
        };
        for line in content.lines() {
            let Some(payload) = line.strip_prefix(VALIDATION_RESULT_PREFIX) else {
                continue;
            };
            let Ok(marker) = serde_json::from_str::<ValidationResultMarker>(payload) else {
                continue;
            };
            let status = match marker.status.as_str() {
                "passed" => codegg_core::run_result::ValidationStatus::Passed,
                "failed" => codegg_core::run_result::ValidationStatus::Failed,
                "cancelled" => codegg_core::run_result::ValidationStatus::Cancelled,
                "timed_out" => codegg_core::run_result::ValidationStatus::TimedOut,
                _ => codegg_core::run_result::ValidationStatus::Unknown,
            };
            validation.push(codegg_core::run_result::ValidationEvidence {
                kind: marker.kind,
                status,
                summary: marker.summary,
            });
            artifacts.push(codegg_core::run_result::AgentRunArtifact {
                kind: "validation_job".into(),
                label: "scheduler validation job".into(),
                reference: Some(format!("job:{}", marker.job_id)),
            });
        }
    }
    (validation, artifacts)
}

fn is_read_only_agent(agent: &Agent) -> bool {
    [
        "write",
        "edit",
        "apply_patch",
        "replace",
        "terminal",
        "bash",
        "git",
        "commit",
    ]
    .iter()
    .all(|tool| {
        agent
            .permissions
            .get(*tool)
            .is_some_and(|level| level == "deny")
    })
}

fn has_explicit_git_write(agent: &Agent) -> bool {
    // `git_read_only` grants inspection, not write authority: the tool refuses
    // every invocation codegg-git does not classify read-only, so this agent
    // has not been given the ability to change the repository and must not be
    // treated as having it (which would push it into isolation and, below,
    // strip its `commit`).
    if agent.git_read_only {
        return false;
    }
    agent
        .permissions
        .get("git")
        .is_some_and(|level| level.eq_ignore_ascii_case("allow"))
}

fn read_only_blocked_tools() -> Vec<String> {
    [
        "write",
        "edit",
        "apply_patch",
        "replace",
        "terminal",
        "git",
        "commit",
        "image",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn delegation_key(request: &SubAgentRequest) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(request.parent_id.as_deref().unwrap_or("<root>").as_bytes());
    hasher.update([0]);
    hasher.update(request.agent.as_bytes());
    hasher.update([0]);
    hasher.update(request.description.as_bytes());
    hasher.update([0]);
    hasher.update(request.prompt.as_bytes());
    format!("delegation-{:x}", hasher.finalize())
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn request(task_id: u64) -> SubAgentRequest {
        SubAgentRequest {
            task_id,
            prompt: "test".into(),
            agent: "build".into(),
            parent_id: Some("root-a".into()),
            parent_run_id: None,
            run_id: None,
            denied_tools: Vec::new(),
            allowed_paths: Vec::new(),
            description: format!("task-{task_id}"),
            depth: 0,
            max_tool_calls: None,
            parent_model: None,
            workspace_root: None,
            workspace_locks: None,
            parent_sandbox_profile: None,
            sandbox_profile: None,
        }
    }

    #[test]
    fn concurrent_admission_is_atomic_and_releases_once() {
        let registry = Arc::new(AdmissionRegistry {
            state: Mutex::new(AdmissionState::default()),
            max_active: 2,
            max_direct_children: usize::MAX,
            max_total_child_tool_calls: usize::MAX,
        });
        let barrier = Arc::new(Barrier::new(8));
        let mut joins = Vec::new();
        for task_id in 0..8 {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                registry.admit(&request(task_id), 0).ok()
            }));
        }
        let leases: Vec<_> = joins.into_iter().map(|join| join.join().unwrap()).collect();
        assert_eq!(leases.iter().filter(|lease| lease.is_some()).count(), 2);
        assert_eq!(registry.active_count(), 2);
        drop(leases);
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn rejected_admission_does_not_consume_identity_or_budget() {
        let registry = Arc::new(AdmissionRegistry {
            state: Mutex::new(AdmissionState::default()),
            max_active: 1,
            max_direct_children: usize::MAX,
            max_total_child_tool_calls: 3,
        });
        let lease = registry.admit(&request(1), 3).unwrap();
        assert!(registry.admit(&request(2), 1).is_err());
        assert_eq!(registry.active_count(), 1);
        drop(lease);
        assert_eq!(registry.active_count(), 0);
        assert!(registry.admit(&request(2), 0).is_ok());
    }

    #[test]
    fn poisoned_admission_lock_is_recovered() {
        let registry = Arc::new(AdmissionRegistry {
            state: Mutex::new(AdmissionState::default()),
            max_active: 1,
            max_direct_children: usize::MAX,
            max_total_child_tool_calls: usize::MAX,
        });
        let lease = registry.admit(&request(1), 0).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = registry.state.lock().unwrap();
            panic!("poison admission lock");
        }));

        drop(lease);
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn scheduler_validation_markers_become_structured_evidence() {
        let events = vec![crate::provider::ChatEvent::ToolResult {
            tool_call_id: Arc::new("test-call".into()),
            content: Arc::new(
                "passed\nCODEGG_VALIDATION_RESULT:{\"job_id\":\"job-1\",\"kind\":\"test\",\"status\":\"passed\",\"summary\":\"12 tests passed\"}"
                    .into(),
            ),
        }];
        let (validation, artifacts) = extract_validation_evidence(&events);
        assert_eq!(validation.len(), 1);
        assert_eq!(
            validation[0].status,
            codegg_core::run_result::ValidationStatus::Passed
        );
        assert_eq!(validation[0].summary, "12 tests passed");
        assert_eq!(artifacts[0].reference.as_deref(), Some("job:job-1"));
    }

    /// The provider/model split a delegated run performs.
    ///
    /// Kept as a named helper so the rule is testable directly: the identity is
    /// derived by the same helper the main loop uses, and the model half is the
    /// **bare** id. The bug this guards is that the child sent
    /// `opencode_go/longcat-2.5-preview-free` where the provider's wire profile
    /// matches on `longcat-2.5-preview-free`, so every adapted-provider
    /// subagent failed closed with "has no reviewed wire mapping" while the
    /// parent turn on the same connection succeeded.
    fn split_runtime_model(resolved_model: &str) -> (String, String) {
        let provider =
            crate::core::session_selection::runtime_model_provider_segment(resolved_model)
                .to_string();
        let model = match resolved_model.split_once('/') {
            Some((_, model_id)) => model_id.to_string(),
            None => resolved_model.to_string(),
        };
        (provider, model)
    }

    #[test]
    fn delegated_provider_and_model_are_split_like_the_parent_turn() {
        // The exact string that failed live.
        assert_eq!(
            split_runtime_model("opencode_go/longcat-2.5-preview-free"),
            (
                "opencode_go".to_string(),
                "longcat-2.5-preview-free".to_string()
            )
        );
    }

    #[test]
    fn a_bare_model_id_never_falls_back_to_openai() {
        // The old worker did `split('/').next()…unwrap_or("openai")`. The
        // regression that matters is the silent `openai` default: with no
        // provider segment to go on it invented one, so a bare id was routed
        // to whatever `openai` happened to be configured as instead of failing
        // with a name. `runtime_model_provider_segment` keeps the whole string
        // (it only strips a storage-key prefix), so the lookup simply misses and
        // the caller reports an unknown provider.
        let (provider, model) = split_runtime_model("gpt-4o");
        assert_eq!(model, "gpt-4o");
        assert_ne!(
            provider, "openai",
            "a bare model id must never be retargeted at openai"
        );
        assert_eq!(provider, "gpt-4o");
    }

    #[test]
    fn a_storage_key_provider_segment_is_normalised() {
        // `other:opencode_go` is a storage key, not a registry id; the helper
        // strips the prefix exactly as `ProviderKind::from_storage_key` in
        // reverse would produce it.
        assert_eq!(
            split_runtime_model("other:opencode_go/glongcat-2.5-preview-free"),
            (
                "opencode_go".to_string(),
                "glongcat-2.5-preview-free".to_string()
            )
        );
    }

    #[test]
    fn read_only_git_is_not_treated_as_git_write_authority() {
        // `git_read_only` grants inspection only. Counting it as git write
        // would re-create the isolation regression: a read-only agent must not
        // be pushed into a managed worktree merely for being allowed to read
        // `git status`.
        let mut agent = Agent {
            name: "explore".into(),
            description: "read-only".into(),
            mode: crate::agent::definition::AgentMode::Subagent,
            git_read_only: true,
            ..Agent::default()
        };
        agent.permissions.insert("git".into(), "allow".into());
        assert!(
            !has_explicit_git_write(&agent),
            "git_read_only must not count as explicit git write"
        );
    }

    #[test]
    fn git_allow_without_read_only_still_counts_as_write() {
        let mut agent = Agent {
            git_read_only: false,
            name: "build".into(),
            description: "writer".into(),
            mode: crate::agent::definition::AgentMode::Primary,
            ..Agent::default()
        };
        agent.permissions.insert("git".into(), "allow".into());
        assert!(has_explicit_git_write(&agent));
    }

    /// `SessionStore` needs a pool; these assertions are about agent
    /// classification and never touch it.
    async fn test_pool() -> SqlitePool {
        SqlitePool::connect("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn a_read_only_git_agent_does_not_require_isolation() {
        // Uses the real shipped `explore` definition rather than a hand-built
        // stand-in: this is about the agent an operator actually gets, and a
        // fixture that quietly forgot a `deny` would pass vacuously.
        let pool = SubAgentPool::new_with_store(
            &Config::default(),
            Arc::new(TokioMutex::new(TaskStore::new())),
            crate::agent::builtins::generated_builtin_agents(),
            ProviderRegistry::new(),
            Arc::new(SessionStore::new(test_pool().await)),
            None,
        )
        .await;

        let explore = pool
            .agents
            .iter()
            .find(|agent| agent.name == "explore")
            .expect("explore is a built-in agent");
        assert!(
            explore.git_read_only,
            "explore must declare read-only git inspection"
        );
        assert!(
            !explore
                .permissions
                .get("git")
                .is_some_and(|level| level.eq_ignore_ascii_case("deny")),
            "explore must not deny git outright any more"
        );
        assert!(
            !pool.requires_isolation("explore", &[]),
            "a read-only git agent must not be classified as mutating"
        );
    }

    #[tokio::test]
    async fn read_only_agents_do_not_demand_a_worktree() {
        // The exact regression: every one of these denied every mutating tool
        // and still demanded isolation, so every read-only subagent failed with
        // `isolation_unavailable`.
        let pool = SubAgentPool::new_with_store(
            &Config::default(),
            Arc::new(TokioMutex::new(TaskStore::new())),
            crate::agent::builtins::generated_builtin_agents(),
            ProviderRegistry::new(),
            Arc::new(SessionStore::new(test_pool().await)),
            None,
        )
        .await;
        for name in ["explore", "plan", "research", "verifier"] {
            assert!(
                !pool.requires_isolation(name, &[]),
                "{name} is a read-only agent and must not require isolation"
            );
        }
    }

    #[tokio::test]
    async fn a_git_write_agent_still_requires_isolation() {
        let mut build = Agent {
            git_read_only: false,
            name: "build".into(),
            description: "writer".into(),
            mode: crate::agent::definition::AgentMode::Primary,
            ..Agent::default()
        };
        build.permissions.insert("git".into(), "allow".into());
        let pool = SubAgentPool::new_with_store(
            &Config::default(),
            Arc::new(TokioMutex::new(TaskStore::new())),
            vec![build],
            ProviderRegistry::new(),
            Arc::new(SessionStore::new(test_pool().await)),
            None,
        )
        .await;
        assert!(
            pool.requires_isolation("build", &[]),
            "git write authority must still demand isolation"
        );
    }
}
