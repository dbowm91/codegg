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
    auto_compact_async, compact_messages_sync, compact_with_policy, detect_overflow,
    prune_tool_outputs, CompactionInput, CompactionStrategy, ContextTracker,
    ResolvedCompactionConfig,
};
use crate::agent::processor::EventProcessor;
use crate::agent::router::ModelRouter;
use crate::agent::worker::SubAgentRequest;
use crate::agent::Agent;
use crate::bus::events::AppEvent;
use crate::bus::{PermissionDecision, PermissionRegistry, QuestionRegistry};
use crate::config::schema::Config;
use crate::error::{AgentError, AppError, ProviderError, ToolError};
use crate::model_profile::policy::push_control_instruction;
use crate::permission::{DoomLoopDetector, PermissionChecker, PermissionResult};
use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::provider::text_tool_parser::parse_text_as_tool_calls;
use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message, ToolCall};
use crate::tool::plan::detect_plan_mode_change;
use crate::tool::question::{format_question_answers, parse_question_questions};
use crate::tool::risk::{classify_tool_risk, summarize_tool_output};
use crate::tool::ToolRegistry;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

static PATH_REDACTION_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    let patterns = [
        r"/home/[^\s/]+",
        r"/Users/[^\s/]+",
        r"/var/[^\s/]+",
        r"/tmp/[^\s/]+",
        r"C:\\Users\\[^\s\\]+",
        r"C:\\Program Files\\[^\s\\]+",
        r"C:\\Windows\\[^\s\\]+",
    ];
    patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
});
use tokio::sync::broadcast::error::TryRecvError;
use tokio::sync::mpsc;
use tracing::instrument;

type ToolDefCache = (
    Option<String>,
    bool,
    bool,
    usize,
    u64,
    bool,
    Vec<crate::provider::ToolDefinition>,
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

fn harden_history(messages: &mut Vec<Message>) {
    let mut hardened: Vec<Message> = Vec::with_capacity(messages.len() + 8);
    let mut pending_tool_calls: BTreeMap<String, String> = BTreeMap::new();

    let flush_pending = |target: &mut Vec<Message>, pending: &mut BTreeMap<String, String>| {
        if pending.is_empty() {
            return;
        }
        for tool_call_id in pending.keys() {
            target.push(Message::Tool {
                tool_call_id: tool_call_id.clone().into(),
                content: "[tool result missing due to history repair]"
                    .to_string()
                    .into(),
            });
        }
        pending.clear();
    };

    for msg in messages.drain(..) {
        match msg {
            Message::Assistant {
                content,
                tool_calls,
            } => {
                flush_pending(&mut hardened, &mut pending_tool_calls);
                for tc in &tool_calls {
                    pending_tool_calls.insert(tc.id.to_string(), tc.name.to_string());
                }
                hardened.push(Message::Assistant {
                    content,
                    tool_calls,
                });
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                if pending_tool_calls.remove(tool_call_id.as_ref()).is_some() {
                    hardened.push(Message::Tool {
                        tool_call_id,
                        content,
                    });
                } else {
                    tracing::warn!(
                        tool_call_id = %tool_call_id,
                        "Orphan tool message during history hardening - preserving to avoid breaking message contract"
                    );
                    hardened.push(Message::Tool {
                        tool_call_id,
                        content,
                    });
                }
            }
            Message::User { content } => {
                flush_pending(&mut hardened, &mut pending_tool_calls);
                hardened.push(Message::User { content });
            }
            Message::System { content } => {
                flush_pending(&mut hardened, &mut pending_tool_calls);
                hardened.push(Message::System { content });
            }
        }
    }

    if !pending_tool_calls.is_empty() {
        for tool_call_id in pending_tool_calls.keys() {
            hardened.push(Message::Tool {
                tool_call_id: tool_call_id.clone().into(),
                content: "[tool result missing due to history repair]"
                    .to_string()
                    .into(),
            });
        }
    }

    *messages = hardened;
}

fn indicates_more_work(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("let me")
        || t.contains("i'll")
        || t.contains("i will")
        || t.contains("next,")
        || t.contains("next step")
        || t.contains("now i")
}

fn is_soft_stop_reason(stop_reason: Option<&str>) -> bool {
    matches!(stop_reason, Some("stop" | "end_turn"))
}

fn is_repo_task_prompt(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    p.contains("review")
        || p.contains("docs")
        || p.contains("read")
        || p.contains("file")
        || p.contains("project")
        || p.contains("repository")
        || p.contains("codebase")
        || p.contains("source")
        || p.contains("structure")
        || p.contains("symbols")
        || p.contains("architecture")
        || p.contains("outline")
}

#[derive(Copy, Clone)]
struct ModelFlags {
    is_gpt: bool,
    is_non_oss: bool,
    /// True if at least one search provider (key-based or no-key) is
    /// configured. Used as the gate for `websearch` (and `codesearch`).
    search_provider_available: bool,
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

    /// Build the `ToolExecutionContext` passed alongside every
    /// native tool call dispatched via
    /// `ToolRegistry::execute_capture()`.
    ///
    /// Centralising this helper keeps the structured-execution
    /// envelope consistent across all native dispatch sites and
    /// makes it trivial to enrich the context (e.g. resolve the
    /// real `backend` for `websearch`/`webfetch`) in a single
    /// place.
    fn build_tool_execution_context(
        &self,
        tc: &ToolCall,
        timeout_ms: Option<u64>,
    ) -> crate::tool::backend::ToolExecutionContext {
        let backend = self.resolve_native_backend(&tc.name);
        crate::tool::backend::ToolExecutionContext {
            backend,
            session_id: Some(self.session_id.clone()),
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            permission_mode: None,
            timeout_ms,
        }
    }

    /// Resolve the `ToolBackendKind` for a native tool name.
    ///
    /// The vast majority of codegg tools are in-process `Native`
    /// wrappers; the live exception is `websearch`/`webfetch` which
    /// may be backed by the external `eggsearch` MCP server. For
    /// those, we read the resolved `SearchConfig` so the
    /// `ToolExecutionContext::backend` field reflects the real
    /// backend the wrapper will eventually delegate to.
    fn resolve_native_backend(&self, tool_name: &str) -> crate::tool::backend::ToolBackendKind {
        use crate::config::schema::SearchBackendConfig;
        use crate::tool::backend::ToolBackendKind;
        if matches!(tool_name, "websearch" | "webfetch") {
            match crate::search_backend::state::search_config().backend() {
                SearchBackendConfig::Eggsearch => ToolBackendKind::Mcp,
                SearchBackendConfig::Builtin | SearchBackendConfig::Disabled => {
                    ToolBackendKind::BuiltinLegacy
                }
            }
        } else {
            ToolBackendKind::Native
        }
    }
}

fn extract_path_from_tool_call(tc: &ToolCall) -> Option<String> {
    let args = &tc.arguments;
    match tc.name.as_str() {
        "read" | "write" | "edit" | "glob" | "grep" | "list" => {
            args.get("path")?.as_str().map(String::from)
        }
        "apply_patch" => args.get("path")?.as_str().map(String::from),
        _ => None,
    }
}

fn extract_bash_command(tc: &ToolCall) -> Option<String> {
    if &*tc.name != "bash" {
        return None;
    }
    tc.arguments.get("command")?.as_str().map(String::from)
}

fn is_test_command(command: &str) -> bool {
    let cmd = command.trim();
    let test_patterns = [
        "cargo test",
        "cargo nextest",
        "npm test",
        "pnpm test",
        "yarn test",
        "pytest",
        "uv run pytest",
        "go test",
        "zig build test",
        "make test",
        "make check",
        "bun test",
    ];
    for pattern in &test_patterns {
        if cmd.starts_with(pattern) {
            return true;
        }
    }
    false
}

fn extract_git_subcommand(tc: &ToolCall) -> Option<String> {
    if &*tc.name != "git" {
        return None;
    }
    tc.arguments.get("subcommand")?.as_str().map(String::from)
}

fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    let delimiter_pos = rest.rfind("__")?;
    let server = &rest[..delimiter_pos];
    let tool = &rest[delimiter_pos + 2..];
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}

fn is_mcp_tool(tool_name: &str) -> bool {
    tool_name.starts_with("mcp__")
}

fn is_workspace_file_mutation(tool_name: &str, path: Option<&str>) -> bool {
    path.is_some() && is_file_modifying_tool(tool_name) && is_path_within_working_directory(path)
}

fn tool_result_is_success(output: &str) -> bool {
    !output.starts_with("Error: ")
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
        if p.is_absolute() {
            p
        } else {
            cwd.join(p)
        }
    };

    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let Some(parent) = candidate.parent() else {
                return false;
            };
            match parent.canonicalize() {
                Ok(parent) => parent,
                Err(_) => return false,
            }
        }
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
                let question_id = format!("q-{}", uuid::Uuid::new_v4());
                let (tx, rx) = tokio::sync::oneshot::channel();
                QuestionRegistry::register_with_session(
                    self.session_id.clone(),
                    None,
                    question_id.clone(),
                    tx,
                );
                crate::bus::global::GlobalEventBus::publish(AppEvent::QuestionPending {
                    session_id: self.session_id.clone(),
                    question_id,
                    turn_id: None,
                    questions: questions_json,
                });
                self.question_rx = Some(rx);
                return ToolPermissionOutcome::QuestionTool;
            }
        }

        self.doom_detector.record_tool_call(&tc.name, &tc.arguments);
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
        let security_hint = if !self.security_service.enabled() {
            crate::security::policy::SecurityDecisionHint {
                action: crate::security::policy::SecurityAction::Observe,
                reason: String::new(),
                finding: None,
            }
        } else if let Some(ref cmd) = bash_command {
            self.security_service.classify_bash(cmd)
        } else if let Some(ref subcommand) = git_subcommand {
            self.security_service.classify_git(subcommand)
        } else {
            self.security_service
                .classify_tool_call(&tc.name, &tc.arguments)
        };
        if let Some(ref finding) = security_hint.finding {
            self.recent_findings.push(finding.clone());
        }
        // Check if the path targets a sensitive file, regardless of permission level
        let sensitive_match = self.config.security.as_ref().and_then(|sec| {
            crate::security::matches_sensitive_path(path.as_deref(), &sec.sensitive_paths)
        });

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
                } else if let Some(sensitive) = sensitive_match {
                    // Escalate: sensitive paths always require user confirmation
                    let reason = sensitive
                        .reason
                        .clone()
                        .unwrap_or_else(|| "sensitive path".to_string());
                    let perm_id = format!("{}-{}", tc.id, tc.name);
                    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                    PermissionRegistry::register_with_session(
                        self.session_id.clone(),
                        None,
                        perm_id.clone(),
                        resp_tx,
                    );
                    let args = serde_json::json!({
                        "command": bash_command.as_deref().unwrap_or(""),
                        "security": {
                            "action": "ask",
                            "reason": format!("Sensitive path access: {}", reason),
                            "review_level": sensitive.review_level.as_deref().unwrap_or("standard"),
                        }
                    });
                    crate::bus::global::GlobalEventBus::publish(AppEvent::PermissionPending {
                        session_id: self.session_id.clone(),
                        perm_id: perm_id.clone(),
                        turn_id: None,
                        tool: (*tc.name).clone(),
                        path: path.clone(),
                        args: Some(args),
                    });
                    let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await
                    {
                        Ok(Ok(choice)) => choice,
                        _ => PermissionDecision::DenyOnce,
                    };
                    PermissionRegistry::unregister(&perm_id);
                    if choice.allowed() {
                        ToolPermissionOutcome::Allowed(tc.clone())
                    } else {
                        ToolPermissionOutcome::Denied {
                            tool_id: tc.id.to_string(),
                            message: format!(
                                "Tool '{}' denied: access to sensitive path refused",
                                tc.name
                            ),
                        }
                    }
                } else if matches!(
                    security_hint.action,
                    crate::security::policy::SecurityAction::Deny
                ) {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!(
                            "Tool '{}' denied by security policy: {}",
                            tc.name, security_hint.reason
                        ),
                    }
                } else if matches!(
                    security_hint.action,
                    crate::security::policy::SecurityAction::Ask
                ) {
                    let perm_id = format!("{}-{}", tc.id, tc.name);
                    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                    PermissionRegistry::register_with_session(
                        self.session_id.clone(),
                        None,
                        perm_id.clone(),
                        resp_tx,
                    );
                    let args = serde_json::json!({
                        "command": bash_command.as_deref().unwrap_or(""),
                        "security": {
                            "action": "ask",
                            "reason": security_hint.reason,
                            "category": security_hint.finding.as_ref().map(|f| format!("{:?}", f.category)).unwrap_or_default(),
                        }
                    });
                    crate::bus::global::GlobalEventBus::publish(AppEvent::PermissionPending {
                        session_id: self.session_id.clone(),
                        perm_id: perm_id.clone(),
                        turn_id: None,
                        tool: (*tc.name).clone(),
                        path: path.clone(),
                        args: Some(args),
                    });
                    let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await
                    {
                        Ok(Ok(choice)) => choice,
                        _ => PermissionDecision::DenyOnce,
                    };
                    PermissionRegistry::unregister(&perm_id);
                    if choice.allowed() {
                        ToolPermissionOutcome::Allowed(tc.clone())
                    } else {
                        ToolPermissionOutcome::Denied {
                            tool_id: tc.id.to_string(),
                            message: format!(
                                "Tool '{}' denied by user (security escalation)",
                                tc.name
                            ),
                        }
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
                // Auto-accept MCP tools (mcp__*) and local file mutations
                // when the target is within the working directory and not
                // sensitive. Read-only and safe-mutating tools are handled by
                // `PermissionChecker::check()` short-circuiting to Allow.
                if (is_mcp_tool(tc.name.as_str())
                    || is_workspace_file_mutation(tc.name.as_str(), req.path.as_deref()))
                    && is_path_within_working_directory(req.path.as_deref())
                    && sensitive_match.is_none()
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
                PermissionRegistry::register_with_session(
                    self.session_id.clone(),
                    None,
                    perm_id.clone(),
                    resp_tx,
                );
                crate::bus::global::GlobalEventBus::publish(AppEvent::PermissionPending {
                    session_id: self.session_id.clone(),
                    perm_id: perm_id.clone(),
                    turn_id: None,
                    tool: req.tool.clone(),
                    path: req.path.clone(),
                    args: req.args.clone(),
                });
                let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
                    Ok(Ok(choice)) => choice,
                    _ => PermissionDecision::DenyOnce,
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
    pub tool_call_count: usize,
    /// Last turn's input tokens (cumulative per-call reported by the
    /// provider). Used to compute per-turn deltas for goal accounting.
    pub last_turn_input_tokens: i64,
    /// Last turn's output tokens (cumulative per-call reported by the
    /// provider).
    pub last_turn_output_tokens: i64,
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
    deferred_tool_definitions: Vec<crate::provider::ToolDefinition>,
    model_router: ModelRouter,
    snapshot_manager: Option<crate::snapshot::SnapshotManager>,
    file_change_rx: tokio::sync::broadcast::Receiver<AppEvent>,
    usage_store: Option<Arc<crate::session::UsageStore>>,
    #[allow(dead_code)]
    pricing_service: crate::util::pricing::PricingService,
    security_service: crate::security::service::SecurityService,
    recent_findings: Vec<crate::security::finding::SecurityFinding>,
    todo_state: std::sync::Arc<tokio::sync::Mutex<crate::task_state::TodoState>>,
    task_state_policy: crate::model_profile::types::TaskStatePolicy,
    todo_pool: Option<sqlx::SqlitePool>,
    event_store: Option<Arc<crate::session::EventStore>>,
    #[allow(dead_code)]
    active_tool_timings: HashMap<String, Instant>,
    execution_policy: Option<crate::agent::policy::ExecutionPolicy>,
    original_user_prompt: Option<String>,
    subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    max_tool_calls: Option<usize>,
    goal_store: Option<Arc<crate::goal::GoalStore>>,
    goal_wall_clock: std::sync::Mutex<crate::goal::runtime::GoalWallClock>,
    cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
    steer_rx: Option<mpsc::UnboundedReceiver<String>>,
    pending_steer: Option<String>,
    context_ledger: crate::agent::context_frame::ContextLedgerState,
    artifact_store: Arc<dyn crate::context::ContextArtifactStore>,
    projection_config: crate::context::ProjectionConfig,
}

impl AgentLoop {
    /// Apply tool exposure filtering based on execution policy's initial_tool_mode.
    fn apply_tool_exposure_filter(
        &self,
        definitions: Vec<crate::provider::ToolDefinition>,
    ) -> Vec<crate::provider::ToolDefinition> {
        let Some(ref policy) = self.execution_policy else {
            return definitions;
        };

        // First apply exposure mode filter
        let filtered = match policy.initial_tool_mode {
            crate::agent::policy::ToolExposureMode::Full => definitions,
            crate::agent::policy::ToolExposureMode::Curated => {
                let core_tools = [
                    "read",
                    "list",
                    "grep",
                    "glob",
                    "codesearch",
                    "edit",
                    "apply_patch",
                    "bash",
                    "git",
                    "diff",
                    "todoread",
                    "todowrite",
                    "question",
                    "tool_search",
                    "skill",
                    "websearch",
                ];
                definitions
                    .into_iter()
                    .filter(|t| core_tools.contains(&t.name.as_str()))
                    .collect()
            }
            crate::agent::policy::ToolExposureMode::MinimalWithDiscovery => {
                let minimal_tools = [
                    "read",
                    "list",
                    "grep",
                    "codesearch",
                    "edit",
                    "apply_patch",
                    "bash",
                    "question",
                    "todowrite",
                    "todoread",
                    "tool_search",
                    "websearch",
                ];
                definitions
                    .into_iter()
                    .filter(|t| minimal_tools.contains(&t.name.as_str()))
                    .collect()
            }
        };

        // Then apply model profile disabled_tools filter
        if let Some(ref disabled) = policy.disabled_tools {
            if !disabled.is_empty() {
                return filtered
                    .into_iter()
                    .filter(|t| !disabled.contains(&t.name))
                    .collect();
            }
        }

        filtered
    }

    pub fn new(
        agents: Vec<Agent>,
        provider: Box<dyn crate::provider::Provider>,
        permission_checker: PermissionChecker,
        tool_registry: ToolRegistry,
        config: Config,
        mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
        pool: Option<sqlx::SqlitePool>,
        artifact_store: Arc<dyn crate::context::ContextArtifactStore>,
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
            if let Some(pool) = pool.clone() {
                let project_root =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                let options = config
                    .snapshot_config
                    .as_ref()
                    .map(|c| crate::snapshot::SnapshotOptions {
                        max_files: c.max_files,
                        max_file_bytes: c.max_file_bytes,
                        max_total_bytes: c.max_total_bytes,
                    })
                    .unwrap_or_default();
                Some(crate::snapshot::SnapshotManager::new_with_options(
                    pool,
                    project_root,
                    options,
                ))
            } else {
                None
            }
        } else {
            None
        };

        let todo_pool = pool.clone();

        let usage_store = pool
            .clone()
            .map(|p| Arc::new(crate::session::UsageStore::new(p)));
        let pricing_service = crate::util::pricing::PricingService::new();
        let security_service =
            crate::security::service::SecurityService::new(config.security.as_ref());

        let mut tool_registry = tool_registry;
        if let Some(deferred) = config
            .catalog
            .as_ref()
            .and_then(|c| c.deferred_tools.as_ref())
        {
            tool_registry.register_deferred_names(deferred);
        }

        // Set search mode from tool_deferral config
        if let Some(ref td) = config.tool_deferral {
            if let Some(ref mode_str) = td.search_mode {
                let mode = crate::tool::catalog::SearchMode::from_config(mode_str);
                tool_registry.set_search_mode(mode);
            }
        }

        let projection_config = Self::resolve_projection_config(&config);

        Self {
            agents: map,
            state: AgentLoopState {
                current_agent: default_name,
                turn_count: 0,
                total_tokens: 0,
                start_time: Instant::now(),
                plan_mode: false,
                plan_topic: None,
                tool_call_count: 0,
                last_turn_input_tokens: 0,
                last_turn_output_tokens: 0,
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
            deferred_tool_definitions: Vec::new(),
            model_router,
            snapshot_manager,
            file_change_rx: crate::bus::global::GlobalEventBus::subscribe(),
            usage_store,
            pricing_service,
            security_service,
            recent_findings: Vec::new(),
            todo_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::task_state::TodoState::new(),
            )),
            task_state_policy: crate::model_profile::types::TaskStatePolicy::explicit_todo(),
            todo_pool: todo_pool.clone(),
            event_store: pool
                .as_ref()
                .map(|p| Arc::new(crate::session::EventStore::new(p.clone()))),
            active_tool_timings: HashMap::new(),
            execution_policy: None,
            original_user_prompt: None,
            subagent_pool: None,
            max_tool_calls: None,
            goal_store: pool
                .as_ref()
                .map(|p| Arc::new(crate::goal::GoalStore::new(p.clone()))),
            goal_wall_clock: std::sync::Mutex::new(crate::goal::runtime::GoalWallClock::default()),
            cancel_rx: None,
            steer_rx: None,
            pending_steer: None,
            context_ledger: crate::agent::context_frame::ContextLedgerState::new(),
            artifact_store,
            projection_config,
        }
    }

    /// Build a `ProjectionConfig` from the loaded `[context]` config section.
    /// Falls back to sensible defaults when the section is absent or fields
    /// are `None`.
    fn resolve_projection_config(config: &Config) -> crate::context::ProjectionConfig {
        let Some(ctx) = config.context.as_ref() else {
            return crate::context::ProjectionConfig::default();
        };
        crate::context::ProjectionConfig {
            enabled: ctx.project_tool_outputs.unwrap_or(true),
            max_success_tokens: ctx.max_success_tokens.unwrap_or(800),
            max_failure_tokens: ctx.max_failure_tokens.unwrap_or(2000),
            artifact_store_enabled: ctx.artifact_store.unwrap_or(true),
            lossless_debug: ctx.lossless_debug.unwrap_or(false),
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
        if let Some(ref policy) = self.execution_policy {
            return policy.max_parallel_tools;
        }
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

    pub fn setup_question_channel_for_exec(&mut self) {
        self.setup_question_channel_impl(true);
    }

    fn setup_question_channel_impl(&mut self, exec_mode: bool) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.question_tx = Some(tx);
        if exec_mode {
            self.question_rx = Some(rx);
        }
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

    pub fn set_subagent_pool(&mut self, pool: Arc<crate::agent::worker::SubAgentPool>) {
        self.subagent_pool = Some(pool);
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn set_task_state_policy(&mut self, policy: crate::model_profile::types::TaskStatePolicy) {
        self.task_state_policy = policy;
    }

    pub fn set_execution_policy(&mut self, policy: crate::agent::policy::ExecutionPolicy) {
        self.context_tracker.set_limit(policy.context_window);
        self.context_tracker
            .set_threshold(policy.compaction_threshold);
        self.context_tracker.set_model(Some(policy.model.clone()));
        self.execution_policy = Some(policy);
    }

    pub fn set_max_tool_calls(&mut self, max: Option<usize>) {
        self.max_tool_calls = max;
    }

    pub fn set_cancel_receiver(&mut self, rx: tokio::sync::watch::Receiver<bool>) {
        self.cancel_rx = Some(rx);
    }

    pub fn set_steer_receiver(&mut self, rx: mpsc::UnboundedReceiver<String>) {
        self.steer_rx = Some(rx);
    }

    /// Evaluate the research trigger heuristic against a user prompt
    /// and, if it fires, prepend a hint to the next user message that
    /// tells the model about the `research` subagent. Returns
    /// `Some(hint)` when the hint was generated (caller can prepend
    /// it to the user-visible message), `None` otherwise.
    ///
    /// The trigger config lives at `config.research.auto_trigger`.
    /// When `enabled` is `false` or the confidence is below
    /// `min_confidence`, the hint is suppressed. Plan mode always
    /// suppresses the hint (research is not part of the plan-mode
    /// surface).
    pub fn maybe_inject_research_hint(&self, user_prompt: &str) -> Option<String> {
        if self.state.plan_mode {
            return None;
        }
        let trigger_cfg = self
            .config
            .research
            .as_ref()
            .and_then(|r| r.auto_trigger.clone())
            .unwrap_or_default();
        if !trigger_cfg.enabled {
            return None;
        }
        // Build a fresh TriggerConfig from the resolved profile (the
        // keyword lists live in the research module and are not part
        // of the user-facing schema).
        let trigger = crate::research::triggers::TriggerConfig {
            enabled: true,
            min_confidence: f64::from(trigger_cfg.min_confidence),
            ..Default::default()
        };
        let analysis = crate::research::triggers::analyze_trigger(user_prompt, &[], &[], &trigger);
        if !analysis.should_invoke {
            return None;
        }
        Some(format!(
            "[Hint: this task looks like a `{:?}` question (confidence: {:.2}). \
             Consider spawning a `research` subagent via \
             `task({{action: 'spawn', agent: 'research', prompt: '…'}})` for a structured, \
             multi-source answer with citations. You can also just use `websearch` for a quick lookup.]",
            analysis.suggested_mode,
            analysis.confidence,
        ))
    }

    pub fn execution_policy(&self) -> Option<&crate::agent::policy::ExecutionPolicy> {
        self.execution_policy.as_ref()
    }

    pub async fn build_context_frame(&self) -> crate::agent::context_frame::ContextFrame {
        let todo = self.todo_state.lock().await;
        let current_task = todo
            .items
            .iter()
            .find(|item| item.status == crate::task_state::TodoStatus::InProgress)
            .map(|item| item.content.clone());
        let next_steps: Vec<String> = todo
            .items
            .iter()
            .filter(|item| item.status == crate::task_state::TodoStatus::Pending)
            .take(3)
            .map(|item| item.content.clone())
            .collect();
        let security_findings: Vec<String> = self
            .recent_findings
            .iter()
            .map(|f| {
                let cat = format!("{:?}", f.category);
                format!("[{}] {}", cat, f.evidence)
            })
            .take(5)
            .collect();
        drop(todo);

        let mut frame = crate::agent::context_frame::ContextFrame {
            user_goal: self.original_user_prompt.clone(),
            current_task,
            constraints: Vec::new(),
            decisions: Vec::new(),
            touched_files: Vec::new(),
            commands_run: Vec::new(),
            test_results: Vec::new(),
            unresolved_errors: Vec::new(),
            security_findings,
            next_steps,
        };

        let ledger_frame = self.context_ledger.to_context_frame();
        if !ledger_frame.touched_files.is_empty() {
            frame.touched_files = ledger_frame.touched_files;
        }
        if !ledger_frame.commands_run.is_empty() {
            frame.commands_run = ledger_frame.commands_run;
        }
        if !ledger_frame.test_results.is_empty() {
            frame.test_results = ledger_frame.test_results;
        }
        if !ledger_frame.unresolved_errors.is_empty() {
            frame.unresolved_errors = ledger_frame.unresolved_errors;
        }

        frame
    }

    pub fn todo_state(&self) -> std::sync::Arc<tokio::sync::Mutex<crate::task_state::TodoState>> {
        self.todo_state.clone()
    }

    pub async fn load_persisted_todos(&self) {
        if let Some(pool) = &self.todo_pool {
            if !self.session_id.is_empty() {
                let store = crate::session::store::TodoStore::new(pool.clone());
                match store.list(&self.session_id).await {
                    Ok(session_items) => {
                        let mut todo = self.todo_state.lock().await;
                        todo.load_from_session(session_items);
                    }
                    Err(e) => {
                        tracing::debug!("No persisted todos for session: {}", e);
                    }
                }
            }
        }
    }

    async fn stream_with_retry(
        &mut self,
        request: &ChatRequest,
    ) -> Result<Vec<ChatEvent>, AppError> {
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
                        &e,
                        AppError::Provider(p) if p.is_retryable()
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

    async fn stream_once(&mut self, request: &ChatRequest) -> Result<Vec<ChatEvent>, AppError> {
        let stream = tokio::time::timeout(Duration::from_secs(120), self.provider.stream(request))
            .await
            .map_err(|_| {
                AppError::Provider(ProviderError::Timeout(
                    "provider stream timeout".to_string(),
                ))
            })??;
        let mut events = Vec::with_capacity(64);
        let session_id_arc: Arc<str> = Arc::from(self.session_id.as_str());
        let model_name = request.model.clone();
        let provider_name = self.provider.name().to_string();
        let usage_store = self.usage_store.clone();
        let pricing_service = crate::util::pricing::PricingService::new();

        use futures::StreamExt;
        let mut stream = stream;
        const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
        loop {
            let next_event = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())
                .await
                .map_err(|_| {
                    AppError::Provider(ProviderError::Timeout(
                        "provider stream stalled waiting for next event".to_string(),
                    ))
                })?;
            let Some(event) = next_event else {
                break;
            };
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
                        ChatEvent::Finish { usage, .. } => {
                            if let Some(ref store) = usage_store {
                                let session_id = self.session_id.clone();
                                let model = model_name.clone();
                                let provider = provider_name.clone();
                                let input_tokens = usage.input_tokens as i64;
                                let output_tokens = usage.output_tokens as i64;
                                let cached_tokens = usage.cached_tokens.unwrap_or(0) as i64;
                                let cost_usd = pricing_service.calculate_cost(
                                    &provider,
                                    &model,
                                    input_tokens,
                                    output_tokens,
                                    cached_tokens,
                                );
                                let timestamp = chrono::Utc::now().timestamp_millis();
                                let record = crate::session::UsageRecord {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    session_id,
                                    provider,
                                    model,
                                    input_tokens,
                                    output_tokens,
                                    cached_tokens,
                                    cost_usd,
                                    timestamp,
                                };
                                let store = store.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = store.insert(record).await {
                                        tracing::error!("failed to insert usage record: {}", e);
                                    }
                                });
                            }
                            // Record per-turn token usage on the loop
                            // state so `account_goal_for_turn` can pass
                            // it to the active goal's budget.
                            self.state.last_turn_input_tokens = usage.input_tokens as i64;
                            self.state.last_turn_output_tokens = usage.output_tokens as i64;
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

    fn publish_agent_finished(&self, events: &[ChatEvent]) {
        let last_finish = events.iter().rev().find_map(|event| {
            if let ChatEvent::Finish { stop_reason, usage } = event {
                Some((stop_reason, usage))
            } else {
                None
            }
        });

        let (stop_reason_str, input_tokens, output_tokens, cached_tokens, reasoning_tokens) =
            if let Some((stop_reason, usage)) = last_finish {
                (
                    stop_reason.to_string(),
                    Some(usage.input_tokens),
                    Some(usage.output_tokens),
                    usage.cached_tokens,
                    if usage.reasoning_tokens > 0 {
                        Some(usage.reasoning_tokens)
                    } else {
                        None
                    },
                )
            } else {
                ("completed".to_string(), None, None, None, None)
            };

        crate::bus::global::GlobalEventBus::publish(AppEvent::AgentFinished {
            session_id: self.session_id.clone(),
            stop_reason: stop_reason_str,
            input_tokens,
            output_tokens,
            cached_tokens,
            reasoning_tokens,
        });
    }

    /// Account a finished turn against the active goal. Called from
    /// `run()` after the loop body so the budget is updated even on
    /// the user's last turn.
    async fn account_goal_for_turn(&self) {
        let Some(goal_store) = self.goal_store.clone() else {
            return;
        };
        if self.session_id.is_empty() {
            return;
        }
        // Compute wall-clock delta since the last accounting tick.
        let wallclock_delta = {
            let mut wc = self
                .goal_wall_clock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let delta = wc.elapsed_secs_since_last();
            // Always reset the clock so the next tick measures fresh
            // wall-clock, even when the goal store is unavailable.
            wc.last_accounted_at = Some(std::time::Instant::now());
            delta
        };
        let tool_calls = self.state.tool_call_count as i64;
        let input_tokens = self.state.last_turn_input_tokens;
        let output_tokens = self.state.last_turn_output_tokens;
        let _ = crate::goal::runtime::account_for_turn(
            &goal_store,
            &self.session_id,
            input_tokens,
            output_tokens,
            tool_calls,
            1,
            wallclock_delta,
        )
        .await;
    }

    /// Decide whether to autonomously continue the active goal.
    ///
    /// Called from `run()` after `account_goal_for_turn()`. If the goal
    /// runtime returns `Continue`, we queue a continuation prompt and
    /// recurse through `drain_follow_up`. If it returns `BudgetLimited`,
    /// we queue a wrap-up prompt and let the loop drain that single
    /// follow-up without scheduling another continuation. This mirrors
    /// codex's `maybe_start_goal_continuation_turn` pattern.
    async fn maybe_continue_goal(
        &mut self,
        request: &mut ChatRequest,
        all_events: &mut Vec<ChatEvent>,
        processor: &mut EventProcessor,
    ) {
        let Some(goal_store) = self.goal_store.clone() else {
            return;
        };
        if self.session_id.is_empty() {
            return;
        }

        // Bounded safety: don't run the continuation loop forever even
        // if the runtime returns Continue on every tick. We rely on
        // the budget/terminal-status checks inside `should_continue`
        // to break out, but cap the outer iterations as a guard.
        const MAX_CONTINUATIONS: usize = 32;
        for _ in 0..MAX_CONTINUATIONS {
            let decision = match crate::goal::runtime::should_continue_for_session(
                &goal_store,
                &self.session_id,
            )
            .await
            {
                Ok(Some(d)) => d,
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("goal runtime decision failed: {e}");
                    return;
                }
            };
            if !decision.should_continue {
                if let Some(prompt) = decision.prompt {
                    // Final wrap-up prompt (e.g. budget-limited).
                    let _ = self.follow_up_tx.send(prompt);
                    self.drain_follow_up(request, all_events, processor).await;
                }
                return;
            }
            let Some(prompt) = decision.prompt else {
                return;
            };
            tracing::info!(
                "goal continuation queued (session={}): {}",
                self.session_id,
                decision.reason
            );
            // Reset per-turn token/tool counters so the next
            // accounting tick measures the *continuation* turn, not
            // a stale carry-over from the user's turn.
            self.state.last_turn_input_tokens = 0;
            self.state.last_turn_output_tokens = 0;
            let _ = self.follow_up_tx.send(prompt);
            self.drain_follow_up(request, all_events, processor).await;
            // After the continuation turn finishes, account for it
            // before deciding whether to continue again.
            // We can't call `account_goal_for_turn` here directly
            // because it borrows self immutably and we already have
            // &mut self via the request parameter. Inline the
            // accounting using a clone of the wall-clock state.
            self.account_goal_for_turn().await;
        }
        tracing::warn!("goal continuation hit MAX_CONTINUATIONS={MAX_CONTINUATIONS}, halting");
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

        if let Some(max) = self.max_tool_calls {
            if self.state.tool_call_count >= max {
                return Some(format!("max tool calls ({}) reached", max));
            }
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
            if let Some(budget) = agent.thinking_budget {
                request.thinking_budget = Some(budget);
            }
            if let Some(effort) = agent.reasoning_effort.clone() {
                request.reasoning_effort = Some(effort);
            }
        }
    }

    fn apply_model_profile_defaults(
        &self,
        request: &mut ChatRequest,
        profile: &crate::model_profile::types::ResolvedModelProfile,
    ) {
        if request.reasoning_effort.is_none() {
            request.reasoning_effort = profile.default_reasoning_effort.clone();
        }
        if request.thinking_budget.is_none() {
            request.thinking_budget = profile.default_thinking_budget;
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
            crate::bus::global::GlobalEventBus::publish(AppEvent::ModelChanged {
                model: model.clone(),
                complexity: complexity.as_str().to_string(),
            });
            request.model = model;
        }
    }

    fn infer_tool_from_prompt(prompt: &str) -> &'static str {
        let p = prompt.to_lowercase();
        if p.contains("debug")
            || p.contains("analyze")
            || p.contains("review")
            || p.contains("architect")
            || p.contains("investigate")
        {
            return "debug";
        }
        if p.contains("edit")
            || p.contains("rewrite")
            || p.contains("refactor")
            || p.contains("patch")
            || p.contains("modify")
            || p.contains("update")
            || p.contains("change")
        {
            return "edit";
        }
        if p.contains("write")
            || p.contains("create")
            || p.contains("implement")
            || p.contains("add")
            || p.contains("build")
        {
            return "write";
        }
        if p.contains("search") || p.contains("find") || p.contains("grep") {
            return "search";
        }
        if p.contains("list") || p.contains("show") || p.contains("read") || p.contains("view") {
            return "read";
        }
        "read"
    }

    fn extract_first_prompt_and_tool(&self, request: &ChatRequest) -> (String, &'static str) {
        for msg in &request.messages {
            if let Message::User { content } = msg {
                for part in content {
                    if let crate::provider::ContentPart::Text { text } = part {
                        let prompt = text.to_string();
                        let tool = Self::infer_tool_from_prompt(&prompt);
                        return (prompt, tool);
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

        // Build an MCP exposure policy from the resolved
        // `[search]` and `[tool_backends.*]` config so raw
        // Codegg-managed backends (eggsearch today, future
        // egglsp/eggsentry MCP adapters) are hidden by default while
        // user-configured third-party MCP servers stay visible.
        let search_cfg = crate::search_backend::state::search_config();
        let tool_backends = self.tool_registry.tool_backends();
        let expose_raw_search = search_cfg.expose_raw_mcp_tools();
        let eggsearch_server = search_cfg
            .eggsearch
            .as_ref()
            .and_then(|e| e.server_name.clone())
            .unwrap_or_else(|| "eggsearch".to_string());
        let mut hidden_servers: Vec<String> = Vec::new();
        // Always hide eggsearch raw tools unless explicitly opted
        // in via `[search].expose_raw_mcp_tools = true`.
        if !expose_raw_search {
            hidden_servers.push(eggsearch_server.clone());
        }
        // Per-domain backend config: when the user has set
        // `expose_raw_mcp_tools = true` for a managed backend,
        // unhide that server. This is the forward-compatible hook
        // for the future `egglsp` and `eggsentry` MCP adapters.
        for domain_cfg in [
            tool_backends.lsp.as_ref(),
            tool_backends.security.as_ref(),
            tool_backends.context.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(server) = domain_cfg.server_name.as_ref() {
                if domain_cfg.expose_raw_mcp_tools() {
                    hidden_servers.retain(|s| s != server);
                } else {
                    if !hidden_servers.iter().any(|s| s == server) {
                        hidden_servers.push(server.clone());
                    }
                }
            }
        }
        let policy = crate::mcp::McpExposurePolicy {
            show_raw: true,
            hidden_servers,
        };

        let mcp_tools = if let Some(ref mcp_arc) = self.mcp_service {
            match mcp_arc.try_read() {
                Ok(mcp) => mcp.list_filtered_tools(&policy),
                Err(_) => {
                    tracing::debug!("MCP service write-locked during tool def building, retrying");
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    mcp_arc
                        .try_read()
                        .map(|mcp| mcp.list_filtered_tools(&policy))
                        .unwrap_or_default()
                }
            }
        } else {
            Vec::new()
        };
        let mcp_tool_count = mcp_tools.len();

        // Set defer_loading on MCP tools based on the catalog
        let catalog = self.tool_registry.catalog();
        let mcp_tools: Vec<_> = mcp_tools
            .into_iter()
            .map(|mut t| {
                if catalog.is_deferred(&t.name) {
                    t.defer_loading = Some(true);
                }
                t
            })
            .collect();

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
            cache_expose_raw,
            ref cached_defs,
            ref cached_deferred,
        )) = self.tool_def_cache
        {
            if cache_model.as_ref().map(|s| s.as_str()) == model.map(|s| s.as_str())
                && cache_plan == self.state.plan_mode
                && cache_lsp == lsp_enabled
                && cache_mcp_count == mcp_tool_count
                && cache_perm_ver == permission_version
                && cache_expose_raw == expose_raw_search
            {
                let mut definitions = cached_defs.clone();
                self.deferred_tool_definitions = cached_deferred.clone();

                // Separate MCP tools into immediate vs deferred
                for mcp_def in mcp_tools {
                    if mcp_def.defer_loading == Some(true) {
                        self.deferred_tool_definitions.push(mcp_def);
                    } else {
                        definitions.push(mcp_def);
                    }
                }

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
                                    defer_loading: None,
                                })
                            })
                            .collect();
                    }
                }

                definitions.extend(self.deferred_tool_definitions.iter().cloned());
                return definitions;
            }
        }

        let tools = self.tool_registry.list();
        let flags = compute_model_flags(model);
        // Hide tools that the registry marks as non-exposed
        // (e.g. `DisabledTool` stubs) so the model never sees a
        // tool whose every call is a guaranteed failure. This is
        // the model-facing half of the same predicate the
        // registry uses in `definitions()`.
        let tools: Vec<&dyn crate::tool::Tool> = tools
            .into_iter()
            .filter(|t| t.expose_in_definitions())
            .collect();
        let filtered =
            filter_tools_for_model(model, &tools, self.state.plan_mode, lsp_enabled, &flags);
        let all_definitions: Vec<_> = filtered
            .iter()
            .map(|t| crate::provider::ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
                defer_loading: if t.defer_loading() { Some(true) } else { None },
            })
            .collect();

        let all_definitions = self.apply_tool_exposure_filter(all_definitions);

        // Include MCP tools in the definitions for deferral partitioning
        let mut all_definitions = all_definitions;
        all_definitions.extend(mcp_tools);

        // Partition tools into immediate vs deferred based on provider capabilities
        let provider_id = self.provider.id();
        let caps = crate::provider::ProviderCapabilities::for_provider(provider_id);
        let deferral_enabled = self
            .config
            .tool_deferral
            .as_ref()
            .and_then(|td| td.defer_loading)
            .unwrap_or(true);

        let always_loaded: Vec<String> = self
            .config
            .tool_deferral
            .as_ref()
            .and_then(|td| td.always_loaded.clone())
            .unwrap_or_default();

        let max_initial = self
            .config
            .tool_deferral
            .as_ref()
            .and_then(|td| td.max_initial_tools);

        let (definitions, deferred) = if deferral_enabled && caps.supports_defer_loading {
            let mut immediate = Vec::new();
            let mut deferred_tools = Vec::new();

            for def in all_definitions {
                let is_always_loaded = always_loaded.iter().any(|n| n == &def.name);
                let should_defer = !is_always_loaded && def.defer_loading == Some(true);

                if should_defer {
                    deferred_tools.push(def);
                } else {
                    immediate.push(def);
                }
            }

            // Apply max_initial_tools cap if configured
            let immediate = if let Some(max) = max_initial {
                if immediate.len() > max {
                    // Move excess tools to deferred
                    let (kept, excess) = immediate.split_at(max);
                    let mut deferred_tools = deferred_tools;
                    deferred_tools.extend(excess.iter().cloned());
                    self.deferred_tool_definitions = deferred_tools;
                    kept.to_vec()
                } else {
                    self.deferred_tool_definitions = deferred_tools;
                    immediate
                }
            } else {
                self.deferred_tool_definitions = deferred_tools;
                immediate
            };

            (immediate, self.deferred_tool_definitions.clone())
        } else {
            // Provider doesn't support defer_loading or deferral is disabled: all tools immediate.
            // Providers like deepseek, qwen, cerebras, groq, etc. go through OpenAiCompatibleProvider
            // with provider_ids not matching "openai" or "anthropic", so they get default capabilities
            // (supports_defer_loading: false). All tools are sent in the single `tools` array.
            self.deferred_tool_definitions.clear();
            (all_definitions, Vec::new())
        };

        // Update tool_search with available tool names so search results
        // only include tools the LLM can actually call
        let mut available_names: Vec<String> = definitions.iter().map(|t| t.name.clone()).collect();
        // Also include deferred tool names so they can be found via search
        available_names.extend(deferred.iter().map(|t| t.name.clone()));
        self.tool_registry
            .set_search_tool_available_tools(available_names);

        self.tool_def_cache = Some((
            model.map(|s| s.to_string()),
            self.state.plan_mode,
            lsp_enabled,
            mcp_tool_count,
            permission_version,
            expose_raw_search,
            definitions.clone(),
            deferred,
        ));

        let mut result = definitions;
        result.extend(self.deferred_tool_definitions.iter().cloned());

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
                            defer_loading: None,
                        })
                    })
                    .collect();
            }
        }

        result
    }

    async fn compact_if_needed(
        &mut self,
        messages: &mut Vec<Message>,
        model_profile: &crate::model_profile::types::ResolvedModelProfile,
    ) {
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
            .execution_policy
            .as_ref()
            .map_or(10_000, |p| p.reserved_output_tokens);

        if detect_overflow(messages, self.context_tracker.context_limit(), reserved) {
            tracing::warn!("Context overflow detected, applying pruning");
            let max_tokens = self
                .execution_policy
                .as_ref()
                .map_or(10_000, |p| p.max_tool_result_tokens);
            *messages = prune_tool_outputs(messages, max_tokens);
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

            // Check if new hybrid engine should be used
            let has_new_config = self
                .config
                .compaction
                .as_ref()
                .map(|c| c.mode.is_some())
                .unwrap_or(false);

            if has_new_config && auto {
                // Use new hybrid engine
                let active_model = Some(model_profile.model.as_str());
                let resolved_config = self
                    .config
                    .compaction
                    .as_ref()
                    .map(|c| {
                        ResolvedCompactionConfig::from_config(
                            c,
                            self.context_tracker.context_limit(),
                            active_model,
                        )
                    })
                    .unwrap_or_default();

                let input = CompactionInput {
                    messages,
                    config: resolved_config,
                    active_model,
                };

                match compact_with_policy(input, Some(self.provider.as_ref())).await {
                    Ok(output) => {
                        *messages = output.messages;
                        tracing::info!(
                            "Hybrid compaction: {} -> {} tokens",
                            output.tokens_before,
                            output.tokens_after,
                        );
                    }
                    Err(err) => {
                        tracing::error!("Hybrid compaction failed: {}, using legacy", err);
                        let limit = self.context_tracker.context_limit();
                        let threshold = self.context_tracker.threshold();
                        let model = self
                            .config
                            .compaction
                            .as_ref()
                            .and_then(|c| c.summarize_model.as_deref());
                        let compacted = auto_compact_async(
                            messages,
                            limit,
                            threshold,
                            prune,
                            Some(self.provider.as_ref()),
                            model,
                        )
                        .await;
                        *messages = compacted;
                    }
                }
            } else {
                // Legacy path
                if auto {
                    let limit = self.context_tracker.context_limit();
                    let threshold = self.context_tracker.threshold();
                    let model = self
                        .config
                        .compaction
                        .as_ref()
                        .and_then(|c| c.summarize_model.as_deref());
                    let compacted = auto_compact_async(
                        messages,
                        limit,
                        threshold,
                        prune,
                        Some(self.provider.as_ref()),
                        model,
                    )
                    .await;
                    *messages = compacted;
                } else {
                    *messages = compact_messages_sync(
                        messages.clone(),
                        CompactionStrategy::DropMiddleMessages,
                    );
                }
            }

            let tokens_before = self.context_tracker.current_tokens();
            let tokens_after = self.context_tracker.estimate_tokens_for_messages(messages);
            self.context_tracker.reset();
            self.context_tracker.add_messages(messages);

            // Skip context frame injection if already injected by compaction
            let already_has_frame = messages.iter().any(|m| {
                matches!(m, Message::System { content } if content.contains("[codegg compacted session state]"))
            });

            if !already_has_frame {
                let frame = self.build_context_frame().await;
                if !frame.is_empty() {
                    let frame_text = frame.to_control_text();
                    tracing::debug!(
                        "Inserting context frame after compaction: {} chars",
                        frame_text.len()
                    );
                    push_control_instruction(messages, model_profile, &frame_text);
                }
            }

            if self.task_state_policy.inject_after_compaction {
                let mut todo = self.todo_state.lock().await;
                if !todo.is_all_done() {
                    if let Some(reminder) =
                        crate::task_state::build_todo_reminder(&todo, &self.task_state_policy)
                    {
                        push_control_instruction(messages, model_profile, &reminder);
                        todo.reminder_pending = false;
                        todo.tool_calls_since_injection = 0;
                    }
                }
            }

            crate::bus::global::GlobalEventBus::publish(AppEvent::CompactionTriggered {
                session_id: self.session_id.clone(),
                tokens_before,
                tokens_after,
            });
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
            for err in hr
                .run_hooks(crate::hooks::HookEvent::SessionStart, &session_start_ctx)
                .await
            {
                tracing::error!("SessionStart hook error: {}", err);
            }
        }

        self.apply_auto_routing(&mut request);
        self.apply_agent_config(&mut request);
        let model_profile =
            crate::model_profile::ModelProfileResolver::new(&self.config).resolve(&request.model);

        let exec_policy =
            crate::agent::policy::ExecutionPolicy::from_profile(&model_profile, &self.config);
        self.set_execution_policy(exec_policy.clone());
        self.apply_model_profile_defaults(&mut request, &model_profile);
        tracing::debug!(
            "Execution policy resolved: model={}, context_window={}, threshold={}, tool_mode={:?}, max_parallel={}",
            exec_policy.model,
            exec_policy.context_window,
            exec_policy.compaction_threshold,
            exec_policy.initial_tool_mode,
            exec_policy.max_parallel_tools,
        );
        if let Some(system) = request.system.take() {
            let mut content = system;
            if let Some(hints) = self
                .security_service
                .format_prompt_hints(&self.recent_findings)
            {
                content.push_str("\n\n");
                content.push_str(&hints);
            }
            if let Some(ref steer) = self.pending_steer {
                content.push_str(&format!("\n\n## User Steering\n{}\n", steer));
                self.pending_steer = None;
            }
            request.messages.insert(
                0,
                Message::System {
                    content: content.into(),
                },
            );
        }
        self.recent_findings.clear();
        request.tools = Some(crate::agent::policy::filter_tool_definitions_for_profile(
            self.build_tool_definitions().await,
            &model_profile,
        ));
        crate::model_profile::policy::apply_startup_profile_policy(
            &mut request.messages,
            &model_profile,
        );
        self.context_tracker.add_messages(&request.messages);

        let mut all_events = Vec::with_capacity(128);
        let mut processor = EventProcessor::new();
        let mut missing_structured_tool_call_retries: u8 = 0;
        let mut post_tool_continuation_retry_budget: u8 = 0;
        let mut just_executed_tools = false;
        let mut did_bootstrap_tool = false;
        let mut bootstrap_repeat_budget: u8 = 0;
        let mut narration_retry_budget: u8 = 0;
        let original_prompt = request
            .messages
            .iter()
            .find_map(|m| {
                if let Message::User { content } = m {
                    content.iter().find_map(|p| {
                        if let ContentPart::Text { text } = p {
                            Some(text.to_string())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            })
            .unwrap_or_default();

        if self.original_user_prompt.is_none() {
            self.original_user_prompt = Some(original_prompt.clone());
        }

        // Phase 3: research trigger hint. If the user's prompt looks
        // like a research task (comparison, library eval, API, security,
        // architecture), prepend a hint to the first user message so
        // the model is steered toward spawning a `research` subagent.
        if !original_prompt.is_empty() {
            if let Some(hint) = self.maybe_inject_research_hint(&original_prompt) {
                if let Some(Message::User { content }) = request
                    .messages
                    .iter_mut()
                    .find(|m| matches!(m, Message::User { .. }))
                {
                    // Prepend a text part to the existing user content.
                    let mut new_parts: Vec<ContentPart> = vec![ContentPart::Text {
                        text: hint.clone().into(),
                    }];
                    let old = std::mem::take(content);
                    new_parts.extend(old);
                    *content = new_parts;
                    tracing::debug!("Injected research trigger hint for mode: {}", hint);
                }
            }
        }

        loop {
            if let Some(reason) = self.check_limits() {
                tracing::info!("Agent loop stopping: {}", reason);
                break;
            }

            if let Some(ref mut cancel_rx) = self.cancel_rx {
                if *cancel_rx.borrow() {
                    tracing::info!("Turn cancelled via cancel signal");
                    break;
                }
            }

            if let Some(ref mut steer_rx) = self.steer_rx {
                if let Ok(text) = steer_rx.try_recv() {
                    self.pending_steer = Some(text.clone());
                    tracing::info!("Turn steer received: {}", text);
                }
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
                        push_control_instruction(&mut request.messages, &model_profile, &system);
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
                for err in hr
                    .run_hooks(crate::hooks::HookEvent::AgentStart, &agent_start_ctx)
                    .await
                {
                    tracing::error!("AgentStart hook error: {}", err);
                }
            }

            // Inject todo reminder if needed
            {
                let mut todo = self.todo_state.lock().await;
                let should_inject = (self.task_state_policy.inject_on_resume
                    && self.state.turn_count == 1)
                    || todo.reminder_pending
                    || (self
                        .task_state_policy
                        .inject_after_tool_calls
                        .is_some_and(|threshold| todo.tool_calls_since_injection >= threshold));
                if should_inject {
                    if let Some(reminder) =
                        crate::task_state::build_todo_reminder(&todo, &self.task_state_policy)
                    {
                        push_control_instruction(&mut request.messages, &model_profile, &reminder);
                        todo.reminder_pending = false;
                        todo.tool_calls_since_injection = 0;
                    }
                }
            }

            self.compact_if_needed(&mut request.messages, &model_profile)
                .await;
            harden_history(&mut request.messages);

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

            let mut tool_calls = processor.tool_calls().to_vec();
            if tool_calls.is_empty() {
                if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                    let preview: String = processor.text().chars().take(200).collect();
                    tracing::info!(
                        "tool-parse-fallback: tool_calls=0, stop_reason={:?}, text_len={}, text_preview={:?}",
                        processor.stop_reason(),
                        processor.text().len(),
                        preview
                    );
                }
                if let Some(parsed_calls) = parse_text_as_tool_calls(processor.text()) {
                    for tc in &parsed_calls {
                        crate::bus::global::GlobalEventBus::publish(AppEvent::ToolCallStarted {
                            session_id: self.session_id.clone(),
                            tool_name: tc.name.to_string(),
                            tool_id: tc.id.to_string(),
                            arguments: tc.arguments.to_string(),
                        });
                    }
                    tool_calls = parsed_calls;
                }
            }

            if tool_calls.is_empty() {
                let bootstrap_allowed = self
                    .execution_policy
                    .as_ref()
                    .map_or(true, |p| p.allow_bootstrap_tool);
                let stop_is_soft_or_confused = is_soft_stop_reason(processor.stop_reason())
                    || matches!(processor.stop_reason(), Some("tool_calls"));
                let model_stuck_narrating =
                    indicates_more_work(processor.text()) && processor.text().trim().len() >= 10;
                if bootstrap_allowed
                    && (!did_bootstrap_tool
                        || (model_stuck_narrating && bootstrap_repeat_budget < 2))
                    && self.state.turn_count <= 6
                    && stop_is_soft_or_confused
                    && is_repo_task_prompt(&original_prompt)
                {
                    let synthetic = ToolCall {
                        id: format!("call_bootstrap_{}", uuid::Uuid::new_v4()).into(),
                        name: "list".to_string().into(),
                        arguments: serde_json::json!({"path":"."}),
                    };
                    crate::bus::global::GlobalEventBus::publish(AppEvent::ToolCallStarted {
                        session_id: self.session_id.clone(),
                        tool_name: synthetic.name.to_string(),
                        tool_id: synthetic.id.to_string(),
                        arguments: synthetic.arguments.to_string(),
                    });
                    let tool_results = self
                        .execute_tool_calls(std::slice::from_ref(&synthetic))
                        .await?;
                    let assistant = Message::Assistant {
                        content: vec![],
                        tool_calls: vec![synthetic],
                    };
                    self.context_tracker.add_message(&assistant);
                    request.messages.push(assistant);
                    for (id, content) in &tool_results {
                        crate::bus::global::GlobalEventBus::publish(AppEvent::ToolResult {
                            tool_id: id.clone(),
                            tool_name: "list".to_string(),
                            session_id: self.session_id.clone(),
                            output: content.clone(),
                            success: tool_result_is_success(content),
                        });
                        let redacted_content = redact_local_paths(content);
                        let tool_name_str = "list";

                        let turn = self.state.turn_count;
                        let handle_result =
                            crate::context::ContextHandle::build_tool(&self.session_id, turn, id);
                        let effective_handle = if self.projection_config.artifact_store_enabled {
                            match handle_result {
                                Ok(ref handle) => {
                                    let store_result = self
                                        .artifact_store
                                        .put(crate::context::ContextArtifact {
                                            handle: handle.clone(),
                                            session_id: self.session_id.clone(),
                                            turn_index: turn,
                                            tool_call_id: Some(id.clone()),
                                            tool_name: Some(tool_name_str.to_string()),
                                            kind: crate::context::ArtifactKind::ToolResult,
                                            created_at_ms: chrono::Utc::now().timestamp_millis(),
                                            content_hash: crate::context::compute_content_hash(
                                                &redacted_content,
                                            ),
                                            redacted_content: redacted_content.clone(),
                                            raw_bytes_len: redacted_content.len(),
                                            estimated_tokens: crate::context::estimate_tokens(
                                                &redacted_content,
                                            ),
                                        })
                                        .await;
                                    match store_result {
                                        Ok(()) => handle.as_str(),
                                        Err(err) => {
                                            tracing::warn!(
                                                tool_call_id = %id,
                                                tool_name = %tool_name_str,
                                                session_id = %self.session_id,
                                                error = %err,
                                                "failed to store context artifact; omitting recovery handle"
                                            );
                                            ""
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        tool_call_id = %id,
                                        tool_name = %tool_name_str,
                                        session_id = %self.session_id,
                                        error = %err,
                                        "failed to build context handle; omitting recovery handle"
                                    );
                                    ""
                                }
                            }
                        } else {
                            ""
                        };

                        let proj = crate::context::project_tool_output(
                            tool_name_str,
                            None,
                            &redacted_content,
                            tool_result_is_success(content),
                            effective_handle,
                            &self.projection_config,
                        );

                        self.context_ledger
                            .record_projection(&proj, effective_handle);

                        let msg = Message::Tool {
                            tool_call_id: id.clone().into(),
                            content: proj.model_text.into(),
                        };
                        self.context_tracker.add_message(&msg);
                        request.messages.push(msg);
                    }
                    did_bootstrap_tool = true;
                    bootstrap_repeat_budget += 1;
                    processor.reset();
                    continue;
                }
                let nudge_allowed = self
                    .execution_policy
                    .as_ref()
                    .map_or(true, |p| p.allow_post_tool_continue_nudge);
                if nudge_allowed
                    && just_executed_tools
                    && post_tool_continuation_retry_budget < 2
                    && is_soft_stop_reason(processor.stop_reason())
                    && (processor.text().trim().len() < 220
                        || indicates_more_work(processor.text()))
                {
                    if let Some(msg) = processor.to_assistant_message() {
                        self.context_tracker.add_message(&msg);
                        request.messages.push(msg);
                    }
                    post_tool_continuation_retry_budget += 1;
                    just_executed_tools = false;
                    processor.reset();
                    continue;
                }
                if just_executed_tools
                    && is_repo_task_prompt(&original_prompt)
                    && is_soft_stop_reason(processor.stop_reason())
                {
                    push_control_instruction(
                        &mut request.messages,
                        &model_profile,
                        "Continue working and use additional structured tool calls as needed to complete repository analysis before finalizing.",
                    );
                    just_executed_tools = false;
                    processor.reset();
                    continue;
                }
                let stop_is_soft_or_confused = is_soft_stop_reason(processor.stop_reason())
                    || matches!(processor.stop_reason(), Some("tool_calls"));
                if stop_is_soft_or_confused
                    && indicates_more_work(processor.text())
                    && narration_retry_budget < 2
                {
                    if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                        tracing::info!(
                            "narration-retry: stop_reason={:?}, text_preview={:?}",
                            processor.stop_reason(),
                            processor.text().chars().take(200).collect::<String>()
                        );
                    }
                    if let Some(msg) = processor.to_assistant_message() {
                        self.context_tracker.add_message(&msg);
                        request.messages.push(msg);
                    }
                    push_control_instruction(
                        &mut request.messages,
                        &model_profile,
                        "You must emit structured tool calls in this turn. Do not describe tool usage in plain text. Return tool calls only.",
                    );
                    narration_retry_budget += 1;
                    processor.reset();
                    continue;
                }
                if matches!(processor.stop_reason(), Some("tool_calls"))
                    && missing_structured_tool_call_retries < 2
                {
                    push_control_instruction(
                        &mut request.messages,
                        &model_profile,
                        "You must emit structured tool calls in this turn. Do not describe tool usage in plain text. Return tool calls only.",
                    );
                    missing_structured_tool_call_retries += 1;
                    processor.reset();
                    continue;
                }
                if matches!(processor.stop_reason(), Some("tool_calls")) {
                    let raw_text = processor.text().to_string();
                    let preview = if raw_text.len() > 600 {
                        format!("{}…", &raw_text[..600])
                    } else {
                        raw_text
                    };
                    let preview = if preview.is_empty() {
                        "<empty stream>".to_string()
                    } else {
                        preview
                    };
                    tracing::warn!(
                        "Model returned stop_reason=tool_calls without parseable structured tool calls after retries; raw_text={}",
                        preview
                    );
                    crate::bus::global::GlobalEventBus::publish(AppEvent::Error {
                        message: format!(
                            "Model returned stop_reason=tool_calls without parseable structured tool calls after retries. Raw text: {}",
                            preview
                        ),
                    });
                }
                break;
            }
            missing_structured_tool_call_retries = 0;
            post_tool_continuation_retry_budget = 0;
            narration_retry_budget = 0;
            let tool_results = self.execute_tool_calls(&tool_calls).await?;
            just_executed_tools = !tool_results.is_empty();

            if !tool_calls.is_empty() {
                self.state.tool_call_count += tool_calls.len();
            }

            // Auto-invoke security-review subagent if triggered by high-risk tools or sensitive paths
            if just_executed_tools {
                let high_risk_findings: Vec<_> = self
                    .recent_findings
                    .iter()
                    .filter(|f| f.is_high_signal())
                    .cloned()
                    .collect();
                let edited_paths: Vec<String> = tool_calls
                    .iter()
                    .filter(|tc| is_file_modifying_tool(&tc.name))
                    .filter_map(extract_path_from_tool_call)
                    .collect();
                let sensitive_edits: Vec<String> = edited_paths
                    .iter()
                    .filter(|p| {
                        self.config.security.as_ref().is_some_and(|sec| {
                            crate::security::matches_sensitive_path(
                                Some(p.as_str()),
                                &sec.sensitive_paths,
                            )
                            .is_some()
                        })
                    })
                    .cloned()
                    .collect();
                if !high_risk_findings.is_empty() || !sensitive_edits.is_empty() {
                    self.maybe_spawn_security_review(&high_risk_findings, &sensitive_edits, false);
                }
            }

            if tool_results
                .iter()
                .any(|(_, out)| tool_result_is_success(out))
            {
                if let Some(doom_tool) = self.doom_detector.current_doom_tool() {
                    let doom_tool_owned = doom_tool.to_string();
                    let dominated =
                        tool_calls
                            .iter()
                            .zip(tool_results.iter())
                            .any(|(tc, (_, out))| {
                                tc.name.as_str() == doom_tool_owned && tool_result_is_success(out)
                            });
                    if dominated {
                        self.doom_detector.reset();
                    }
                } else {
                    self.doom_detector.reset();
                }
            }

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
                let success = tool_result_is_success(content);
                let redacted_output = redact_local_paths(content);
                crate::bus::global::GlobalEventBus::publish(AppEvent::ToolResult {
                    tool_id: id.clone(),
                    tool_name,
                    session_id: self.session_id.clone(),
                    output: redacted_output,
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

                let tool_args = tool_calls
                    .iter()
                    .find(|tc| tc.id.as_str() == id.as_str())
                    .map(|tc| tc.arguments.to_string());
                let tool_name_str = tool_calls
                    .iter()
                    .find(|tc| tc.id.as_str() == id.as_str())
                    .map(|tc| tc.name.to_string())
                    .unwrap_or_default();

                let turn = self.state.turn_count;
                let handle_result =
                    crate::context::ContextHandle::build_tool(&self.session_id, turn, id);
                let effective_handle = if self.projection_config.artifact_store_enabled {
                    match handle_result {
                        Ok(ref handle) => {
                            let store_result = self
                                .artifact_store
                                .put(crate::context::ContextArtifact {
                                    handle: handle.clone(),
                                    session_id: self.session_id.clone(),
                                    turn_index: turn,
                                    tool_call_id: Some(id.clone()),
                                    tool_name: Some(tool_name_str.clone()),
                                    kind: crate::context::ArtifactKind::ToolResult,
                                    created_at_ms: chrono::Utc::now().timestamp_millis(),
                                    content_hash: crate::context::compute_content_hash(
                                        &redacted_content,
                                    ),
                                    redacted_content: redacted_content.clone(),
                                    raw_bytes_len: redacted_content.len(),
                                    estimated_tokens: crate::context::estimate_tokens(
                                        &redacted_content,
                                    ),
                                })
                                .await;
                            match store_result {
                                Ok(()) => handle.as_str(),
                                Err(err) => {
                                    tracing::warn!(
                                        tool_call_id = %id,
                                        tool_name = %tool_name_str,
                                        session_id = %self.session_id,
                                        error = %err,
                                        "failed to store context artifact; omitting recovery handle"
                                    );
                                    ""
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                tool_call_id = %id,
                                tool_name = %tool_name_str,
                                session_id = %self.session_id,
                                error = %err,
                                "failed to build context handle; omitting recovery handle"
                            );
                            ""
                        }
                    }
                } else {
                    ""
                };

                let proj = crate::context::project_tool_output(
                    &tool_name_str,
                    tool_args.as_deref(),
                    &redacted_content,
                    tool_result_is_success(content),
                    effective_handle,
                    &self.projection_config,
                );

                self.context_ledger
                    .record_projection(&proj, effective_handle);

                let msg = Message::Tool {
                    tool_call_id: id.clone().into(),
                    content: proj.model_text.into(),
                };
                self.context_tracker.add_message(&msg);
                request.messages.push(msg);
            }

            // Track tool calls for todo reminder cadence
            if !tool_calls.is_empty() {
                let mut todo = self.todo_state.lock().await;
                todo.tool_calls_since_injection += tool_calls.len();
            }

            // Reset todo injection counter if todowrite was called
            {
                let has_todowrite = tool_calls.iter().any(|tc| tc.name.as_str() == "todowrite");
                if has_todowrite {
                    let mut todo = self.todo_state.lock().await;
                    todo.tool_calls_since_injection = 0;
                }
            }

            // Compact after tool results to prevent context overflow from large outputs
            self.compact_if_needed(&mut request.messages, &model_profile)
                .await;

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
                for err in hr
                    .run_hooks(crate::hooks::HookEvent::AgentEnd, &agent_end_ctx)
                    .await
                {
                    tracing::error!("AgentEnd hook error: {}", err);
                }
            }
        }

        self.drain_follow_up(&mut request, &mut all_events, &mut processor)
            .await;
        self.publish_agent_finished(&all_events);
        self.account_goal_for_turn().await;
        // After draining queued follow-ups and accounting, decide
        // whether to autonomously continue the active goal (long-
        // horizon continuation loop). Mirrors codex's
        // `maybe_start_goal_continuation_turn`.
        self.maybe_continue_goal(&mut request, &mut all_events, &mut processor)
            .await;

        crate::bus::global::GlobalEventBus::publish(AppEvent::ContextUpdated {
            session_id: self.session_id.clone(),
            context_tokens: self.context_tracker.current_tokens(),
            context_limit: self.context_tracker.context_limit(),
        });

        // Auto-invoke security-review subagent at session end for comprehensive review
        {
            let findings: Vec<_> = self
                .recent_findings
                .iter()
                .filter(|f| f.is_high_signal())
                .cloned()
                .collect();
            self.maybe_spawn_security_review(&findings, &[], true);
        }

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
            for err in hr
                .run_hooks(crate::hooks::HookEvent::SessionEnd, &session_end_ctx)
                .await
            {
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

    /// Evaluate heuristics and optionally spawn the security-review subagent.
    ///
    /// Triggers when:
    /// - A tool call is classified as high-risk by SecurityService
    /// - A file edit touches a sensitive path
    /// - `at_session_end` is true (pre-commit style review)
    ///
    /// Spawns as a background task — never blocks the main agent loop.
    fn maybe_spawn_security_review(
        &self,
        triggered_findings: &[crate::security::finding::SecurityFinding],
        edited_paths: &[String],
        at_session_end: bool,
    ) {
        let Some(ref pool) = self.subagent_pool else {
            return;
        };

        let _sec_config = match self.config.security.as_ref() {
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

        let spawner = pool.spawner();
        let task_id = rand::random::<u64>();
        let session_id = self.session_id.clone();

        let request = SubAgentRequest {
            task_id,
            prompt,
            agent: "security-review".to_string(),
            parent_id: Some(session_id),
            denied_tools: Vec::new(),
            allowed_paths: Vec::new(),
            description: "Auto-triggered security review".to_string(),
            depth: 1,
            max_tool_calls: None,
        };

        tokio::spawn(async move {
            if let Err(e) = spawner.send(request).await {
                tracing::warn!("Failed to spawn security-review subagent: {}", e);
            }
        });
    }

    fn drain_file_change_events(&mut self) -> Vec<(String, Option<String>)> {
        let mut changes = Vec::new();
        loop {
            match self.file_change_rx.try_recv() {
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

    async fn capture_incremental_snapshot_if_needed(&mut self, label: Option<String>) {
        if self.snapshot_manager.is_none() {
            return;
        }

        let changes = self.drain_file_change_events();
        if changes.is_empty() {
            return;
        }

        if let Some(ref snapshot_manager) = self.snapshot_manager {
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

    #[allow(clippy::incompatible_msrv)]
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
        let has_file_modifying = allowed_tools
            .iter()
            .any(|(_, tc)| is_file_modifying_tool(&tc.name));
        if has_file_modifying {
            // Clear stale file-change events so we only checkpoint this batch.
            let _ = self.drain_file_change_events();
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

        let mcp_timeout = Duration::from_secs(60);
        let mut mcp_futures = Vec::with_capacity(mcp_tool_calls.len());
        for (orig_idx, tc) in mcp_tool_calls {
            let name = tc.name.clone();
            let mcp_arc = self.mcp_service.clone();
            mcp_futures.push(async move {
                if let Some((server, tool)) = parse_mcp_tool_name(&name) {
                    if let Some(mcp_arc) = mcp_arc {
                        // Retry up to 3 times with brief backoff if RwLock is held
                        let mut last_err = None;
                        for attempt in 0..3 {
                            if attempt > 0 {
                                tokio::time::sleep(Duration::from_millis(50 * (attempt as u64))).await;
                            }
                            match mcp_arc.try_read() {
                                Ok(mcp) => {
                                    let call_result = tokio::time::timeout(
                                        mcp_timeout,
                                        mcp.call_tool(server, tool, tc.arguments.clone()),
                                    )
                                    .await;
                                    match call_result {
                                        Ok(Ok(result)) => {
                                            return (orig_idx, tc.id.to_string(), result);
                                        }
                                        Ok(Err(e)) => {
                                            return (orig_idx, tc.id.to_string(), format!("Error: {}", e));
                                        }
                                        Err(_) => {
                                            return (orig_idx, tc.id.to_string(), format!(
                                                "Error: MCP tool '{}' on server '{}' timed out after {:?}",
                                                tool, server, mcp_timeout
                                            ));
                                        }
                                    }
                                }
                                Err(_) => {
                                    last_err = Some(format!(
                                        "MCP service locked (attempt {}/3)",
                                        attempt + 1
                                    ));
                                }
                            }
                        }
                        (orig_idx, tc.id.to_string(), format!("Error: {}", last_err.unwrap_or_default()))
                    } else {
                        (orig_idx, tc.id.to_string(), "Error: MCP service not available".to_string())
                    }
                } else {
                    (orig_idx, tc.id.to_string(), format!("Error: Invalid MCP tool name '{}'", name))
                }
            });
        }
        let mcp_results = futures::future::join_all(mcp_futures).await;
        for result in mcp_results {
            tool_results.push(result);
        }

        let mut results = Vec::with_capacity(regular_tool_count);
        let sem = Arc::new(tokio::sync::Semaphore::new(effective_max));
        let mut futures = Vec::with_capacity(regular_tool_count);
        let hook_registry = self.hook_registry.as_ref().map(Arc::clone);
        let plugin_service = self.plugin_service.as_ref().map(Arc::clone);
        let event_store = self.event_store.clone();
        for (orig_idx, tc) in regular_tools {
            // Build the structured-execution context here (before
            // `tc` is moved into an Arc) so the helper, which takes
            // `&self`, can read live state without forcing the
            // `async move` closure to capture `self` by move.
            let tool_name_for_ctx = tc.name.clone();
            let timeout = self.get_tool_timeout(&tool_name_for_ctx);
            let exec_ctx = self.build_tool_execution_context(&tc, Some(timeout.as_millis() as u64));
            let tc_arc = Arc::new(tc);
            let sem = Arc::clone(&sem);
            let id = tc_arc.id.clone();
            let tool_name = tc_arc.name.clone();
            let hook_registry = hook_registry.clone();
            let plugin_service = plugin_service.clone();
            let session_id = self.session_id.clone();
            let idx_for_results = orig_idx;
            let event_store = event_store.clone();
            futures.push(async move {
                let permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        return (
                            idx_for_results,
                            id,
                            Err(ToolError::Execution(
                                "semaphore closed during tool execution".into(),
                            )),
                        );
                    }
                };

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

                if let Some(ref ps) = plugin_service {
                    let input = serde_json::json!({
                        "tool_name": tool_name,
                        "arguments": tc_arc.arguments,
                        "session_id": session_id,
                    });
                    let hook_result = ps.dispatch_tool_execute_before(input).await;
                    if hook_result.blocked {
                        tracing::warn!("Tool execution blocked by plugin hook");
                        drop(permit);
                        return (
                            idx_for_results,
                            id,
                            Err(ToolError::Execution("blocked by plugin hook".to_string())),
                        );
                    }
                    if let Some(err) = hook_result.error {
                        tracing::warn!("ToolExecuteBefore hook error: {}", err);
                    }
                }

                let tool_start = Instant::now();
                let risk = classify_tool_risk(&tool_name, &tc_arc.arguments);
                {
                    let meta = crate::session::events::EventMeta::new(&session_id);
                    let event = crate::session::events::SessionEvent::ToolCallStarted(
                        crate::session::events::ToolCallStartedEvent {
                            meta,
                            tool_call_id: id.to_string(),
                            tool_name: tool_name.to_string(),
                            arguments: tc_arc.arguments.to_string(),
                            risk: risk.clone(),
                        },
                    );
                    if let Some(ref store) = event_store {
                        let store = Arc::clone(store);
                        let ev = event.clone();
                        tokio::spawn(async move {
                            if let Err(e) = store.append(&ev).await {
                                tracing::warn!("Failed to store ToolCallStarted event: {}", e);
                            }
                        });
                    }
                }

                let result = {
                    let tc_inner = Arc::clone(&tc_arc);
                    if registry.get(&tc_inner.name).is_none() {
                        Err(ToolError::NotFound(tc_inner.name.to_string()))
                    } else {
                        let mut last_result: Result<String, ToolError> =
                            Err(ToolError::NotFound("no attempts made".into()));
                        for attempt in 0..2 {
                            if attempt > 0 {
                                tokio::time::sleep(Duration::from_millis(500)).await;
                                tracing::info!(
                                    "Retrying tool '{}' (attempt {})",
                                    tc_inner.name,
                                    attempt + 1
                                );
                            }
                            let exec_ctx = exec_ctx.clone();
                            let exec_fut = async {
                                let structured = registry
                                    .execute_capture(
                                        &tc_inner.name,
                                        tc_inner.arguments.clone(),
                                        Some(exec_ctx),
                                    )
                                    .await?;
                                if let Some(ref p) = structured.provenance {
                                    tracing::debug!(
                                        tool = %tc_inner.name,
                                        backend = %p.backend,
                                        implementation = %p.implementation,
                                        elapsed_ms = ?p.elapsed_ms,
                                        trust = ?p.trust,
                                        "native tool completed with provenance"
                                    );
                                }
                                Ok::<String, ToolError>(structured.output)
                            };
                            match tokio::time::timeout(timeout, exec_fut).await {
                                Ok(r) => match &r {
                                    Ok(_) => {
                                        last_result = r;
                                        break;
                                    }
                                    Err(e) if e.is_retryable() => {
                                        tracing::warn!(
                                            "Tool '{}' retryable error: {}",
                                            tc_inner.name,
                                            e
                                        );
                                        last_result = r;
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Tool '{}' non-retryable error: {}",
                                            tc_inner.name,
                                            e
                                        );
                                        last_result = r;
                                        break;
                                    }
                                },
                                Err(_) => {
                                    last_result = Err(ToolError::Execution(format!(
                                        "Tool '{}' timed out after {:?}",
                                        tc_inner.name, timeout
                                    )));
                                    break;
                                }
                            }
                        }
                        last_result
                    }
                };

                if let Some(ref ps) = plugin_service {
                    let input = serde_json::json!({
                        "tool_name": tool_name,
                        "arguments": tc_arc.arguments,
                        "session_id": session_id,
                        "result": result.as_ref().ok(),
                    });
                    let hook_result = ps.dispatch_tool_execute_after(input).await;
                    if let Some(err) = hook_result.error {
                        tracing::warn!("ToolExecuteAfter hook error: {}", err);
                    }
                }

                let post_ctx = crate::hooks::HookContext {
                    event: crate::hooks::HookEvent::PostToolExecute,
                    session_id: Some(session_id.clone()),
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

                let duration_ms = tool_start.elapsed().as_millis() as u64;
                let success = result.is_ok();
                let output_preview = result.as_ref().ok().map(|o| {
                    summarize_tool_output(&tool_name, o, success).unwrap_or_else(|| {
                        if o.len() > 200 {
                            format!("{}...", &o[..197])
                        } else {
                            o.clone()
                        }
                    })
                });
                {
                    let meta = crate::session::events::EventMeta::new(&session_id);
                    let event = crate::session::events::SessionEvent::ToolCallFinished(
                        crate::session::events::ToolCallFinishedEvent {
                            meta,
                            tool_call_id: id.to_string(),
                            tool_name: tool_name.to_string(),
                            status: if success {
                                crate::session::events::ToolCallStatus::Success
                            } else {
                                crate::session::events::ToolCallStatus::Error
                            },
                            duration_ms: Some(duration_ms),
                            output_preview,
                        },
                    );
                    if let Some(ref store) = event_store {
                        let store = Arc::clone(store);
                        let ev = event.clone();
                        tokio::spawn(async move {
                            if let Err(e) = store.append(&ev).await {
                                tracing::warn!("Failed to store ToolCallFinished event: {}", e);
                            }
                        });
                    }

                    // Emit test run events for test commands
                    if *tool_name == *"bash" {
                        if let Some(cmd) = tc_arc.arguments.get("command").and_then(|v| v.as_str())
                        {
                            if is_test_command(cmd) {
                                let test_meta = crate::session::events::EventMeta::new(&session_id);
                                let start_event =
                                    crate::session::events::SessionEvent::TestRunStarted(
                                        crate::session::events::TestRunStartedEvent {
                                            meta: test_meta,
                                            command: cmd.to_string(),
                                        },
                                    );
                                if let Some(ref store) = event_store {
                                    let store = Arc::clone(store);
                                    let ev = start_event;
                                    tokio::spawn(async move {
                                        if let Err(e) = store.append(&ev).await {
                                            tracing::warn!(
                                                "Failed to store TestRunStarted event: {}",
                                                e
                                            );
                                        }
                                    });
                                }

                                let test_output = result.as_ref().ok().cloned().unwrap_or_default();
                                let passed = success && !test_output.starts_with("Error: ");
                                let summary = if passed {
                                    "passed".to_string()
                                } else {
                                    let preview = if test_output.len() > 200 {
                                        format!("{}...", &test_output[..197])
                                    } else {
                                        test_output.clone()
                                    };
                                    format!("failed: {}", preview)
                                };
                                let finish_meta =
                                    crate::session::events::EventMeta::new(&session_id);
                                let finish_event =
                                    crate::session::events::SessionEvent::TestRunFinished(
                                        crate::session::events::TestRunFinishedEvent {
                                            meta: finish_meta,
                                            command: cmd.to_string(),
                                            passed,
                                            duration_ms: Some(duration_ms),
                                            summary,
                                        },
                                    );
                                if let Some(ref store) = event_store {
                                    let store = Arc::clone(store);
                                    let ev = finish_event;
                                    tokio::spawn(async move {
                                        if let Err(e) = store.append(&ev).await {
                                            tracing::warn!(
                                                "Failed to store TestRunFinished event: {}",
                                                e
                                            );
                                        }
                                    });
                                }
                            }
                        }
                    }
                }

                drop(permit);
                (idx_for_results, id, result)
            });
        }
        let all_results = futures::future::join_all(futures).await;
        results.extend(all_results);

        const MAX_TOOL_RESULT_BYTES_FALLBACK: usize = 512 * 1024; // 512KB per tool result
        let max_tool_result_bytes = self
            .execution_policy
            .as_ref()
            .map_or(MAX_TOOL_RESULT_BYTES_FALLBACK, |p| {
                p.max_tool_result_tokens * 4
            });
        for (idx, id, result) in results {
            let output = match result {
                Ok(output) => output,
                Err(e) => format!("Error: {}", e),
            };
            let truncated = if output.len() > max_tool_result_bytes {
                let safe_end = output.floor_char_boundary(max_tool_result_bytes);
                let mut truncated = output[..safe_end].to_string();
                truncated.push_str(&format!(
                    "\n... [truncated: output was {} bytes, limit is {} bytes]",
                    output.len(),
                    max_tool_result_bytes
                ));
                truncated
            } else {
                output
            };
            tool_results.push((idx, id.to_string(), truncated));
        }

        if has_file_modifying {
            self.capture_incremental_snapshot_if_needed(Some("incremental-pre-change".to_string()))
                .await;
        }

        if has_pending_question {
            if let Some(rx) = self.question_rx.take() {
                match tokio::time::timeout(Duration::from_secs(300), rx).await {
                    Ok(Ok(answers)) => {
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
                    Ok(Err(_)) => {
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
                    Err(_) => {
                        tool_results = tool_results
                            .into_iter()
                            .map(|(idx, id, output)| {
                                if output == "__QUESTION_PENDING__" {
                                    (
                                        idx,
                                        id,
                                        "[question timed out waiting for user response]"
                                            .to_string(),
                                    )
                                } else {
                                    (idx, id, output)
                                }
                            })
                            .collect();
                    }
                }
                QuestionRegistry::unregister(&self.session_id);
            } else {
                tool_results = tool_results
                    .into_iter()
                    .map(|(idx, id, output)| {
                        if output == "__QUESTION_PENDING__" {
                            (idx, id, "[question not supported in exec mode]".to_string())
                        } else {
                            (idx, id, output)
                        }
                    })
                    .collect();
            }
        }

        tool_results.sort_by_key(|(idx, _, _)| *idx);
        let ordered_results: Vec<(String, String)> = tool_results
            .into_iter()
            .map(|(_, id, output)| (id, output))
            .collect();

        Ok(ordered_results)
    }

    /// Drains queued follow-up prompts, if any are already queued.
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
        let model_profile =
            crate::model_profile::ModelProfileResolver::new(&self.config).resolve(&request.model);
        loop {
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
            let mut missing_structured_tool_call_retries: u8 = 0;
            let mut post_tool_continuation_retry_budget: u8 = 0;
            let mut just_executed_tools = false;
            let mut narration_retry_budget: u8 = 0;
            loop {
                self.compact_if_needed(&mut request.messages, &model_profile)
                    .await;
                harden_history(&mut request.messages);

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

                let mut tool_calls = processor.tool_calls().to_vec();
                if tool_calls.is_empty() {
                    if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                        let preview: String = processor.text().chars().take(200).collect();
                        tracing::info!(
                            "tool-parse-fallback(followup): tool_calls=0, stop_reason={:?}, text_len={}, text_preview={:?}",
                            processor.stop_reason(),
                            processor.text().len(),
                            preview
                        );
                    }
                    if let Some(parsed_calls) = parse_text_as_tool_calls(processor.text()) {
                        for tc in &parsed_calls {
                            crate::bus::global::GlobalEventBus::publish(
                                AppEvent::ToolCallStarted {
                                    session_id: self.session_id.clone(),
                                    tool_name: tc.name.to_string(),
                                    tool_id: tc.id.to_string(),
                                    arguments: tc.arguments.to_string(),
                                },
                            );
                        }
                        tool_calls = parsed_calls;
                    }
                }

                if tool_calls.is_empty() {
                    if just_executed_tools
                        && post_tool_continuation_retry_budget < 2
                        && is_soft_stop_reason(processor.stop_reason())
                        && (processor.text().trim().len() < 220
                            || indicates_more_work(processor.text()))
                    {
                        if let Some(msg) = processor.to_assistant_message() {
                            request.messages.push(msg);
                        }
                        post_tool_continuation_retry_budget += 1;
                        just_executed_tools = false;
                        processor.reset();
                        continue;
                    }
                    if just_executed_tools && is_soft_stop_reason(processor.stop_reason()) {
                        push_control_instruction(
                        &mut request.messages,
                        &model_profile,
                        "Continue the task and emit structured tool calls as needed before finalizing.",
                    );
                        just_executed_tools = false;
                        processor.reset();
                        continue;
                    }
                    let stop_is_soft_or_confused_fu = is_soft_stop_reason(processor.stop_reason())
                        || matches!(processor.stop_reason(), Some("tool_calls"));
                    if stop_is_soft_or_confused_fu
                        && indicates_more_work(processor.text())
                        && narration_retry_budget < 2
                    {
                        if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                            tracing::info!(
                                "narration-retry(followup): stop_reason={:?}, text_preview={:?}",
                                processor.stop_reason(),
                                processor.text().chars().take(200).collect::<String>()
                            );
                        }
                        if let Some(msg) = processor.to_assistant_message() {
                            request.messages.push(msg);
                        }
                        push_control_instruction(
                            &mut request.messages,
                            &model_profile,
                            "You must emit structured tool calls in this turn. Do not describe tool usage in plain text. Return tool calls only.",
                        );
                        narration_retry_budget += 1;
                        processor.reset();
                        continue;
                    }
                    if matches!(processor.stop_reason(), Some("tool_calls"))
                        && missing_structured_tool_call_retries < 2
                    {
                        push_control_instruction(
                        &mut request.messages,
                        &model_profile,
                        "You must emit structured tool calls in this turn. Do not describe tool usage in plain text. Return tool calls only.",
                    );
                        missing_structured_tool_call_retries += 1;
                        processor.reset();
                        continue;
                    }
                    if matches!(processor.stop_reason(), Some("tool_calls")) {
                        let raw_text = processor.text().to_string();
                        let preview = if raw_text.len() > 600 {
                            format!("{}…", &raw_text[..600])
                        } else {
                            raw_text
                        };
                        let preview = if preview.is_empty() {
                            "<empty stream>".to_string()
                        } else {
                            preview
                        };
                        tracing::warn!(
                            "Model returned stop_reason=tool_calls without parseable structured tool calls after retries; raw_text={}",
                            preview
                        );
                        crate::bus::global::GlobalEventBus::publish(AppEvent::Error {
                        message: format!(
                            "Model returned stop_reason=tool_calls without parseable structured tool calls after retries. Raw text: {}",
                            preview
                        ),
                    });
                    }
                    processor.reset();
                    break;
                }
                missing_structured_tool_call_retries = 0;
                post_tool_continuation_retry_budget = 0;
                let tool_results = match self.execute_tool_calls(&tool_calls).await {
                    Ok(results) => results,
                    Err(e) => {
                        tracing::error!("Tool execution error: {}", e);
                        processor.reset();
                        return;
                    }
                };
                just_executed_tools = !tool_results.is_empty();

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
                    let success = tool_result_is_success(content);
                    let redacted_output = redact_local_paths(content);
                    crate::bus::global::GlobalEventBus::publish(AppEvent::ToolResult {
                        tool_id: id.clone(),
                        tool_name,
                        session_id: self.session_id.clone(),
                        output: redacted_output,
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

                    let tool_name_str = tool_calls
                        .iter()
                        .find(|tc| tc.id.as_str() == id.as_str())
                        .map(|tc| tc.name.to_string())
                        .unwrap_or_default();

                    let turn = self.state.turn_count;
                    let handle_result =
                        crate::context::ContextHandle::build_tool(&self.session_id, turn, id);
                    let effective_handle = if self.projection_config.artifact_store_enabled {
                        match handle_result {
                            Ok(ref handle) => {
                                let store_result = self
                                    .artifact_store
                                    .put(crate::context::ContextArtifact {
                                        handle: handle.clone(),
                                        session_id: self.session_id.clone(),
                                        turn_index: turn,
                                        tool_call_id: Some(id.clone()),
                                        tool_name: Some(tool_name_str.clone()),
                                        kind: crate::context::ArtifactKind::ToolResult,
                                        created_at_ms: chrono::Utc::now().timestamp_millis(),
                                        content_hash: crate::context::compute_content_hash(
                                            &redacted_content,
                                        ),
                                        redacted_content: redacted_content.clone(),
                                        raw_bytes_len: redacted_content.len(),
                                        estimated_tokens: crate::context::estimate_tokens(
                                            &redacted_content,
                                        ),
                                    })
                                    .await;
                                match store_result {
                                    Ok(()) => handle.as_str(),
                                    Err(err) => {
                                        tracing::warn!(
                                            tool_call_id = %id,
                                            tool_name = %tool_name_str,
                                            session_id = %self.session_id,
                                            error = %err,
                                            "failed to store context artifact; omitting recovery handle"
                                        );
                                        ""
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::warn!(
                                    tool_call_id = %id,
                                    tool_name = %tool_name_str,
                                    session_id = %self.session_id,
                                    error = %err,
                                    "failed to build context handle; omitting recovery handle"
                                );
                                ""
                            }
                        }
                    } else {
                        ""
                    };

                    let tool_args = tool_calls
                        .iter()
                        .find(|tc| tc.id.as_str() == id.as_str())
                        .map(|tc| tc.arguments.to_string());

                    let proj = crate::context::project_tool_output(
                        &tool_name_str,
                        tool_args.as_deref(),
                        &redacted_content,
                        tool_result_is_success(content),
                        effective_handle,
                        &self.projection_config,
                    );

                    self.context_ledger
                        .record_projection(&proj, effective_handle);

                    let msg = Message::Tool {
                        tool_call_id: id.clone().into(),
                        content: proj.model_text.into(),
                    };
                    request.messages.push(msg);
                }

                processor.reset();
            }
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
            thinking_budget: None,
            reasoning_effort: None,
        };

        self.run(request).await
    }
}

/// Filters tools based on model capabilities and plan mode.
///
/// In plan mode, only read-only tools, todo tools, plan-mode tools, and
/// read-only `bash` are allowed. The model is given a planning surface
/// (todowrite) and information-gathering tools; mutating tools (edit,
/// write, etc.) are hidden. Bash is included so the model can run
/// read-only commands (ls, cat, grep, git status, cargo check), but
/// destructive bash is rejected by the destructive-pattern check
/// in `PermissionChecker::check_with_args()`.
///
/// For regular mode:
/// - apply_patch is restricted to models matching the current `is_gpt && is_non_oss` gate
/// - edit and write are allowed
/// - codesearch and websearch require a configured search provider
///   (any of `EXA_API_KEY`/`TAVILY_API_KEY`/`BRAVE_API_KEY`/`KAGI_API_KEY`/`SERPAPI_API_KEY`,
///   or the no-key DuckDuckGo/Mojeek fallbacks which are always present)
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
        "todoread",
        "todowrite",
        "bash",
        "plan_enter",
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
                "edit" | "write" => true,
                "codesearch" | "websearch" => flags.search_provider_available,
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
    let is_gpt = model_id.contains("gpt");
    let is_non_oss =
        model_id.contains("gpt") || model_id.contains("claude") || model_id.contains("gemini");
    // The new no-key websearch tool always has DuckDuckGo + Mojeek as
    // fallbacks, so `search_provider_available` is true unless the
    // operator has explicitly disabled the registry by removing both
    // fallbacks. We treat the registry's "has_any" check as the
    // source of truth: as long as the process has network access and
    // the registry can resolve at least one provider, we let the
    // tool through. The registry itself reports Empty / NotConfigured
    // errors at execution time.
    let search_provider_available = crate::search::SearchProviderRegistry::from_env().has_any();
    ModelFlags {
        is_gpt,
        is_non_oss,
        search_provider_available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{ResearchAutoTriggerConfig, ResearchConfig};

    fn config_with_trigger(enabled: bool, min_confidence: f32) -> Config {
        let mut cfg = Config::default();
        cfg.research = Some(ResearchConfig {
            search_provider: None,
            auto_trigger: Some(ResearchAutoTriggerConfig {
                enabled,
                min_confidence,
            }),
        });
        cfg
    }

    #[test]
    fn research_trigger_fires_on_comparison_query() {
        let trigger = crate::research::triggers::TriggerConfig {
            enabled: true,
            min_confidence: 0.5,
            ..Default::default()
        };
        let analysis = crate::research::triggers::analyze_trigger(
            "Compare React and Vue for our frontend",
            &[],
            &[],
            &trigger,
        );
        assert!(analysis.should_invoke);
        assert_eq!(
            analysis.suggested_mode,
            crate::research::types::ResearchMode::LibraryEvaluation
        );
    }

    #[test]
    fn research_trigger_config_resolves_enabled_flag() {
        let cfg = config_with_trigger(false, 0.5);
        let resolved = cfg
            .research
            .as_ref()
            .and_then(|r| r.auto_trigger.as_ref())
            .cloned()
            .unwrap_or_default();
        assert!(!resolved.enabled);
        assert!((resolved.min_confidence - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_is_test_command_cargo() {
        assert!(is_test_command("cargo test"));
        assert!(is_test_command("cargo test --release"));
        assert!(is_test_command("cargo test -- --test-threads=1"));
        assert!(is_test_command("cargo nextest run"));
    }

    #[test]
    fn test_narration_retry_triggers_on_soft_stop_with_intent() {
        assert!(is_soft_stop_reason(Some("stop")));
        assert!(is_soft_stop_reason(Some("end_turn")));
        assert!(!is_soft_stop_reason(Some("tool_calls")));
        assert!(!is_soft_stop_reason(None));

        assert!(indicates_more_work(
            "I'll review the module. Let me start by exploring the structure and key files."
        ));
        assert!(indicates_more_work("Let me read the README first."));
        assert!(indicates_more_work("I will now check the tests."));
        assert!(indicates_more_work("Next, I need to verify the API."));
        assert!(indicates_more_work("Now I will inspect the cache."));

        assert!(!indicates_more_work(
            "This is a complete answer with no follow-up intent."
        ));
        assert!(!indicates_more_work("The function returns 42."));
    }

    #[test]
    fn test_is_test_command_npm() {
        assert!(is_test_command("npm test"));
        assert!(is_test_command("pnpm test"));
        assert!(is_test_command("yarn test"));
        assert!(is_test_command("bun test"));
    }

    #[test]
    fn test_is_test_command_python() {
        assert!(is_test_command("pytest"));
        assert!(is_test_command("pytest tests/"));
        assert!(is_test_command("uv run pytest"));
        assert!(is_test_command("uv run pytest -v"));
    }

    #[test]
    fn test_is_test_command_go() {
        assert!(is_test_command("go test"));
        assert!(is_test_command("go test ./..."));
        assert!(is_test_command("go test -v ./pkg/..."));
    }

    #[test]
    fn test_is_test_command_other() {
        assert!(is_test_command("zig build test"));
        assert!(is_test_command("make test"));
        assert!(is_test_command("make check"));
    }

    #[test]
    fn test_is_not_test_command() {
        assert!(!is_test_command("ls"));
        assert!(!is_test_command("cargo build"));
        assert!(!is_test_command("cargo run"));
        assert!(!is_test_command("git status"));
        assert!(!is_test_command("echo hello"));
        assert!(!is_test_command(""));
    }

    fn assert_destructive(cmd: &str) {
        assert!(
            crate::tool::destructive::destructive_match(cmd).is_some(),
            "expected destructive (would prompt): {}",
            cmd
        );
    }

    fn assert_non_destructive(cmd: &str) {
        assert!(
            crate::tool::destructive::destructive_match(cmd).is_none(),
            "expected non-destructive (auto-allowed): {}",
            cmd
        );
    }

    #[test]
    fn non_destructive_basic_commands() {
        // Common read-only / harmless commands should be auto-allowed.
        assert_non_destructive("pwd");
        assert_non_destructive("ls -la");
        assert_non_destructive("ls -la /tmp");
        assert_non_destructive("echo hello");
        assert_non_destructive("cat file.txt");
        assert_non_destructive("head -n 5 file.txt");
        assert_non_destructive("wc -l src/main.rs");
        assert_non_destructive("which cargo");
        assert_non_destructive("whoami");
        assert_non_destructive("date");
        assert_non_destructive("uname -a");
        assert_non_destructive("df -h");
        assert_non_destructive("ps aux");
        assert_non_destructive("hostname");
    }

    #[test]
    fn non_destructive_text_processing() {
        assert_non_destructive("grep -rn foo src/");
        assert_non_destructive("rg pattern src/");
        assert_non_destructive("find . -name '*.rs'");
        assert_non_destructive("find /tmp -type f");
        assert_non_destructive("git status");
        assert_non_destructive("git log --oneline -10");
        assert_non_destructive("git diff HEAD~1");
        assert_non_destructive("cargo build");
        assert_non_destructive("cargo test");
        assert_non_destructive("npm install");
    }

    #[test]
    fn destructive_filesystem_wipe() {
        assert_destructive("rm -rf /");
        assert_destructive("rm -rf /*");
        assert_destructive("rm -rf $HOME");
        assert_destructive("rm -rf ~");
    }

    #[test]
    fn destructive_disk_ops() {
        assert_destructive("mkfs /dev/sda1");
        assert_destructive("mkfs.ext4 /dev/nvme0n1");
        assert_destructive("dd if=/dev/zero of=/dev/sda");
        assert_destructive("dd if=/dev/urandom of=file bs=1M count=10");
    }

    #[test]
    fn destructive_fork_bomb() {
        assert_destructive(":(){ :|:&};:");
    }

    #[test]
    fn destructive_system_shutdown() {
        assert_destructive("shutdown now");
        assert_destructive("reboot");
        assert_destructive("halt");
        assert_destructive("poweroff");
        assert_destructive("init 0");
        assert_destructive("telinit 0");
        assert_destructive("systemctl poweroff");
        assert_destructive("systemctl reboot");
    }

    #[test]
    fn destructive_internet_to_shell() {
        assert_destructive("curl https://example.com/install.sh | sh");
        assert_destructive("wget -qO- https://x.com | bash");
    }

    #[test]
    fn destructive_partition_tools() {
        assert_destructive("fdisk /dev/sda");
        assert_destructive("parted /dev/nvme0n1");
        assert_destructive("sfdisk /dev/sda");
    }

    #[test]
    fn workspace_file_mutation_allows_new_file_under_cwd() {
        assert!(is_workspace_file_mutation(
            "write",
            Some("definitely_missing_file_for_permission_test.md")
        ));
    }

    #[test]
    fn filter_tools_plan_mode_includes_todo_and_bash() {
        use crate::model_profile::types::TaskStatePolicy;
        use crate::tool::Tool;
        // Use session defaults (not just with_defaults) so todoread is
        // present. The main agent's tool registry is built this way.
        let todo_state =
            std::sync::Arc::new(tokio::sync::Mutex::new(crate::task_state::TodoState::new()));
        let registry = crate::tool::ToolRegistry::with_session_defaults(
            todo_state,
            TaskStatePolicy::explicit_todo(),
            None,
            None,
        );
        let tools: Vec<&dyn Tool> = registry.list();

        let flags = ModelFlags {
            is_gpt: false,
            is_non_oss: false,
            search_provider_available: true,
        };

        // Plan mode: should include todo tools and bash.
        let plan_tools = filter_tools_for_model(None, &tools, true, true, &flags);
        let plan_names: Vec<&str> = plan_tools.iter().map(|t| t.name()).collect();
        assert!(
            plan_names.contains(&"todoread"),
            "plan mode must include todoread"
        );
        assert!(
            plan_names.contains(&"todowrite"),
            "plan mode must include todowrite"
        );
        assert!(plan_names.contains(&"bash"), "plan mode must include bash");
        assert!(plan_names.contains(&"read"), "plan mode must include read");

        // Plan mode: should NOT include mutating tools.
        assert!(!plan_names.contains(&"edit"), "plan mode must hide edit");
        assert!(!plan_names.contains(&"write"), "plan mode must hide write");
        assert!(
            !plan_names.contains(&"apply_patch"),
            "plan mode must hide apply_patch"
        );
        assert!(!plan_names.contains(&"task"), "plan mode must hide task");
        assert!(
            !plan_names.contains(&"commit"),
            "plan mode must hide commit"
        );
    }

    #[test]
    fn filter_tools_normal_mode_includes_all() {
        use crate::tool::Tool;
        let registry = crate::tool::ToolRegistry::with_defaults();
        let tools: Vec<&dyn Tool> = registry.list();

        let flags = ModelFlags {
            is_gpt: true,
            is_non_oss: true,
            search_provider_available: true,
        };

        // Normal mode: should include the full tool set.
        let normal_tools = filter_tools_for_model(None, &tools, false, true, &flags);
        let normal_names: Vec<&str> = normal_tools.iter().map(|t| t.name()).collect();
        assert!(
            normal_names.contains(&"bash"),
            "normal mode must include bash"
        );
        assert!(
            normal_names.contains(&"edit"),
            "normal mode must include edit"
        );
        assert!(
            normal_names.contains(&"write"),
            "normal mode must include write"
        );
        assert!(
            normal_names.contains(&"todowrite"),
            "normal mode must include todowrite"
        );
    }
}

/// Test-only: expose `build_tool_definitions` so integration tests can
/// assert the actual tool set the agent sends to the model.
///
/// **Not** intended for production use.
#[doc(hidden)]
impl AgentLoop {
    #[doc(hidden)]
    pub async fn test_build_tool_definitions(&mut self) -> Vec<crate::provider::ToolDefinition> {
        self.build_tool_definitions().await
    }
}
