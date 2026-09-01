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
}

impl SubAgentResult {
    pub fn success(task_id: u64, result: String) -> Self {
        Self {
            task_id,
            success: true,
            result,
            report: None,
        }
    }

    pub fn success_with_report(task_id: u64, result: String, report: SubAgentReport) -> Self {
        Self {
            task_id,
            success: true,
            result,
            report: Some(report),
        }
    }

    pub fn failure(task_id: u64, error: String) -> Self {
        Self {
            task_id,
            success: false,
            result: error,
            report: None,
        }
    }
}

struct WorkerRequest {
    request: SubAgentRequest,
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
                    Some(WorkerRequest { request, response_tx, lease, lineage_token, scheduler_cancel, scheduled }) = request_rx.recv() => {
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
        self.enqueue_request_with_mode(request, CancellationToken::new(), false)
            .await
    }

    async fn enqueue_request_with_mode(
        &self,
        request: SubAgentRequest,
        scheduler_cancel: CancellationToken,
        scheduled: bool,
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
        self.send_and_wait_with_mode(request, CancellationToken::new(), false)
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
        self.send_and_wait_with_mode(request, scheduler_cancel, true)
            .await
    }

    async fn send_and_wait_with_mode(
        &self,
        request: SubAgentRequest,
        scheduler_cancel: CancellationToken,
        scheduled: bool,
    ) -> Result<SubAgentResult, String> {
        let task_id = request.task_id;
        let response = self
            .enqueue_request_with_mode(request, scheduler_cancel, scheduled)
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
                Ok((output, report)) => {
                    GlobalEventBus::publish(AppEvent::SubagentCompleted {
                        session_id: session_id.clone(),
                        task_id,
                        agent: request.agent.clone(),
                        result_summary: output.chars().take(200).collect(),
                    });
                    // Don't update task store here - let handle_response do it
                    if let Some(report) = report {
                        SubAgentResult::success_with_report(task_id, output, report)
                    } else {
                        SubAgentResult::success(task_id, output)
                    }
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
    agents: Arc<Vec<Agent>>,
    provider_registry: Arc<ProviderRegistry>,
    config: Arc<Config>,
    _session_store: Arc<SessionStore>,
    pool: Option<SqlitePool>,
    subagent_pool: Arc<SubAgentPool>,
) -> Result<(String, Option<SubAgentReport>), Box<dyn std::error::Error + Send + Sync>> {
    let agent_name = &request.agent;
    let agent = agents
        .iter()
        .find(|a| a.name == *agent_name)
        .ok_or_else(|| format!("Agent '{}' not found", agent_name))?;

    // Phase 3: Apply safety envelope to prevent custom files from escalating
    // permissions beyond session/config/hard policy bounds.
    let session_rules = crate::permission::PermissionRuleset::default();
    let config_rules = crate::permission::config_ruleset(Some(&config));
    let hard_deny = vec![
        "commit".to_string(),
        "todowrite".to_string(),
        "todoread".to_string(),
    ];
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

    // Extract provider name from the resolved model (format: "provider/model").
    let provider_name = profile
        .resolved_model
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("openai")
        .to_string();

    let provider = provider_registry
        .get(&provider_name)
        .ok_or_else(|| format!("Provider '{}' not found", provider_name))?
        .clone_box();

    let mut tool_registry = ToolRegistry::with_config(&config);
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
        let durable_submission = subagent_pool.durable_submission.lock().await.clone();
        let durable_agent_runs = subagent_pool.durable_agent_runs.lock().await.clone();
        let durable_run_control = subagent_pool.durable_run_control.lock().await.clone();
        let mut inherited_denied = request.denied_tools.clone();
        if parent_is_read_only {
            inherited_denied.extend(read_only_blocked_tools());
            inherited_denied.sort();
            inherited_denied.dedup();
        }
        let task_tool = crate::tool::task::TaskTool::new(
            subagent_pool.task_store(),
            Some(subagent_pool.spawner()),
            Some(request.parent_id.clone().unwrap_or_default()),
            inherited_denied,
        )
        .with_depth(request.depth)
        .with_parent_run_id(request.parent_run_id.clone())
        .with_parent_model(Some(profile.resolved_model.clone()))
        .with_workspace_root(request.workspace_root.clone())
        .with_parent_allowed_paths(request.allowed_paths.clone());
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
    let adapter = codegg_core::model_profile::resolve_adapter(None, &profile.resolved_model);
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

    let permission_checker =
        PermissionChecker::new(Some(&config), None).with_agent_rules(agent_rules);

    // Bootstraps the search backend (eggsearch by default) before the agent
    // loop starts. The bootstrap is idempotent: if the parent process has
    // already populated the global `McpService` slot, this returns the
    // existing service without spawning a new eggsearch connection. If the
    // subagent is spawned in a context where no parent has bootstrapped,
    // this gives the subagent a chance to set up its own service. If the
    // backend is `disabled`, this returns `None` and the loop runs without
    // MCP tools.
    let (mcp_service, _report) =
        crate::search_backend::bootstrap::bootstrap_search_backend(&config).await;

    let subagent_session_id = request
        .parent_id
        .as_ref()
        .map(|parent_id| format!("{}-sub-{}", parent_id, request.task_id))
        .unwrap_or_else(|| format!("subagent-{}", request.task_id));
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
        crate::model_profile::ModelProfileResolver::new(&config).resolve(&profile.resolved_model);
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

    let model = profile.resolved_model.clone();
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

    Ok((output, report))
}

fn is_read_only_agent(agent: &Agent) -> bool {
    ["write", "edit", "apply_patch", "replace", "multiedit"]
        .iter()
        .all(|tool| {
            agent
                .permissions
                .get(*tool)
                .is_some_and(|level| level == "deny")
        })
}

fn read_only_blocked_tools() -> Vec<String> {
    [
        "write",
        "edit",
        "apply_patch",
        "replace",
        "multiedit",
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
}
