use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
                            f.line
                                .map(|l| format!(":{}", l))
                                .unwrap_or_default()
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
    pub prompt: String,
    pub agent: String,
    pub parent_id: Option<String>,
    pub denied_tools: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub description: String,
    pub depth: usize,
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
}

pub struct SubAgentPool {
    active_count: Arc<AtomicUsize>,
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
        let (request_tx, request_rx) = mpsc::channel(max_concurrent * 2);
        let active_count = Arc::new(AtomicUsize::new(0));
        let task_store = Arc::new(TokioMutex::new(TaskStore::new()));
        if let Some(ref p) = pool {
            task_store.lock().await.set_pool(p.clone());
        }
        let workers = Arc::new(TokioMutex::new(Vec::new()));
        let cancel_token = CancellationToken::new();
        let active_handles = Arc::new(TokioMutex::new(Vec::new()));

        let pool_inst = Self {
            active_count,
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
        let (request_tx, request_rx) = mpsc::channel(max_concurrent * 2);
        let active_count = Arc::new(AtomicUsize::new(0));
        let workers = Arc::new(TokioMutex::new(Vec::new()));
        let cancel_token = CancellationToken::new();
        let active_handles = Arc::new(TokioMutex::new(Vec::new()));
        if let Some(ref p) = pool {
            task_store.lock().await.set_pool(p.clone());
        }

        let pool_inst = Self {
            active_count,
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
        };

        let pool_clone = pool_inst.clone();
        pool_clone.start_worker_loop(request_rx);

        pool_inst
    }

    fn start_worker_loop(&self, mut request_rx: mpsc::Receiver<WorkerRequest>) {
        let cancel_token = self.cancel_token.clone();
        let active_count = Arc::clone(&self.active_count);
        let task_store = Arc::clone(&self.task_store);
        let max_concurrent = self.max_concurrent;
        let agents = Arc::clone(&self.agents);
        let provider_registry = Arc::clone(&self.provider_registry);
        let config = Arc::clone(&self.config);
        let session_store = Arc::clone(&self.session_store);
        let workers = Arc::clone(&self.workers);
        let active_handles = Arc::clone(&self.active_handles);
        let db_pool = self.pool.clone();

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
                    Some(WorkerRequest { request, response_tx }) = request_rx.recv() => {
                        if cancel_token.is_cancelled() {
                            let _ = response_tx.send(SubAgentResult::failure(
                                request.task_id,
                                "pool shutting down".to_string(),
                            ));
                            continue;
                        }

                        let sem = Arc::clone(&sem);
                        let active_count = Arc::clone(&active_count);
                        let task_store = Arc::clone(&task_store);
                        let agents = Arc::clone(&agents);
                        let provider_registry = Arc::clone(&provider_registry);
                        let config = Arc::clone(&config);
                        let session_store = Arc::clone(&session_store);
                        let cancel_token = cancel_token.clone();
                        let db_pool = db_pool.clone();

                        let handle = tokio::spawn(async move {
                            // RAII guard for active_count
                            struct ActiveCountGuard {
                                active_count: Arc<AtomicUsize>,
                            }

                            impl ActiveCountGuard {
                                fn new(active_count: Arc<AtomicUsize>) -> Self {
                                    active_count.fetch_add(1, Ordering::SeqCst);
                                    Self { active_count }
                                }
                            }

                            impl Drop for ActiveCountGuard {
                                fn drop(&mut self) {
                                    self.active_count.fetch_sub(1, Ordering::SeqCst);
                                }
                            }

                            let _guard = ActiveCountGuard::new(active_count);

                            // Wait for semaphore permit, but also check for cancellation
                            let permit = tokio::select! {
                                biased;
                                _ = cancel_token.cancelled() => {
                                    let _ = response_tx.send(SubAgentResult::failure(
                                        request.task_id,
                                        "pool shutting down".to_string(),
                                    ));
                                    return;
                                }
                                result = sem.acquire() => {
                                    match result {
                                        Ok(p) => p,
                                        Err(e) => {
                                            tracing::error!("Failed to acquire semaphore: {}", e);
                                            let _ = response_tx.send(SubAgentResult::failure(
                                                request.task_id,
                                                format!("Worker semaphore error: {}", e),
                                            ));
                                            return;
                                        }
                                    }
                                }
                            };

                            let result = run_subagent_task_with_cancel(
                                request,
                                task_store,
                                agents,
                                provider_registry,
                                config,
                                session_store,
                                cancel_token,
                                db_pool,
                            ).await;

                            let _ = response_tx.send(result);
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
        self.active_count.load(Ordering::SeqCst)
    }

    pub fn task_store(&self) -> Arc<TokioMutex<TaskStore>> {
        self.task_store.clone()
    }

    pub async fn shutdown(&self) {
        tracing::info!("SubAgentPool initiating shutdown");
        self.cancel_token.cancel();

        // Wait briefly for cooperative cancellation to finish
        let mut attempts = 0;
        while self.active_count.load(Ordering::SeqCst) > 0 && attempts < 10 {
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
            let _ = handle.await;
        }

        // Wait for aborted tasks to complete (active_count to reach 0)
        let mut attempts = 0;
        while self.active_count.load(Ordering::SeqCst) > 0 && attempts < 10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            attempts += 1;
        }

        let final_count = self.active_count.load(Ordering::SeqCst);
        tracing::info!(
            "SubAgentPool shutdown complete, final active count: {}",
            final_count
        );
    }
}

impl Clone for SubAgentPool {
    fn clone(&self) -> Self {
        Self {
            active_count: Arc::clone(&self.active_count),
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
                    .set_failed(task_id, format!("worker error: {}", e))
                    .await;
            }
        }
    }

    async fn enqueue_request(
        &self,
        request: SubAgentRequest,
    ) -> Result<oneshot::Receiver<SubAgentResult>, String> {
        if request.depth >= self.pool.max_depth {
            return Err(format!(
                "subagent max depth {} exceeded (request depth: {})",
                self.pool.max_depth, request.depth
            ));
        }

        let (response_tx, response_rx) = oneshot::channel();
        let worker_request = WorkerRequest {
            request,
            response_tx,
        };

        self.pool
            .request_tx
            .send(worker_request)
            .await
            .map_err(|e| format!("failed to queue request: {}", e))?;

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
}

async fn run_subagent_task_with_cancel(
    request: SubAgentRequest,
    task_store: Arc<TokioMutex<TaskStore>>,
    agents: Arc<Vec<Agent>>,
    provider_registry: Arc<ProviderRegistry>,
    config: Arc<Config>,
    session_store: Arc<SessionStore>,
    cancel_token: CancellationToken,
    pool: Option<SqlitePool>,
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
        result = execute_agent_task(
            &request,
            agents,
            provider_registry,
            config,
            session_store,
            pool,
        ) => {
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
) -> Result<(String, Option<SubAgentReport>), Box<dyn std::error::Error + Send + Sync>> {
    let agent_name = &request.agent;
    let agent = agents
        .iter()
        .find(|a| a.name == *agent_name)
        .ok_or_else(|| format!("Agent '{}' not found", agent_name))?;

    let provider_name = agent
        .model
        .as_ref()
        .and_then(|m| m.split('/').next())
        .unwrap_or("openai")
        .to_string();

    let provider = provider_registry
        .get(&provider_name)
        .ok_or_else(|| format!("Provider '{}' not found", provider_name))?
        .clone_box();

    let mut tool_registry = ToolRegistry::with_defaults();
    if !request.denied_tools.is_empty() {
        tool_registry.filter_out(&request.denied_tools);
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

    let permission_checker = PermissionChecker::new(Some(&config), None).with_agent_rules(agent_rules);

    let mut agent_loop = AgentLoop::new(
        agents.iter().cloned().collect(),
        provider,
        permission_checker,
        tool_registry,
        (*config).clone(),
        None,
        pool,
    );

    if let Some(parent_id) = &request.parent_id {
        let subagent_session_id = format!("{}-sub-{}", parent_id, request.task_id);
        agent_loop.set_session_id(&subagent_session_id);
    }

    if agent_name == "plan" {
        agent_loop.enter_plan_mode(Some(request.description.clone()));
    }

    agent_loop.set_agent(agent_name)?;

    let mut messages = Vec::new();
    if let Some(ref system_prompt) = agent.system_prompt {
        messages.push(crate::provider::Message::System {
            content: system_prompt.clone().into(),
        });
    }

    messages.push(crate::provider::Message::User {
        content: vec![crate::provider::ContentPart::Text {
            text: request.prompt.clone().into(),
        }],
    });

    let model = agent.model.clone().unwrap_or_default();
    let request = crate::provider::ChatRequest {
        messages,
        model,
        tools: None,
        system: None,
        temperature: agent.temperature,
        top_p: agent.top_p,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
    };

    let events = agent_loop.run(request).await?;

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
