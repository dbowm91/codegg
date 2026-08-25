//! Tool permission, execution-context, and batch execution ownership.

use super::r#loop::{
    extract_bash_command, extract_git_subcommand, extract_path_from_tool_call,
    is_file_modifying_tool, is_path_within_workspace, is_test_command, is_workspace_file_mutation,
    parse_mcp_tool_name, truncate_test_event_preview, AgentLoop, ToolPermissionOutcome,
    ToolTimeoutConfig,
};
use crate::agent::progress_recovery::ToolExecutionOutcome;
use crate::bus::events::AppEvent;
use crate::bus::{PermissionDecision, PermissionRegistry, QuestionRegistry};
use crate::error::{AppError, ToolError};
use crate::permission::{PermissionDecisionReceipt, PermissionResult};
use crate::provider::ToolCall;
use crate::tool::question::{format_question_answers, parse_question_questions};
use crate::tool::risk::{classify_tool_risk, summarize_tool_output};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct ToolBatchExecutor<'a> {
    loop_: &'a mut AgentLoop,
}
impl<'a> ToolBatchExecutor<'a> {
    pub(super) fn new(loop_: &'a mut AgentLoop) -> Self {
        Self { loop_ }
    }
    pub(super) async fn execute(
        self,
        tool_calls: &[ToolCall],
    ) -> Result<Vec<(String, ToolExecutionOutcome)>, AppError> {
        self.loop_.execute_tool_calls_impl(tool_calls).await
    }
}

impl AgentLoop {
    pub(super) fn get_tool_timeout(&self, tool_name: &str) -> Duration {
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

    pub(super) fn timeout_for_tool(&self, _tool_name: &str, default: Duration) -> Duration {
        self.config
            .server
            .as_ref()
            .and_then(|s| s.tool_timeout_seconds)
            .map(Duration::from_secs)
            .unwrap_or(default)
    }

    pub(super) fn build_tool_execution_context(
        &self,
        tc: &ToolCall,
        timeout_ms: Option<u64>,
        receipt: &PermissionDecisionReceipt,
    ) -> crate::tool::backend::ToolExecutionContext {
        let backend = self.resolve_native_backend(&tc.name);
        let agent_id = self.state.current_agent.clone();
        crate::tool::backend::ToolExecutionContext {
            backend,
            session_id: Some(self.session_id.clone()),
            // The workspace root is captured during construction and is the
            // sole cwd authority for this loop's tool execution context.
            cwd: self.workspace_root.clone(),
            permission_mode: None,
            timeout_ms,
            invocation_key: Some(format!("{}:{}", self.session_id, tc.id)),
            turn_id: None,
            agent_id: Some(agent_id.clone()),
            parent_job_id: None,
            parent_attempt_id: None,
            provider_name: Some(self.provider.name().to_string()),
            backend_policy: Some("native_only".into()),
            cancellation: None,
            deadline: None,
            // M014-A2: Populate the real accepted decision fields.
            // These are the actual permission/path-policy decision
            // values, not synthesized from identity strings.
            decision_id: Some(receipt.decision_id.clone()),
            decision_outcome: Some(receipt.outcome.as_str().into()),
            workspace_path_policy_id: None,
            workspace_path_policy_revision: None,
            permission_policy_revision: receipt.policy_revision.clone(),
            principal_identity: Some(agent_id),
            caller_class: Some("agent".into()),
            max_effect_class: Some("non_idempotent".into()),
            decision_issued_at: Some(receipt.issued_at),
            decision_expires_at: None,
            decision_revoked_at: None,
            program_contract_snapshot: if tc.name.as_str() == "tool_program" {
                tc.arguments
                    .get("tools")
                    .and_then(serde_json::Value::as_array)
                    .map(|tools| {
                        tools
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .and_then(|tools| {
                        crate::tool::tool_program_context::resolve_contract_snapshot(
                            &self.tool_broker,
                            &tools,
                        )
                        .ok()
                    })
            } else {
                None
            },
        }
    }

    pub(super) fn resolve_native_backend(
        &self,
        tool_name: &str,
    ) -> crate::tool::backend::ToolBackendKind {
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

    pub(super) fn accepted_permission_receipt(&self, source: &str) -> PermissionDecisionReceipt {
        // This is a content fingerprint of the configured permission policy,
        // not a fabricated workspace/session revision. Session decisions are
        // represented by the receipt source and decision id.
        PermissionDecisionReceipt::allowed(
            source,
            Some(format!("config:{:016x}", self.permission_version())),
        )
    }

    pub(super) async fn check_tool_permission(&mut self, tc: &ToolCall) -> ToolPermissionOutcome {
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
                if let Some(sensitive) = sensitive_match {
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
                    PermissionRegistry::unregister_scoped(&self.session_id, &perm_id);
                    if choice.allowed() {
                        ToolPermissionOutcome::Allowed {
                            tool_call: tc.clone(),
                            receipt: self.accepted_permission_receipt("user_choice"),
                        }
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
                    PermissionRegistry::unregister_scoped(&self.session_id, &perm_id);
                    if choice.allowed() {
                        ToolPermissionOutcome::Allowed {
                            tool_call: tc.clone(),
                            receipt: self.accepted_permission_receipt("user_choice"),
                        }
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
                    ToolPermissionOutcome::Allowed {
                        tool_call: tc.clone(),
                        receipt: self.accepted_permission_receipt("permission_evaluation"),
                    }
                }
            }
            PermissionResult::Deny => ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message: format!("Tool '{}' denied by permissions", tc.name),
            },
            PermissionResult::Ask(req) => {
                // Preserve the narrow local-file UX exception. External MCP
                // origin is never evidence that an unknown tool is safe.
                if is_workspace_file_mutation(
                    tc.name.as_str(),
                    req.path.as_deref(),
                    &self.workspace_root,
                ) && is_path_within_workspace(req.path.as_deref(), &self.workspace_root)
                    && sensitive_match.is_none()
                {
                    return ToolPermissionOutcome::Allowed {
                        tool_call: tc.clone(),
                        receipt: self.accepted_permission_receipt("workspace_file_mutation"),
                    };
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
                PermissionRegistry::unregister_scoped(&self.session_id, &perm_id);
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
                    ToolPermissionOutcome::Allowed {
                        tool_call: tc.clone(),
                        receipt: self.accepted_permission_receipt("user_choice"),
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

    #[allow(clippy::incompatible_msrv)]
    pub(super) async fn execute_tool_calls_impl(
        &mut self,
        tool_calls: &[ToolCall],
    ) -> Result<Vec<(String, ToolExecutionOutcome)>, AppError> {
        let mut tool_results = Vec::with_capacity(16);
        let mut has_pending_question = false;

        let mut allowed_tools = Vec::with_capacity(tool_calls.len());
        for (idx, tc) in tool_calls.iter().enumerate() {
            match self.check_tool_permission(tc).await {
                ToolPermissionOutcome::QuestionTool => {
                    has_pending_question = true;
                    tool_results.push((
                        idx,
                        tc.id.to_string(),
                        ToolExecutionOutcome::success("__QUESTION_PENDING__"),
                    ));
                }
                ToolPermissionOutcome::Allowed { tool_call, receipt } => {
                    allowed_tools.push((idx, tool_call, receipt));
                }
                ToolPermissionOutcome::Denied { tool_id, message } => {
                    let outcome = if message.starts_with("Error: invalid tool call") {
                        ToolExecutionOutcome {
                            status: crate::agent::progress_recovery::ToolExecutionStatus::ToolError,
                            model_text: message,
                        }
                    } else {
                        ToolExecutionOutcome {
                            status: crate::agent::progress_recovery::ToolExecutionStatus::Denied,
                            model_text: message,
                        }
                    };
                    tool_results.push((idx, tool_id, outcome));
                }
            }
        }

        // Capture snapshot before executing file-modifying tools
        let has_file_modifying = allowed_tools
            .iter()
            .any(|(_, tc, _)| is_file_modifying_tool(&tc.name));
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
            .filter(|(idx, tc, _)| {
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
                                            return (
                                                orig_idx,
                                                tc.id.to_string(),
                                                ToolExecutionOutcome::success(result),
                                            );
                                        }
                                        Ok(Err(e)) => {
                                            return (
                                                orig_idx,
                                                tc.id.to_string(),
                                                ToolExecutionOutcome {
                                                    status: crate::agent::progress_recovery::ToolExecutionStatus::ToolError,
                                                    model_text: format!("Error: {}", e),
                                                },
                                            );
                                        }
                                        Err(_) => {
                                            return (
                                                orig_idx,
                                                tc.id.to_string(),
                                                ToolExecutionOutcome {
                                                    status: crate::agent::progress_recovery::ToolExecutionStatus::Timeout,
                                                    model_text: format!(
                                                        "Error: MCP tool '{}' on server '{}' timed out after {:?}",
                                                        tool, server, mcp_timeout
                                                    ),
                                                },
                                            );
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
                        (
                            orig_idx,
                            tc.id.to_string(),
                            ToolExecutionOutcome {
                                status: crate::agent::progress_recovery::ToolExecutionStatus::ToolError,
                                model_text: format!("Error: {}", last_err.unwrap_or_default()),
                            },
                        )
                    } else {
                        (
                            orig_idx,
                            tc.id.to_string(),
                            ToolExecutionOutcome {
                                status: crate::agent::progress_recovery::ToolExecutionStatus::ToolError,
                                model_text: "Error: MCP service not available".into(),
                            },
                        )
                    }
                } else {
                    (
                        orig_idx,
                        tc.id.to_string(),
                        ToolExecutionOutcome {
                            status: crate::agent::progress_recovery::ToolExecutionStatus::ProtocolError,
                            model_text: format!("Error: Invalid MCP tool name '{}'", name),
                        },
                    )
                }
            });
        }
        let mcp_results = futures_util::future::join_all(mcp_futures).await;
        for result in mcp_results {
            tool_results.push(result);
        }

        let mut results = Vec::with_capacity(regular_tool_count);
        let sem = Arc::new(tokio::sync::Semaphore::new(effective_max));
        let mut futures = Vec::with_capacity(regular_tool_count);
        let hook_registry = self.hook_registry.as_ref().map(Arc::clone);
        let plugin_service = self.plugin_service.as_ref().map(Arc::clone);
        let event_store = self.event_store.clone();
        let tool_broker = Arc::clone(&self.tool_broker);
        let authority_ref = {
            // M012-F01: Derive authority from the agent's real identity.
            // The agent_id is the current agent's name (e.g. "code", "plan"),
            // replacing the legacy synthetic session-based format.
            let agent_id = &self.state.current_agent;
            format!("agent:{}", agent_id)
        };
        // M012-F01: Derive workspace identity from the workspace root path.
        let agent_workspace_id = {
            use sha2::Digest;
            format!(
                "ws:{:x}",
                sha2::Sha256::digest(self.workspace_root.to_string_lossy().as_bytes())
            )
        };
        let agent_id = self.state.current_agent.clone();
        for (orig_idx, tc, receipt) in regular_tools {
            // Build the structured-execution context here (before
            // `tc` is moved into an Arc) so the helper, which takes
            // `&self`, can read live state without forcing the
            // `async move` closure to capture `self` by move.
            let tool_name_for_ctx = tc.name.clone();
            let timeout = self.get_tool_timeout(&tool_name_for_ctx);
            let exec_ctx =
                self.build_tool_execution_context(&tc, Some(timeout.as_millis() as u64), &receipt);
            let tc_arc = Arc::new(tc);
            let sem = Arc::clone(&sem);
            let id = tc_arc.id.clone();
            let tool_name = tc_arc.name.clone();
            let hook_registry = hook_registry.clone();
            let plugin_service = plugin_service.clone();
            let session_id = self.session_id.clone();
            let authority_ref = authority_ref.clone();
            let agent_workspace_id = agent_workspace_id.clone();
            let agent_id = agent_id.clone();
            let idx_for_results = orig_idx;
            let event_store = event_store.clone();
            let tool_broker = Arc::clone(&tool_broker);
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

                let mut effective_args = tc_arc.arguments.clone();
                if let Some(ref ps) = plugin_service {
                    use crate::plugin::lifecycle::{
                        LifecycleHooks, PluginHookOutcome, ToolBeforeAction, ToolBeforeHookInput,
                    };
                    let risk = classify_tool_risk(&tool_name, &tc_arc.arguments);
                    let lifecycle_hooks = LifecycleHooks::new(
                        ps.clone(),
                        crate::plugin::policy::PluginLifecyclePolicy::default(),
                    );
                    let before_input = ToolBeforeHookInput {
                        tool_name: tool_name.to_string(),
                        tool_call_id: id.to_string(),
                        args: tc_arc.arguments.clone(),
                        session_id: session_id.clone(),
                        risk: risk.to_string(),
                    };
                    match lifecycle_hooks.before_tool_execute(before_input).await {
                        PluginHookOutcome::Ok(output, effects) => {
                            match output.action {
                                ToolBeforeAction::Deny => {
                                    tracing::warn!(
                                        tool = %tool_name,
                                        reason = output.reason.as_deref().unwrap_or("no reason"),
                                        "Tool execution denied by plugin hook"
                                    );
                                    drop(permit);
                                    return (
                                        idx_for_results,
                                        id,
                                        Err(ToolError::Execution(format!(
                                            "blocked by plugin: {}",
                                            output.reason.unwrap_or_default()
                                        ))),
                                    );
                                }
                                ToolBeforeAction::Modify => {
                                    if let Some(new_args) = output.args {
                                        tracing::debug!(
                                            tool = %tool_name,
                                            "Plugin modified tool arguments"
                                        );
                                        effective_args = new_args;
                                    }
                                }
                                ToolBeforeAction::Allow => {}
                            }
                            for effect in effects {
                                crate::bus::global::GlobalEventBus::publish(
                                    crate::bus::events::AppEvent::PluginUiEffect {
                                        session_id: Some(session_id.clone()),
                                        plugin_id: "lifecycle".into(),
                                        invocation_id: None,
                                        effect,
                                    },
                                );
                            }
                        }
                        PluginHookOutcome::Blocked { reason } => {
                            tracing::warn!(
                                tool = %tool_name,
                                reason = reason.as_deref().unwrap_or("no reason"),
                                "Tool execution blocked by plugin hook"
                            );
                            drop(permit);
                            return (
                                idx_for_results,
                                id,
                                Err(ToolError::Execution(format!(
                                    "blocked by plugin: {}",
                                    reason.unwrap_or_default()
                                ))),
                            );
                        }
                        PluginHookOutcome::Failed { error } => {
                            tracing::warn!(
                                tool = %tool_name,
                                error = %error,
                                "Before-tool hook failed"
                            );
                        }
                        PluginHookOutcome::Skipped => {}
                    }
                }

                let tool_start = Instant::now();
                let risk = classify_tool_risk(&tool_name, &effective_args);
                {
                    let meta = crate::session::events::EventMeta::new(&session_id);
                    let event = crate::session::events::SessionEvent::ToolCallStarted(
                        crate::session::events::ToolCallStartedEvent {
                            meta,
                            tool_call_id: id.to_string(),
                            tool_name: tool_name.to_string(),
                            arguments: effective_args.to_string(),
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
                            let exec_args = effective_args.clone();
                            let tool_name_clone = tc_inner.name.clone();
                            let broker_for_exec = Arc::clone(&tool_broker);
                            let authority_ref = authority_ref.clone();
                            let agent_workspace_id = agent_workspace_id.clone();
                            let agent_id = agent_id.clone();
                            let exec_fut = async move {
                                // M014-A2: Build manifest digest from the tool name.
                                // For AgentLoop direct calls, the manifest is the
                                // single tool being invoked.
                                let manifest_digest = {
                                    use sha2::Digest;
                                    format!(
                                        "sha256:{:x}",
                                        sha2::Sha256::digest(tool_name_clone.as_bytes())
                                    )
                                };
                                // M014-A2: Use the real decision fields from the
                                // execution context rather than synthesizing
                                // authority from identity strings.
                                let now = chrono::Utc::now().timestamp_millis();
                                let principal_ref = exec_ctx
                                    .principal_identity
                                    .clone()
                                    .unwrap_or_else(|| authority_ref.clone());
                                let workspace_path_policy_id = exec_ctx
                                    .workspace_path_policy_id
                                    .clone()
                                    .unwrap_or_else(|| format!("workspace:{}", agent_workspace_id));
                                let policy_revision = exec_ctx
                                    .permission_policy_revision
                                    .clone()
                                    .or_else(|| exec_ctx.workspace_path_policy_revision.clone())
                                    .unwrap_or_else(|| {
                                        format!(
                                            "agent:{}:{}",
                                            agent_id,
                                            exec_ctx.session_id.as_deref().unwrap_or("anon")
                                        )
                                    });
                                let policy_revision_for_ctx = policy_revision.clone();
                                let ws_id = agent_workspace_id.clone();
                                let ws_id_for_ctx = agent_workspace_id.clone();
                                let grant = codegg_core::jobs::ToolAuthorityGrant {
                                    schema_version: 1,
                                    grant_id: exec_ctx
                                        .decision_id
                                        .clone()
                                        .unwrap_or_else(|| authority_ref.clone()),
                                    principal_ref: principal_ref.clone(),
                                    workspace_id: ws_id,
                                    workspace_path_policy_id: workspace_path_policy_id.clone(),
                                    session_id: exec_ctx.session_id.clone(),
                                    agent_id: Some(agent_id.clone()),
                                    turn_id: exec_ctx.turn_id.clone(),
                                    permission_mode: exec_ctx.permission_mode.clone(),
                                    policy_revision,
                                    allowed_caller_class: exec_ctx
                                        .caller_class
                                        .clone()
                                        .unwrap_or_else(|| "agent".into()),
                                    allowed_effect_class: exec_ctx
                                        .max_effect_class
                                        .clone()
                                        .unwrap_or_else(|| "non_idempotent".into()),
                                    manifest_digest,
                                    source_digest: String::new(),
                                    ir_digest: String::new(),
                                    contract_digest: String::new(),
                                    contract_snapshot_json: String::new(),
                                    issued_at: exec_ctx.decision_issued_at.unwrap_or(now),
                                    expires_at: exec_ctx.decision_expires_at,
                                    revoked_at: exec_ctx.decision_revoked_at,
                                    decision_digest: String::new(),
                                };
                                let decision_digest = grant.compute_digest();
                                let grant = codegg_core::jobs::ToolAuthorityGrant {
                                    decision_digest,
                                    ..grant
                                };
                                let broker_ctx = crate::tool::broker::BrokerInvocationContext {
                                    caller: crate::tool::contract::ToolCaller::Agent,
                                    cwd: exec_ctx.cwd.clone(),
                                    session_id: exec_ctx.session_id.clone(),
                                    workspace_id: Some(ws_id_for_ctx.clone()),
                                    agent_id: Some(agent_id.clone()),
                                    turn_id: exec_ctx.turn_id.clone(),
                                    job_id: None,
                                    attempt_id: None,
                                    permission_mode: exec_ctx.permission_mode.clone(),
                                    timeout_ms: exec_ctx.timeout_ms,
                                    submission_key: None,
                                    authority: crate::tool::broker::BrokerAuthority::from_grant(
                                        grant,
                                    ),
                                    cancellation: exec_ctx.cancellation.clone(),
                                    deadline: exec_ctx.deadline,
                                    // Bind the broker context to the same
                                    // principal used to issue the grant. The
                                    // decision identity is not a principal.
                                    principal_ref: Some(principal_ref.clone()),
                                    workspace_path_policy_id: Some(format!(
                                        "workspace:{}",
                                        ws_id_for_ctx
                                    )),
                                    allowed_tools: None,
                                    current_policy_revision: Some(policy_revision_for_ctx),
                                };
                                let broker_result = broker_for_exec
                                    .execute(registry, &tool_name_clone, exec_args, broker_ctx)
                                    .await
                                    .map_err(|e| match e {
                                        crate::tool::broker::BrokerError::NotFound(name) => {
                                            ToolError::NotFound(name)
                                        }
                                        crate::tool::broker::BrokerError::NoContract(name) => {
                                            ToolError::NotFound(name)
                                        }
                                        crate::tool::broker::BrokerError::CallerDenied {
                                            tool,
                                            ..
                                        } => ToolError::Permission(format!(
                                            "caller denied for tool: {}",
                                            tool
                                        )),
                                        crate::tool::broker::BrokerError::InputTooLarge {
                                            tool,
                                            size,
                                            max,
                                        } => ToolError::Execution(format!(
                                            "input for {} is {} bytes, max is {}",
                                            tool, size, max
                                        )),
                                        crate::tool::broker::BrokerError::Execution(msg) => {
                                            ToolError::Execution(msg)
                                        }
                                        crate::tool::broker::BrokerError::AuthorityError {
                                            tool,
                                            reason,
                                        } => ToolError::Permission(format!(
                                            "authority error for tool {}: {}",
                                            tool, reason
                                        )),
                                    })?;
                                if let Some(ref p) = broker_result.value.provenance {
                                    tracing::debug!(
                                        tool = %tool_name_clone,
                                        backend = %p.backend,
                                        implementation = %p.implementation,
                                        elapsed_ms = ?p.elapsed_ms,
                                        trust = ?p.trust,
                                        "broker: native tool completed with provenance"
                                    );
                                }
                                Ok::<String, ToolError>(broker_result.value.display)
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
                                    last_result = Err(ToolError::Timeout(format!(
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
                    use crate::plugin::lifecycle::{
                        LifecycleHooks, PluginHookOutcome, ToolAfterHookInput,
                    };
                    let duration_ms = tool_start.elapsed().as_millis() as u64;
                    let lifecycle_hooks = LifecycleHooks::new(
                        ps.clone(),
                        crate::plugin::policy::PluginLifecyclePolicy::default(),
                    );
                    let after_input = ToolAfterHookInput {
                        tool_name: tool_name.to_string(),
                        tool_call_id: id.to_string(),
                        args: effective_args.clone(),
                        success: result.is_ok(),
                        output: result
                            .as_ref()
                            .ok()
                            .map(|o| {
                                if o.len() > 500 {
                                    format!("{}...", &o[..497])
                                } else {
                                    o.clone()
                                }
                            })
                            .unwrap_or_default(),
                        duration_ms,
                    };
                    if let PluginHookOutcome::Failed { error } =
                        lifecycle_hooks.after_tool_execute(after_input).await
                    {
                        tracing::warn!(tool = %tool_name, error = %error, "After-tool hook failed");
                    }
                }

                let post_ctx = crate::hooks::HookContext {
                    event: crate::hooks::HookEvent::PostToolExecute,
                    session_id: Some(session_id.clone()),
                    tool_name: Some(tool_name.to_string()),
                    tool_arguments: Some(effective_args.clone()),
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
                                    let preview = truncate_test_event_preview(&test_output, 200);
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
        let all_results = futures_util::future::join_all(futures).await;
        results.extend(all_results);

        const MAX_TOOL_RESULT_BYTES_FALLBACK: usize = 512 * 1024; // 512KB per tool result
        let max_tool_result_bytes = self
            .execution_policy
            .as_ref()
            .map_or(MAX_TOOL_RESULT_BYTES_FALLBACK, |p| {
                p.max_tool_result_tokens * 4
            });
        for (idx, id, result) in results {
            let mut outcome = match result {
                Ok(output) => ToolExecutionOutcome::success(output),
                Err(error) => ToolExecutionOutcome::from_tool_error(error),
            };
            let output = &outcome.model_text;
            if output.len() > max_tool_result_bytes {
                let safe_end = output.floor_char_boundary(max_tool_result_bytes);
                let mut truncated = output[..safe_end].to_string();
                truncated.push_str(&format!(
                    "\n... [truncated: output was {} bytes, limit is {} bytes]",
                    output.len(),
                    max_tool_result_bytes
                ));
                outcome.model_text = truncated;
            }
            tool_results.push((idx, id.to_string(), outcome));
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
                            .map(|(idx, id, mut outcome)| {
                                if outcome.model_text == "__QUESTION_PENDING__" {
                                    outcome.model_text = formatted.clone();
                                } else {
                                    return (idx, id, outcome);
                                }
                                (idx, id, outcome)
                            })
                            .collect();
                    }
                    Ok(Err(_)) => {
                        tool_results = tool_results
                            .into_iter()
                            .map(|(idx, id, mut outcome)| {
                                if outcome.model_text == "__QUESTION_PENDING__" {
                                    outcome.status = crate::agent::progress_recovery::ToolExecutionStatus::Cancelled;
                                    outcome.model_text = "[question cancelled by user]".to_string();
                                } else {
                                    return (idx, id, outcome);
                                }
                                (idx, id, outcome)
                            })
                            .collect();
                    }
                    Err(_) => {
                        tool_results = tool_results
                            .into_iter()
                            .map(|(idx, id, mut outcome)| {
                                if outcome.model_text == "__QUESTION_PENDING__" {
                                    outcome.status = crate::agent::progress_recovery::ToolExecutionStatus::Timeout;
                                    outcome.model_text =
                                        "[question timed out waiting for user response]".to_string();
                                } else {
                                    return (idx, id, outcome);
                                }
                                (idx, id, outcome)
                            })
                            .collect();
                    }
                }
                QuestionRegistry::unregister(&self.session_id);
            } else {
                tool_results = tool_results
                    .into_iter()
                    .map(|(idx, id, mut outcome)| {
                        if outcome.model_text == "__QUESTION_PENDING__" {
                            outcome.status =
                                crate::agent::progress_recovery::ToolExecutionStatus::ToolError;
                            outcome.model_text =
                                "[question not supported in exec mode]".to_string();
                        } else {
                            return (idx, id, outcome);
                        }
                        (idx, id, outcome)
                    })
                    .collect();
            }
        }

        tool_results.sort_by_key(|(idx, _, _)| *idx);
        let ordered_results: Vec<(String, ToolExecutionOutcome)> = tool_results
            .into_iter()
            .map(|(_, id, outcome)| (id, outcome))
            .collect();

        Ok(ordered_results)
    }
}
