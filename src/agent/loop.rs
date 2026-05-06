//! Agent Loop - orchestrates conversation between LLM and tools.
//!
//! The agent loop manages the core execution cycle:
//! 1. Send messages to provider (LLM)
//! 2. Receive tool calls from provider
//! 3. Execute tools via ToolRegistry
//! 4. Handle permissions via PermissionChecker
//! 5. Return results to provider
//!
//! Key components:
//! - `AgentLoop` - main orchestration struct
//! - `AgentLoopState` - tracks turn count, tokens, plan mode
//! - `ExecutionLimits` - bounds on turns, tokens, timeouts
//! - `ContextTracker` - monitors token usage for compaction

use crate::agent::compaction::{
    auto_compact_async, compact_messages_sync, detect_overflow, prune_tool_outputs,
    CompactionStrategy, ContextTracker,
};
use crate::agent::processor::EventProcessor;
use crate::agent::router::ModelRouter;
use crate::agent::Agent;
use crate::bus::events::AppEvent;
use crate::bus::{PermissionRegistry, QuestionRegistry};
use crate::config::schema::Config;
use crate::error::{AgentError, AppError, ProviderError, ToolError};
use crate::permission::{DoomLoopDetector, PermissionChecker, PermissionChoice, PermissionResult};
use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message, ToolCall};
use crate::tool::plan::detect_plan_mode_change;
use crate::tool::question::{format_question_answers, parse_question_questions};
use crate::tool::ToolRegistry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

static PATH_REDACTION_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    let patterns = [
        r"/home/[^/\s]+",
        r"/Users/[^/\s]+",
        r"/var/[^/\s]+",
        r"/tmp/[^/\s]+",
        r"C:\\Users\\[^\\]+",
        r"C:\\Program Files\\[^\\]+",
        r"C:\\Windows\\[^\\]+",
    ];
    patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
});
use tokio::sync::mpsc;
use tracing::instrument;

type ToolDefCache = (
    Option<String>,
    bool,
    bool,
    usize,
    u64,
    Vec<crate::provider::ToolDefinition>,
);

fn redact_local_paths(input: &str) -> String {
    let mut result = input.to_string();

    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.to_string_lossy();
        if !cwd_str.is_empty() {
            result = result.replace(&*cwd_str, "[CWD]");
        }

        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            result = result.replace(&home, "[HOME]");
        }
    }

    for re in PATH_REDACTION_PATTERNS.iter() {
        result = re.replace_all(&result, "[REDACTED_PATH]").to_string();
    }

    result
}

#[derive(Copy, Clone)]
struct ModelFlags {
    is_gpt: bool,
    is_non_oss: bool,
    needs_exa: bool,
    exa_available: bool,
}

pub struct ToolTimeoutConfig {
    pub bash: Duration,
    pub read: Duration,
    pub write: Duration,
    pub edit: Duration,
    pub glob: Duration,
    pub grep: Duration,
    pub list: Duration,
    pub task: Duration,
    pub webfetch: Duration,
    pub websearch: Duration,
    pub codesearch: Duration,
    pub diff: Duration,
    pub replace: Duration,
    pub multiedit: Duration,
    pub apply_patch: Duration,
    pub terminal: Duration,
    pub batch: Duration,
    pub lsp: Duration,
    pub skill: Duration,
    pub git: Duration,
    pub todo: Duration,
    pub question: Duration,
    pub default_timeout: Duration,
}

impl Default for ToolTimeoutConfig {
    fn default() -> Self {
        Self {
            bash: Duration::from_secs(120),
            read: Duration::from_secs(60),
            write: Duration::from_secs(60),
            edit: Duration::from_secs(60),
            glob: Duration::from_secs(30),
            grep: Duration::from_secs(60),
            list: Duration::from_secs(30),
            task: Duration::from_secs(300),
            webfetch: Duration::from_secs(30),
            websearch: Duration::from_secs(60),
            codesearch: Duration::from_secs(60),
            diff: Duration::from_secs(30),
            replace: Duration::from_secs(30),
            multiedit: Duration::from_secs(60),
            apply_patch: Duration::from_secs(60),
            terminal: Duration::from_secs(120),
            batch: Duration::from_secs(300),
            lsp: Duration::from_secs(60),
            skill: Duration::from_secs(30),
            git: Duration::from_secs(60),
            todo: Duration::from_secs(30),
            question: Duration::from_secs(30),
            default_timeout: Duration::from_secs(120),
        }
    }
}

/// Check if a tool modifies files (requires snapshot before execution)
fn is_file_modifying_tool(name: &str) -> bool {
    matches!(
        name,
        "write" | "edit" | "replace" | "multiedit" | "apply_patch"
    )
}

impl AgentLoop {
    fn get_tool_timeout(&self, tool_name: &str) -> Duration {
        let cfg = ToolTimeoutConfig::default();
        match tool_name {
            "bash" => self.timeout_for_tool(tool_name, cfg.bash),
            "read" => self.timeout_for_tool(tool_name, cfg.read),
            "write" => self.timeout_for_tool(tool_name, cfg.write),
            "edit" => self.timeout_for_tool(tool_name, cfg.edit),
            "glob" => self.timeout_for_tool(tool_name, cfg.glob),
            "grep" => self.timeout_for_tool(tool_name, cfg.grep),
            "list" => self.timeout_for_tool(tool_name, cfg.list),
            "task" => self.timeout_for_tool(tool_name, cfg.task),
            "webfetch" => self.timeout_for_tool(tool_name, cfg.webfetch),
            "websearch" => self.timeout_for_tool(tool_name, cfg.websearch),
            "codesearch" => self.timeout_for_tool(tool_name, cfg.codesearch),
            "diff" => self.timeout_for_tool(tool_name, cfg.diff),
            "replace" => self.timeout_for_tool(tool_name, cfg.replace),
            "multiedit" => self.timeout_for_tool(tool_name, cfg.multiedit),
            "apply_patch" => self.timeout_for_tool(tool_name, cfg.apply_patch),
            "terminal" => self.timeout_for_tool(tool_name, cfg.terminal),
            "batch" => self.timeout_for_tool(tool_name, cfg.batch),
            "lsp" => self.timeout_for_tool(tool_name, cfg.lsp),
            "skill" => self.timeout_for_tool(tool_name, cfg.skill),
            "git" => self.timeout_for_tool(tool_name, cfg.git),
            "todo" => self.timeout_for_tool(tool_name, cfg.todo),
            "question" => self.timeout_for_tool(tool_name, cfg.question),
            _ => self.timeout_for_tool(tool_name, cfg.default_timeout),
        }
    }

    fn timeout_for_tool(&self, _tool_name: &str, default: Duration) -> Duration {
        self.config
            .server
            .as_ref()
            .and_then(|s| s.tool_timeout_seconds)
            .map(Duration::from_secs)
            .unwrap_or(default)
    }
}

fn extract_path_from_tool_call(tc: &ToolCall) -> Option<String> {
    let args = &tc.arguments;
    match tc.name.as_str() {
        "read" | "write" | "edit" | "glob" | "grep" | "list" => {
            args.get("path")?.as_str().map(String::from)
        }
        "apply_patch" => args.get("patch_path")?.as_str().map(String::from),
        _ => None,
    }
}

fn extract_bash_command(tc: &ToolCall) -> Option<String> {
    if &*tc.name != "bash" {
        return None;
    }
    tc.arguments.get("command")?.as_str().map(String::from)
}

fn extract_git_subcommand(tc: &ToolCall) -> Option<String> {
    if &*tc.name != "git" {
        return None;
    }
    tc.arguments.get("subcommand")?.as_str().map(String::from)
}

fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    let parts: Vec<&str> = rest.split("__").collect();
    if parts.len() == 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

fn is_auto_accept_read_only_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read" | "glob" | "grep" | "list" | "webfetch" | "websearch" | "codesearch"
    )
}

fn is_path_within_working_directory(path: Option<&str>) -> bool {
    let cwd = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let Some(raw_path) = path else {
        // For tools like glob, missing path means "use cwd".
        return true;
    };

    let candidate = {
        let p = std::path::PathBuf::from(raw_path);
        if p.is_absolute() { p } else { cwd.join(p) }
    };

    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    canonical.starts_with(&cwd)
}

enum ToolPermissionOutcome {
    QuestionTool,
    Allowed(ToolCall),
    Denied { tool_id: String, message: String },
}

impl AgentLoop {
    async fn check_tool_permission(&mut self, tc: &ToolCall) -> ToolPermissionOutcome {
        if tc.name.trim().is_empty() {
            return ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message: "Error: invalid tool call with empty tool name".to_string(),
            };
        }

        if &*tc.name == "question" {
            if let Ok(questions) = parse_question_questions(tc.arguments.clone()) {
                let questions_json = serde_json::to_string(&questions).unwrap_or_default();
                let (tx, rx) = tokio::sync::oneshot::channel();
                QuestionRegistry::register(self.session_id.clone(), tx);
                crate::bus::global::GlobalEventBus::publish(AppEvent::QuestionPending {
                    session_id: self.session_id.clone(),
                    questions: questions_json,
                });
                self.question_rx = Some(rx);
                return ToolPermissionOutcome::QuestionTool;
            }
        }

        self.doom_detector.record_tool_call(&tc.name);
        let doom_loop = self.doom_detector.is_doom_loop();

        let path = extract_path_from_tool_call(tc);
        let bash_command = extract_bash_command(tc);
        let git_subcommand = extract_git_subcommand(tc);

        let perm_result = if bash_command.is_some() {
            self.permission_checker
                .check_bash(
                    path.as_deref(),
                    bash_command.as_deref(),
                    Some(&self.session_id),
                )
                .await
        } else if git_subcommand.is_some() {
            self.permission_checker
                .check_git(
                    path.as_deref(),
                    git_subcommand.as_deref(),
                    Some(&self.session_id),
                )
                .await
        } else {
            self.permission_checker
                .check(&tc.name, path.as_deref(), Some(&self.session_id))
                .await
        };
        match perm_result {
            PermissionResult::Allow => {
                if doom_loop {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!(
                            "Tool '{}' denied: potential doom loop detected (repeated identical tool calls)",
                            tc.name
                        ),
                    }
                } else {
                    ToolPermissionOutcome::Allowed(tc.clone())
                }
            }
            PermissionResult::Deny => ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message: format!("Tool '{}' denied by permissions", tc.name),
            },
            PermissionResult::Ask(req) => {
                if is_auto_accept_read_only_tool(tc.name.as_str())
                    && is_path_within_working_directory(req.path.as_deref())
                {
                    if doom_loop {
                        return ToolPermissionOutcome::Denied {
                            tool_id: tc.id.to_string(),
                            message: format!(
                                "Tool '{}' denied: potential doom loop detected (repeated identical tool calls)",
                                tc.name
                            ),
                        };
                    }
                    return ToolPermissionOutcome::Allowed(tc.clone());
                }

                let perm_id = format!("{}-{}", tc.id, tc.name);
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                PermissionRegistry::register(perm_id.clone(), resp_tx);
                crate::bus::global::GlobalEventBus::publish(AppEvent::PermissionPending {
                    session_id: self.session_id.clone(),
                    perm_id: perm_id.clone(),
                    tool: req.tool.clone(),
                    path: req.path.clone(),
                    args: req.args.clone(),
                });
                let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
                    Ok(Ok(choice)) => choice,
                    _ => PermissionChoice::DenyOnce,
                };
                PermissionRegistry::unregister(&perm_id);
                let allowed = choice.allowed();
                if choice.persist() {
                    if allowed {
                        self.permission_checker
                            .always_allow(&tc.name, req.path.as_deref(), Some(&self.session_id))
                            .await;
                    } else {
                        self.permission_checker
                            .always_deny(&tc.name, req.path.as_deref(), Some(&self.session_id))
                            .await;
                    }
                }
                if allowed {
                    if doom_loop {
                        ToolPermissionOutcome::Denied {
                            tool_id: tc.id.to_string(),
                            message: format!(
                                "Tool '{}' denied: potential doom loop detected (repeated identical tool calls)",
                                tc.name
                            ),
                        }
                    } else {
                        ToolPermissionOutcome::Allowed(tc.clone())
                    }
                } else {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!("Tool '{}' denied by user", tc.name),
                    }
                }
            }
        }
    }
}

pub struct AgentLoopState {
    pub current_agent: String,
    pub turn_count: usize,
    pub total_tokens: usize,
    pub start_time: Instant,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
}

pub struct ExecutionLimits {
    pub max_turns: usize,
    pub max_tokens: usize,
    pub timeout: Duration,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 100,
            max_tokens: 1_000_000,
            timeout: Duration::from_secs(600),
        }
    }
}

pub struct AgentLoop {
    agents: HashMap<String, Agent>,
    state: AgentLoopState,
    limits: ExecutionLimits,
    provider: Box<dyn crate::provider::Provider>,
    permission_checker: PermissionChecker,
    tool_registry: ToolRegistry,
    hook_registry: Option<Arc<crate::hooks::HookRegistry>>,
    context_tracker: ContextTracker,
    doom_detector: DoomLoopDetector,
    steering: AtomicBool,
    follow_up_tx: mpsc::UnboundedSender<String>,
    follow_up_rx: mpsc::UnboundedReceiver<String>,
    config: Config,
    question_tx: Option<tokio::sync::oneshot::Sender<String>>,
    question_rx: Option<tokio::sync::oneshot::Receiver<String>>,
    plugin_service: Option<Arc<crate::plugin::service::PluginService>>,
    session_id: String,
    mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
    tool_def_cache: Option<ToolDefCache>,
    model_router: ModelRouter,
    snapshot_manager: Option<crate::snapshot::SnapshotManager>,
}

impl AgentLoop {
    pub fn new(
        agents: Vec<Agent>,
        provider: Box<dyn crate::provider::Provider>,
        permission_checker: PermissionChecker,
        tool_registry: ToolRegistry,
        config: Config,
        mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
        pool: Option<sqlx::SqlitePool>,
    ) -> Self {
        let mut map = HashMap::new();
        let mut default_name = "build".to_string();

        for agent in &agents {
            if agent.name == "build" {
                default_name = agent.name.clone();
            }
            map.insert(agent.name.clone(), agent.clone());
        }

        let (follow_up_tx, follow_up_rx) = mpsc::unbounded_channel();

        let mut context_tracker = ContextTracker::new(128_000, 0.85);
        if let Some(ref compaction) = config.compaction {
            if let Some(max_tokens) = compaction.max_tokens {
                context_tracker.set_limit(max_tokens);
            }
            if let Some(threshold) = compaction.threshold {
                context_tracker.set_threshold(threshold);
            }
        }

        let hook_registry = config
            .hooks
            .as_ref()
            .map(|hooks| Arc::new(crate::hooks::HookRegistry::from_config(hooks)));

        let model_router = ModelRouter::from_config(&config);

        let snapshot_manager = if config.snapshot.unwrap_or(false) {
            if let Some(pool) = pool {
                let project_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                Some(crate::snapshot::SnapshotManager::new(pool, project_root))
            } else {
                None
            }
        } else {
            None
        };

        Self {
            agents: map,
            state: AgentLoopState {
                current_agent: default_name,
                turn_count: 0,
                total_tokens: 0,
                start_time: Instant::now(),
                plan_mode: false,
                plan_topic: None,
            },
            limits: ExecutionLimits::default(),
            provider,
            permission_checker,
            tool_registry,
            hook_registry,
            context_tracker,
            doom_detector: DoomLoopDetector::new(
                50,
                config
                    .permission
                    .as_ref()
                    .and_then(|p| p.doomloop_threshold)
                    .unwrap_or(20),
            ),
            steering: AtomicBool::new(false),
            follow_up_tx,
            follow_up_rx,
            config,
            question_tx: None,
            question_rx: None,
            plugin_service: None,
            session_id: String::new(),
            mcp_service,
            tool_def_cache: None,
            model_router,
            snapshot_manager,
        }
    }

    pub fn set_agent(&mut self, name: &str) -> Result<(), AgentError> {
        if self.agents.contains_key(name) {
            self.state.current_agent = name.to_string();
            Ok(())
        } else {
            Err(AgentError::NotFound(name.to_string()))
        }
    }

    pub fn enter_plan_mode(&mut self, topic: Option<String>) {
        self.state.plan_mode = true;
        self.state.plan_topic = topic;
    }

    pub fn exit_plan_mode(&mut self) {
        self.state.plan_mode = false;
        self.state.plan_topic = None;
    }

    pub fn is_plan_mode(&self) -> bool {
        self.state.plan_mode
    }

    pub fn plan_topic(&self) -> Option<&str> {
        self.state.plan_topic.as_deref()
    }

    pub fn current_agent(&self) -> Option<&Agent> {
        self.agents.get(&self.state.current_agent)
    }

    pub fn agents(&self) -> &HashMap<String, Agent> {
        &self.agents
    }

    pub fn state(&self) -> &AgentLoopState {
        &self.state
    }

    pub fn set_limits(&mut self, limits: ExecutionLimits) {
        self.limits = limits;
    }

    pub fn set_max_turns(&mut self, turns: usize) {
        self.limits.max_turns = turns;
    }

    fn tool_timeout(&self) -> u64 {
        self.config
            .server
            .as_ref()
            .and_then(|s| s.tool_timeout_seconds)
            .unwrap_or(120)
    }

    fn permission_version(&self) -> u64 {
        if let Some(ref perm) = self.config.permission {
            let json = serde_json::to_string(perm).unwrap_or_default();
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            json.hash(&mut hasher);
            hasher.finish()
        } else {
            0
        }
    }

    fn max_parallel_tools(&self) -> usize {
        self.config
            .server
            .as_ref()
            .and_then(|s| s.max_parallel_tools)
            .unwrap_or(usize::MAX)
    }

    pub fn steering(&self) -> &AtomicBool {
        &self.steering
    }

    pub fn interrupt(&self) {
        self.steering.store(true, Ordering::SeqCst);
    }

    /// Returns a sender for queueing follow-up prompts.
    ///
    /// Follow-up contract:
    /// - Follow-ups queued BEFORE `run()` starts are processed by that `run()` call
    /// - Follow-ups that arrive AFTER `run()` has already returned are NOT consumed
    ///   (they require another `run()` call or alternative event-driven handling)
    /// - The channel is unbounded; callers should be mindful of memory if queueing many
    pub fn follow_up_sender(&self) -> mpsc::UnboundedSender<String> {
        self.follow_up_tx.clone()
    }

    pub fn setup_question_channel(&mut self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.question_tx = Some(tx);
        self.question_rx = Some(rx);
    }

    pub fn question_sender(&self) -> Option<&tokio::sync::oneshot::Sender<String>> {
        self.question_tx.as_ref()
    }

    pub fn context_tracker(&mut self) -> &mut ContextTracker {
        &mut self.context_tracker
    }

    pub fn set_plugin_service(&mut self, service: Arc<crate::plugin::service::PluginService>) {
        self.plugin_service = Some(service);
    }

    pub fn set_session_id(&mut self, id: &str) {
        self.session_id = id.to_string();
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    async fn stream_with_retry(&self, request: &ChatRequest) -> Result<Vec<ChatEvent>, AppError> {
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
        let max_retries = 3;
        let mut delay = Duration::from_secs(1);
        let mut last_retryable_err: Option<AppError> = None;

        for attempt in 0..max_retries {
            if attempt > 0 {
                tracing::info!("Retry attempt {} after {:?}", attempt, delay);
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2).min(MAX_RETRY_DELAY);
            }

            match self.stream_once(request).await {
                Ok(events) => return Ok(events),
                Err(e) => {
                    let is_retryable = matches!(
                        e,
                        AppError::Provider(ProviderError::RateLimit)
                            | AppError::Provider(ProviderError::Timeout(_))
                            | AppError::Provider(ProviderError::Stream(_))
                    );
                    if is_retryable {
                        last_retryable_err = Some(e);
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_retryable_err.unwrap_or_else(|| AppError::Provider(ProviderError::RateLimit)))
    }

    async fn stream_once(&self, request: &ChatRequest) -> Result<Vec<ChatEvent>, AppError> {
        let stream = tokio::time::timeout(Duration::from_secs(120), self.provider.stream(request))
            .await
            .map_err(|_| {
                AppError::Provider(ProviderError::Timeout(
                    "provider stream timeout".to_string(),
                ))
            })??;
        let mut events = Vec::with_capacity(64);
        let session_id_arc: Arc<str> = Arc::from(self.session_id.as_str());

        use futures::StreamExt;
        let mut stream = stream;
        while let Some(event) = stream.next().await {
            match event {
                Ok(evt) => {
                    match &evt {
                        ChatEvent::TextDelta(text) => {
                            crate::bus::global::GlobalEventBus::publish(AppEvent::TextDelta {
                                session_id: Arc::clone(&session_id_arc),
                                delta: Arc::from(text.as_str()),
                            });
                        }
                        ChatEvent::ReasoningDelta(text) => {
                            crate::bus::global::GlobalEventBus::publish(AppEvent::ReasoningDelta {
                                session_id: Arc::clone(&session_id_arc),
                                delta: text.to_string(),
                            });
                        }
                        ChatEvent::ToolCall(tc) => {
                            crate::bus::global::GlobalEventBus::publish(
                                AppEvent::ToolCallStarted {
                                    session_id: self.session_id.clone(),
                                    tool_name: tc.name.to_string(),
                                    tool_id: tc.id.to_string(),
                                    arguments: tc.arguments.to_string(),
                                },
                            );
                        }
                        ChatEvent::Finish { stop_reason, .. } => {
                            crate::bus::global::GlobalEventBus::publish(AppEvent::AgentFinished {
                                session_id: self.session_id.clone(),
                                stop_reason: stop_reason.to_string(),
                            });
                        }
                        _ => {}
                    }
                    events.push(evt);
                }
                Err(e) => return Err(AppError::Provider(e)),
            }
        }

        Ok(events)
    }

    fn check_limits(&self) -> Option<String> {
        if let Some(agent) = self.agents.get(&self.state.current_agent) {
            if let Some(steps) = agent.steps {
                if self.state.turn_count >= steps {
                    return Some(format!("max steps ({}) reached", steps));
                }
            }
        }

        if self.state.turn_count >= self.limits.max_turns {
            return Some(format!("max turns ({}) reached", self.limits.max_turns));
        }

        if self.state.total_tokens >= self.limits.max_tokens {
            return Some(format!("max tokens ({}) reached", self.limits.max_tokens));
        }

        if self.state.start_time.elapsed() >= self.limits.timeout {
            return Some(format!("timeout ({:?}) reached", self.limits.timeout));
        }

        if self.steering.load(Ordering::SeqCst) {
            return Some("interrupted by user".to_string());
        }

        None
    }

    fn apply_agent_config(&self, request: &mut ChatRequest) {
        if let Some(agent) = self.agents.get(&self.state.current_agent) {
            if let Some(ref model) = agent.model {
                request.model = model.clone();
            }
            if let Some(temp) = agent.temperature {
                request.temperature = Some(temp);
            }
            if let Some(top_p) = agent.top_p {
                request.top_p = Some(top_p);
            }
        }
    }

    fn apply_auto_routing(&self, request: &mut ChatRequest) {
        if !self.model_router.is_enabled() {
            return;
        }

        let (prompt, tool_name) = self.extract_first_prompt_and_tool(request);
        if prompt.is_empty() {
            return;
        }

        let complexity = self.model_router.classify(&prompt, tool_name);
        if let Some(model) = self.model_router.route_model(complexity) {
            tracing::info!(
                "Auto-routing task to {} (complexity: {:?}, prompt: {:.50}...)",
                model,
                complexity,
                prompt
            );
            request.model = model;
        }
    }

    fn extract_first_prompt_and_tool(&self, request: &ChatRequest) -> (String, &'static str) {
        for msg in &request.messages {
            if let Message::User { content } = msg {
                for part in content {
                    if let crate::provider::ContentPart::Text { text } = part {
                        return (text.to_string(), "read");
                    }
                }
            }
        }
        (String::new(), "read")
    }

    async fn build_tool_definitions(&mut self) -> Vec<crate::provider::ToolDefinition> {
        let model = self
            .agents
            .get(&self.state.current_agent)
            .and_then(|a| a.model.as_ref());

        let lsp_enabled = self
            .config
            .experimental
            .as_ref()
            .and_then(|e| e.lsp_tool)
            .unwrap_or(false);

        let mcp_tools = if let Some(ref mcp_arc) = self.mcp_service {
            mcp_arc
                .try_read()
                .map(|mcp| mcp.list_tools())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mcp_tool_count = mcp_tools.len();

        let permission_version = self.permission_version();

        // Note: The tool definition cache uses mcp_tool_count as a proxy for MCP tool changes.
        // If MCP tool identities change without count changing (e.g., same number but different
        // tools), the cache may be stale. This is a known limitation - the MCP service would
        // need to expose a version/hash for more precise invalidation. Current behavior with
        // try_read() is intentional to avoid blocking the agent loop during MCP writes.

        if let Some((
            ref cache_model,
            cache_plan,
            cache_lsp,
            cache_mcp_count,
            cache_perm_ver,
            ref cached_defs,
        )) = self.tool_def_cache
        {
            if cache_model.as_ref().map(|s| s.as_str()) == model.map(|s| s.as_str())
                && cache_plan == self.state.plan_mode
                && cache_lsp == lsp_enabled
                && cache_mcp_count == mcp_tool_count
                && cache_perm_ver == permission_version
            {
                let mut definitions = cached_defs.clone();
                definitions.extend(mcp_tools.iter().cloned());

                if let Some(ref plugin_svc) = self.plugin_service {
                    let input = serde_json::json!({
                        "tools": definitions,
                        "model": model,
                    });
                    let hook_result = plugin_svc.dispatch_tool_definition(input).await;
                    if let Some(tools) = hook_result.output.get("tools").and_then(|v| v.as_array())
                    {
                        return tools
                            .iter()
                            .filter_map(|t| {
                                Some(crate::provider::ToolDefinition {
                                    name: t.get("name")?.as_str()?.to_string(),
                                    description: t.get("description")?.as_str()?.to_string(),
                                    parameters: t.get("parameters")?.clone(),
                                })
                            })
                            .collect();
                    }
                }

                return definitions;
            }
        }

        let tools = self.tool_registry.list();
        let flags = compute_model_flags(model);
        let filtered =
            filter_tools_for_model(model, &tools, self.state.plan_mode, lsp_enabled, &flags);
        let definitions: Vec<_> = filtered
            .iter()
            .map(|t| crate::provider::ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect();

        self.tool_def_cache = Some((
            model.map(|s| s.to_string()),
            self.state.plan_mode,
            lsp_enabled,
            mcp_tool_count,
            permission_version,
            definitions.clone(),
        ));

        let mut result = definitions;
        result.extend(mcp_tools);

        if let Some(ref plugin_svc) = self.plugin_service {
            let input = serde_json::json!({
                "tools": result,
                "model": model,
            });
            let hook_result = plugin_svc.dispatch_tool_definition(input).await;
            if let Some(tools) = hook_result.output.get("tools").and_then(|v| v.as_array()) {
                return tools
                    .iter()
                    .filter_map(|t| {
                        Some(crate::provider::ToolDefinition {
                            name: t.get("name")?.as_str()?.to_string(),
                            description: t.get("description")?.as_str()?.to_string(),
                            parameters: t.get("parameters")?.clone(),
                        })
                    })
                    .collect();
            }
        }

        result
    }

    async fn compact_if_needed(&mut self, messages: &mut Vec<Message>) {
        let auto = self
            .config
            .compaction
            .as_ref()
            .and_then(|c| c.auto)
            .unwrap_or(false);
        let prune = self
            .config
            .compaction
            .as_ref()
            .and_then(|c| c.prune)
            .unwrap_or(false);
        let reserved = self
            .config
            .compaction
            .as_ref()
            .and_then(|c| c.reserved)
            .unwrap_or(10_000);

        if detect_overflow(messages, self.context_tracker.context_limit(), reserved) {
            tracing::warn!("Context overflow detected, applying pruning");
            *messages = prune_tool_outputs(messages, 10_000);
            self.context_tracker.reset();
            self.context_tracker.add_messages(messages);
        }

        if self.context_tracker.needs_compaction() {
            let hook_result = if let Some(ref plugin_svc) = self.plugin_service {
                let compaction_input = serde_json::json!({
                    "messages": messages.iter().map(|m| {
                        match m {
                            Message::System { content } => serde_json::json!({"role": "system", "content": content}),
                            Message::User { content } => serde_json::json!({"role": "user", "content": content.iter().map(|p| match p {
                                ContentPart::Text { text } => serde_json::json!({"type": "text", "text": text}),
                                _ => serde_json::json!({"type": "unknown"}),
                            }).collect::<Vec<_>>()}),
                            Message::Assistant { content, tool_calls } => {
                                let mut json = serde_json::json!({
                                    "role": "assistant",
                                    "content": content.iter().map(|p| match p {
                                        ContentPart::Text { text } => serde_json::json!({"type": "text", "text": text}),
                                        _ => serde_json::json!({"type": "unknown"}),
                                    }).collect::<Vec<_>>()
                                });
                                if !tool_calls.is_empty() {
                                    json["tool_calls"] = serde_json::json!(tool_calls.iter().map(|tc| {
                                        serde_json::json!({
                                            "id": tc.id,
                                            "name": tc.name,
                                            "arguments": tc.arguments
                                        })
                                    }).collect::<Vec<_>>());
                                }
                                json
                            },
                            Message::Tool { tool_call_id, content } => serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_call_id,
                                "content": content
                            }),
                        }
                    }).collect::<Vec<_>>(),
                    "context_limit": self.context_tracker.context_limit(),
                    "current_tokens": self.context_tracker.current_tokens(),
                    "strategy": if auto { "auto_compact" } else { "drop_middle" },
                });
                let ctx = HookContext {
                    hook_type: HookType::SessionCompacting,
                    input: compaction_input,
                };
                plugin_svc.dispatch_hook(ctx).await
            } else {
                HookResult::ok(serde_json::Value::Null)
            };

            match hook_result {
                HookResult { blocked: true, .. } => {
                    tracing::info!("Compaction blocked by plugin");
                    return;
                }
                HookResult {
                    error: Some(err), ..
                } => {
                    tracing::warn!("Compaction hook error: {}", err);
                }
                _ => {}
            }

            if auto {
                let limit = self.context_tracker.context_limit();
                let threshold = self.context_tracker.threshold();
                let compacted = auto_compact_async(
                    messages,
                    limit,
                    threshold,
                    prune,
                    Some(self.provider.as_ref()),
                )
                .await;
                *messages = compacted;
            } else {
                *messages =
                    compact_messages_sync(messages.clone(), CompactionStrategy::DropMiddleMessages);
            }

            self.context_tracker.reset();
            self.context_tracker.add_messages(messages);
        }
    }

    #[instrument(skip(self, request), fields(session_id = %self.session_id, turn_count = self.state.turn_count))]
    pub async fn run(&mut self, mut request: ChatRequest) -> Result<Vec<ChatEvent>, AppError> {
        let session_start_ctx = crate::hooks::HookContext {
            event: crate::hooks::HookEvent::SessionStart,
            session_id: Some(self.session_id.clone()),
            tool_name: None,
            tool_arguments: None,
            tool_result: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        };
        if let Some(ref hr) = self.hook_registry {
            for err in hr.run_hooks(crate::hooks::HookEvent::SessionStart, &session_start_ctx).await {
                tracing::error!("SessionStart hook error: {}", err);
            }
        }

        self.apply_auto_routing(&mut request);
        self.apply_agent_config(&mut request);
        if let Some(system) = request.system.take() {
            request.messages.insert(
                0,
                Message::System {
                    content: system.into(),
                },
            );
        }
        request.tools = Some(self.build_tool_definitions().await);
        self.context_tracker.add_messages(&request.messages);

        let mut all_events = Vec::with_capacity(128);
        let mut processor = EventProcessor::new();

        loop {
            if let Some(reason) = self.check_limits() {
                tracing::info!("Agent loop stopping: {}", reason);
                break;
            }

            if let Some(agent) = self.agents.get(&self.state.current_agent) {
                if let Some(steps) = agent.steps {
                    if self.state.turn_count + 1 >= steps {
                        tracing::info!(
                            "Max steps ({}) reached on next turn, injecting termination message",
                            steps
                        );
                        let system = format!(
                            "CRITICAL - MAXIMUM STEPS REACHED\n\nYou have reached the maximum number of steps ({}). Provide a summary of your work and exit.",
                            steps
                        );
                        request.messages.push(Message::System {
                            content: system.into(),
                        });
                        request.messages.push(Message::Assistant {
                            content: vec![ContentPart::Text {
                                text: "Here is a summary of my work so far:".to_string().into(),
                            }],
                            tool_calls: vec![],
                        });
                        request.tools = None;
                    }
                }
            }

            self.state.turn_count += 1;
            tracing::debug!("Agent turn {}", self.state.turn_count);

            let agent_start_ctx = crate::hooks::HookContext {
                event: crate::hooks::HookEvent::AgentStart,
                session_id: Some(self.session_id.clone()),
                tool_name: None,
                tool_arguments: None,
                tool_result: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            };
            if let Some(ref hr) = self.hook_registry {
                for err in hr.run_hooks(crate::hooks::HookEvent::AgentStart, &agent_start_ctx).await {
                    tracing::error!("AgentStart hook error: {}", err);
                }
            }

            self.compact_if_needed(&mut request.messages).await;

            let events = match self.stream_with_retry(&request).await {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    return Err(e);
                }
            };

            for event in &events {
                processor.process(event.clone());
            }
            all_events.extend(events);

            if !processor.has_tool_calls() {
                break;
            }

            let tool_calls = processor.tool_calls().to_vec();
            let tool_results = self.execute_tool_calls(&tool_calls).await?;

            if let Some(msg) = processor.to_assistant_message() {
                self.context_tracker.add_message(&msg);
                request.messages.push(msg);
            }

            for (id, content) in &tool_results {
                let tool_name = tool_calls
                    .iter()
                    .find(|tc| *tc.id == id.as_str())
                    .map(|tc| tc.name.to_string())
                    .unwrap_or_default();
                let success = !content.starts_with("Error:");
                crate::bus::global::GlobalEventBus::publish(AppEvent::ToolResult {
                    tool_id: id.clone(),
                    tool_name,
                    session_id: self.session_id.clone(),
                    output: content.clone(),
                    success,
                });
            }

            for (id, content) in &tool_results {
                if let Some(change) = detect_plan_mode_change(content) {
                    match change {
                        crate::tool::plan::PlanModeChange::Enter(topic) => {
                            self.enter_plan_mode(topic);
                            tracing::info!("Plan mode entered");
                        }
                        crate::tool::plan::PlanModeChange::Exit => {
                            self.exit_plan_mode();
                            tracing::info!("Plan mode exited");
                        }
                    }
                }

                let redacted_content = redact_local_paths(content);
                let msg = Message::Tool {
                    tool_call_id: id.clone().into(),
                    content: redacted_content.into(),
                };
                self.context_tracker.add_message(&msg);
                request.messages.push(msg);
            }

            processor.reset();

            let agent_end_ctx = crate::hooks::HookContext {
                event: crate::hooks::HookEvent::AgentEnd,
                session_id: Some(self.session_id.clone()),
                tool_name: None,
                tool_arguments: None,
                tool_result: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            };
            if let Some(ref hr) = self.hook_registry {
                for err in hr.run_hooks(crate::hooks::HookEvent::AgentEnd, &agent_end_ctx).await {
                    tracing::error!("AgentEnd hook error: {}", err);
                }
            }
        }

        self.drain_follow_up(&mut request, &mut all_events, &mut processor)
            .await;

        let session_end_ctx = crate::hooks::HookContext {
            event: crate::hooks::HookEvent::SessionEnd,
            session_id: Some(self.session_id.clone()),
            tool_name: None,
            tool_arguments: None,
            tool_result: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        };
        if let Some(ref hr) = self.hook_registry {
            for err in hr.run_hooks(crate::hooks::HookEvent::SessionEnd, &session_end_ctx).await {
                tracing::error!("SessionEnd hook error: {}", err);
            }
        }

        Ok(all_events)
    }

    /// Capture a snapshot of the project state if snapshot_manager is configured
    async fn capture_snapshot_if_needed(&mut self) {
        if let Some(ref mut snapshot_manager) = self.snapshot_manager {
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

    #[instrument(skip(self, tool_calls), fields(tool_count = tool_calls.len()))]
    async fn execute_tool_calls(
        &mut self,
        tool_calls: &[ToolCall],
    ) -> Result<Vec<(String, String)>, AppError> {
        let mut tool_results = Vec::with_capacity(16);
        let mut has_pending_question = false;

        let mut allowed_tools = Vec::with_capacity(tool_calls.len());
        for (idx, tc) in tool_calls.iter().enumerate() {
            match self.check_tool_permission(tc).await {
                ToolPermissionOutcome::QuestionTool => {
                    has_pending_question = true;
                    tool_results.push((idx, tc.id.to_string(), "__QUESTION_PENDING__".to_string()));
                }
                ToolPermissionOutcome::Allowed(tc) => {
                    allowed_tools.push((idx, tc));
                }
                ToolPermissionOutcome::Denied { tool_id, message } => {
                    tool_results.push((idx, tool_id, message));
                }
            }
        }

        // Capture snapshot before executing file-modifying tools
        let has_file_modifying = allowed_tools.iter().any(|(_, tc)| is_file_modifying_tool(&tc.name));
        if has_file_modifying {
            self.capture_snapshot_if_needed().await;
        }

        let _timeout_secs = self.tool_timeout();
        let max_parallel = self.max_parallel_tools();
        const MAX_PARALLEL_DEFAULT: usize = 100;
        let effective_max = if max_parallel == usize::MAX {
            MAX_PARALLEL_DEFAULT
        } else {
            max_parallel
        };
        let regular_tool_count = allowed_tools.len();
        let registry = &self.tool_registry;

        let mut mcp_tool_calls = Vec::with_capacity(4);
        let regular_tools: Vec<_> = allowed_tools
            .into_iter()
            .filter(|(idx, tc)| {
                if tc.name.starts_with("mcp__") {
                    mcp_tool_calls.push((*idx, tc.clone()));
                    false
                } else {
                    true
                }
            })
            .collect();

        for (orig_idx, tc) in mcp_tool_calls {
            let name = tc.name.clone();
            if let Some((server, tool)) = parse_mcp_tool_name(&name) {
                if let Some(ref mcp_arc) = self.mcp_service {
                    if let Ok(mcp) = mcp_arc.try_read() {
                        match mcp.call_tool(server, tool, tc.arguments.clone()).await {
                            Ok(result) => {
                                tool_results.push((orig_idx, tc.id.to_string(), result));
                            }
                            Err(e) => {
                                tool_results.push((
                                    orig_idx,
                                    tc.id.to_string(),
                                    format!("Error: {}", e),
                                ));
                            }
                        }
                    } else {
                        tracing::debug!(server = %server, tool = %tool, "MCP service temporarily unavailable (write locked)");
                        tool_results.push((
                            orig_idx,
                            tc.id.to_string(),
                            "Error: MCP service locked, please retry".to_string(),
                        ));
                    }
                } else {
                    tool_results.push((
                        orig_idx,
                        tc.id.to_string(),
                        "Error: MCP service not available".to_string(),
                    ));
                }
            } else {
                tool_results.push((
                    orig_idx,
                    tc.id.to_string(),
                    format!("Error: Invalid MCP tool name '{}'", name),
                ));
            }
        }

        let mut results = Vec::with_capacity(regular_tool_count);
        let sem = Arc::new(tokio::sync::Semaphore::new(effective_max));
        let mut futures = Vec::with_capacity(regular_tool_count);
        let hook_registry = self.hook_registry.as_ref().map(Arc::clone);
        for (orig_idx, tc) in regular_tools {
            let tc_arc = Arc::new(tc);
            let sem = Arc::clone(&sem);
            let id = tc_arc.id.clone();
            let tool_name = tc_arc.name.clone();
            let timeout = self.get_tool_timeout(&tool_name);
            let hook_registry = hook_registry.clone();
            let session_id = self.session_id.clone();
            let idx_for_results = orig_idx;
            futures.push(async move {
                let permit = sem
                    .acquire()
                    .await
                    .expect("failed to acquire semaphore permit for tool execution");

                let pre_ctx = crate::hooks::HookContext {
                    event: crate::hooks::HookEvent::PreToolExecute,
                    session_id: Some(session_id.clone()),
                    tool_name: Some(tool_name.to_string()),
                    tool_arguments: Some(tc_arc.arguments.clone()),
                    tool_result: None,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64,
                };
                if let Some(ref hr) = hook_registry {
                    for err in hr
                        .run_hooks(crate::hooks::HookEvent::PreToolExecute, &pre_ctx)
                        .await
                    {
                        tracing::error!("Pre-tool hook error: {}", err);
                    }
                }

                let result = {
                    let tc_inner = Arc::clone(&tc_arc);
                    let tool = registry
                        .get(&tc_inner.name)
                        .ok_or_else(|| ToolError::NotFound(tc_inner.name.to_string()));
                    match tool {
                        Ok(t) => {
                            match tokio::time::timeout(
                                timeout,
                                t.execute(tc_inner.arguments.clone()),
                            )
                            .await
                            {
                                Ok(r) => r,
                                Err(_) => Err(ToolError::Execution(format!(
                                    "Tool '{}' timed out after {:?}",
                                    tc_inner.name, timeout
                                ))),
                            }
                        }
                        Err(e) => Err(e),
                    }
                };

                let post_ctx = crate::hooks::HookContext {
                    event: crate::hooks::HookEvent::PostToolExecute,
                    session_id: Some(session_id),
                    tool_name: Some(tool_name.to_string()),
                    tool_arguments: Some(tc_arc.arguments.clone()),
                    tool_result: result.as_ref().ok().cloned(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64,
                };
                if let Some(ref hr) = hook_registry {
                    for err in hr
                        .run_hooks(crate::hooks::HookEvent::PostToolExecute, &post_ctx)
                        .await
                    {
                        tracing::error!("Post-tool hook error: {}", err);
                    }
                }

                drop(permit);
                (idx_for_results, id, result)
            });
        }
        let all_results = futures::future::join_all(futures).await;
        results.extend(all_results);
        for (idx, id, result) in results {
            let output = match result {
                Ok(output) => output,
                Err(e) => format!("Error: {}", e),
            };
            tool_results.push((idx, id.to_string(), output));
        }

        if has_pending_question {
            if let Some(rx) = self.question_rx.take() {
                match rx.await {
                    Ok(answers) => {
                        let formatted = format_question_answers(&answers);
                        tool_results = tool_results
                            .into_iter()
                            .map(|(idx, id, output)| {
                                if output == "__QUESTION_PENDING__" {
                                    (idx, id, formatted.clone())
                                } else {
                                    (idx, id, output)
                                }
                            })
                            .collect();
                    }
                    Err(_) => {
                        tool_results = tool_results
                            .into_iter()
                            .map(|(idx, id, output)| {
                                if output == "__QUESTION_PENDING__" {
                                    (idx, id, "[question cancelled by user]".to_string())
                                } else {
                                    (idx, id, output)
                                }
                            })
                            .collect();
                    }
                }
                QuestionRegistry::unregister(&self.session_id);
            }
        }

        tool_results.sort_by_key(|(idx, _, _)| *idx);
        let ordered_results: Vec<(String, String)> = tool_results
            .into_iter()
            .map(|(_, id, output)| (id, output))
            .collect();

        Ok(ordered_results)
    }

    /// Drains a single queued follow-up prompt, if one exists.
    ///
    /// Uses non-blocking `try_recv()` - does NOT wait if no follow-up is queued.
    /// This means late-arriving follow-ups (after `run()` returns) are NOT processed
    /// by the same `run()` call; they require a new `run()` invocation.
    async fn drain_follow_up(
        &mut self,
        request: &mut ChatRequest,
        all_events: &mut Vec<ChatEvent>,
        processor: &mut EventProcessor,
    ) {
        // Check if a follow-up is already queued without blocking
        let prompt = match self.follow_up_rx.try_recv() {
            Ok(prompt) => {
                tracing::info!("Processing follow-up: {}", prompt);
                prompt
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                // No follow-up queued, return immediately without blocking
                tracing::debug!("No follow-up queued, skipping drain");
                return;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                tracing::info!("Follow-up channel disconnected");
                return;
            }
        };

        request.messages.push(Message::User {
            content: vec![ContentPart::Text {
                text: prompt.into(),
            }],
        });

        // Continue processing until done (handles tool calls and follow-up responses)
        loop {
            self.compact_if_needed(&mut request.messages).await;

            let events = match self.stream_with_retry(request).await {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!("Follow-up stream error: {}", e);
                    return;
                }
            };

            for event in &events {
                processor.process(event.clone());
            }
            all_events.extend(events);

            if !processor.has_tool_calls() {
                processor.reset();
                break;
            }

            let tool_calls = processor.tool_calls().to_vec();
            let tool_results = match self.execute_tool_calls(&tool_calls).await {
                Ok(results) => results,
                Err(e) => {
                    tracing::error!("Tool execution error: {}", e);
                    processor.reset();
                    return;
                }
            };

            // Push assistant message BEFORE tool results (fix Packet 2)
            if let Some(msg) = processor.to_assistant_message() {
                request.messages.push(msg);
            }

            for (id, content) in &tool_results {
                let tool_name = tool_calls
                    .iter()
                    .find(|tc| *tc.id == id.as_str())
                    .map(|tc| tc.name.to_string())
                    .unwrap_or_default();
                let success = !content.starts_with("Error:");
                crate::bus::global::GlobalEventBus::publish(AppEvent::ToolResult {
                    tool_id: id.clone(),
                    tool_name,
                    session_id: self.session_id.clone(),
                    output: content.clone(),
                    success,
                });
            }

            for (id, content) in &tool_results {
                if let Some(change) = detect_plan_mode_change(content) {
                    match change {
                        crate::tool::plan::PlanModeChange::Enter(topic) => {
                            self.enter_plan_mode(topic);
                            tracing::info!("Plan mode entered");
                        }
                        crate::tool::plan::PlanModeChange::Exit => {
                            self.exit_plan_mode();
                            tracing::info!("Plan mode exited");
                        }
                    }
                }

                let redacted_content = redact_local_paths(content);
                let msg = Message::Tool {
                    tool_call_id: id.clone().into(),
                    content: redacted_content.into(),
                };
                request.messages.push(msg);
            }

            processor.reset();
        }
    }

    pub async fn run_with_prompt(
        &mut self,
        system: Option<String>,
        prompt: String,
    ) -> Result<Vec<ChatEvent>, AppError> {
        let mut messages = Vec::new();

        if let Some(sys) = system {
            messages.push(Message::System {
                content: sys.into(),
            });
        }

        messages.push(Message::User {
            content: vec![ContentPart::Text {
                text: prompt.into(),
            }],
        });

        let request = ChatRequest {
            messages,
            model: String::new(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
        };

        self.run(request).await
    }
}

/// Filters tools based on model capabilities and plan mode.
///
/// In plan mode, only safe read-only tools are allowed to prevent the agent from
/// modifying files while planning. The allowed tools are: read, glob, grep, list,
/// codesearch, webfetch, lsp, skill, and plan_exit.
///
/// For regular mode:
/// - apply_patch is restricted to non-OSS models (GPT but not GPT-4) to prevent
///   patching models that don't support it
/// - edit and write are blocked for GPT models since GPT-4 has better write handling
/// - codesearch and websearch require EXA_API_KEY or EXA_CODE_API_KEY
/// - lsp requires lsp_enabled flag
/// - batch is always disabled
fn filter_tools_for_model<'a>(
    _model: Option<&String>,
    tools: &[&'a dyn crate::tool::Tool],
    plan_mode: bool,
    lsp_enabled: bool,
    flags: &ModelFlags,
) -> Vec<&'a dyn crate::tool::Tool> {
    let plan_allowed_tools = [
        "read",
        "glob",
        "grep",
        "list",
        "codesearch",
        "webfetch",
        "lsp",
        "skill",
        "plan_exit",
    ];

    tools
        .iter()
        .filter(|t| {
            if plan_mode {
                return plan_allowed_tools.contains(&t.name());
            }

            match t.name() {
                "apply_patch" => flags.is_gpt && flags.is_non_oss,
                "edit" | "write" => flags.needs_exa,
                "codesearch" | "websearch" => flags.exa_available,
                "lsp" => lsp_enabled,
                "batch" => false,
                _ => true,
            }
        })
        .copied()
        .collect()
}

fn compute_model_flags(model: Option<&String>) -> ModelFlags {
    let model_id = model.map(|s| s.to_lowercase()).unwrap_or_default();
    let is_gpt = model_id.contains("gpt") && !model_id.contains("gpt-4");
    let is_non_oss =
        model_id.contains("gpt") || model_id.contains("claude") || model_id.contains("gemini");
    let needs_exa = model_id.contains("claude")
        || model_id.contains("gpt")
        || model_id.contains("gemini")
        || model_id.contains("llm");
    let exa_available =
        std::env::var("EXA_API_KEY").is_ok() || std::env::var("EXA_CODE_API_KEY").is_ok();
    ModelFlags {
        is_gpt,
        is_non_oss,
        needs_exa,
        exa_available,
    }
}
