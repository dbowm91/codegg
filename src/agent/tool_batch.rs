//! Tool permission, execution-context, and batch execution ownership.

use super::r#loop::{
    extract_bash_command, extract_git_subcommand, extract_path_from_tool_call,
    is_path_within_workspace, is_test_command, is_workspace_file_mutation, parse_mcp_tool_name,
    truncate_test_event_preview, AgentLoop, ToolPermissionOutcome, ToolTimeoutConfig,
};
use crate::agent::progress_recovery::ToolExecutionOutcome;
use crate::bus::events::AppEvent;
use crate::bus::QuestionRegistry;
use crate::error::{AppError, ToolError};
use crate::permission::approval::{
    source as approval_source, ApprovalDecision, ApprovalMode, ApprovalRequest, ApprovalRouter,
    DeterministicVerdict, ExecutionPolicySnapshot,
};
use crate::permission::reviewer as approval_reviewer;
use crate::permission::{PermissionDecisionReceipt, PermissionResult};
use crate::provider::ToolCall;
use crate::tool::question::{format_question_answers, parse_question_questions};
use crate::tool::risk::{classify_tool_risk, summarize_tool_output};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct CheckpointContext {
    paths: Vec<String>,
    pre_states: std::collections::HashMap<String, crate::snapshot::checkpoint::FileState>,
    workspace_id: String,
    session_id: String,
    turn_id: Option<String>,
    batch_seq: i64,
}

fn is_affirmatively_read_only_tool(tc: &ToolCall) -> bool {
    is_affirmatively_read_only_name(&tc.name, &tc.arguments)
}

fn is_affirmatively_read_only_name(name: &str, input: &serde_json::Value) -> bool {
    matches!(
        classify_tool_risk(name, input),
        crate::session::events::ToolRisk::Read
    )
}

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
        self.services
            .config
            .server
            .as_ref()
            .and_then(|s| s.tool_timeout_seconds)
            .map(Duration::from_secs)
            .unwrap_or(default)
    }

    pub(super) fn build_tool_execution_context(
        &self,
        tc: &ToolCall,
        accepted_call_ordinal: usize,
        timeout_ms: Option<u64>,
        receipt: &PermissionDecisionReceipt,
        snapshot: &ExecutionPolicySnapshot,
    ) -> crate::tool::backend::ToolExecutionContext {
        let backend = self.resolve_native_backend(&tc.name);
        let agent_id = self.state.current_agent.clone();
        let invocation_key = invocation_key_for(
            &self.session_id,
            self.turn_id.as_deref(),
            self.run_id.as_ref(),
            self.state.turn_count,
            tc.id.as_str(),
            accepted_call_ordinal,
        );
        crate::tool::backend::ToolExecutionContext {
            backend,
            session_id: Some(self.session_id.clone()),
            // The workspace root is captured during construction and is the
            // sole cwd authority for this loop's tool execution context.
            cwd: self.workspace_root.clone(),
            // M003: durable approval mode from the captured batch snapshot,
            // never `None` on production construction paths.
            permission_mode: Some(snapshot.approval_mode().as_str().to_owned()),
            // M005: parent sandbox ceiling for subagent construction.
            // Approval mode never mutates this value.
            sandbox_profile: Some(snapshot.sandbox_profile().as_str().to_owned()),
            timeout_ms,
            invocation_key: Some(invocation_key),
            turn_id: self.turn_id.clone(),
            agent_id: Some(agent_id.clone()),
            parent_job_id: None,
            parent_attempt_id: None,
            provider_name: Some(self.services.provider.name().to_string()),
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
            // M003 origin: stamped by the agent loop via `apply_origin`
            // once turn attribution is threaded through (M005).
            origin_principal: None,
            origin_auth_method: None,
            origin_decision_id: None,
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
                            &self.services.tool_broker,
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
            // Explicit loop-owned search context (M005): no
            // process-global lookup at execution time.
            match self.services.search_runtime.backend() {
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

    #[allow(dead_code)]
    pub(super) async fn check_tool_permission(&mut self, tc: &ToolCall) -> ToolPermissionOutcome {
        // Capture the immutable batch snapshot at evaluation entry so a
        // concurrent mode toggle cannot retroactively bless this pending
        // action. Batch-level callers capture once per batch and use the
        // `_with_snapshot` entrypoint directly.
        let snapshot = self.capture_execution_snapshot();
        self.check_tool_permission_with_snapshot(tc, &snapshot)
            .await
    }

    /// M003: single production escalation owner. All deterministic
    /// `Ask`/security/sensitive branches normalize into one
    /// [`ApprovalRouter`] decision; no call site registers
    /// `PermissionPending` directly (see `scripts/check_approval_router.py`).
    pub(super) async fn check_tool_permission_with_snapshot(
        &mut self,
        tc: &ToolCall,
        snapshot: &ExecutionPolicySnapshot,
    ) -> ToolPermissionOutcome {
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
            self.services
                .permission_checker
                .check_bash(
                    path.as_deref(),
                    bash_command.as_deref(),
                    Some(&self.session_id),
                )
                .await
        } else if git_subcommand.is_some() {
            self.services
                .permission_checker
                .check_git(
                    path.as_deref(),
                    git_subcommand.as_deref(),
                    Some(&self.session_id),
                )
                .await
        } else {
            self.services
                .permission_checker
                .check(&tc.name, path.as_deref(), Some(&self.session_id))
                .await
        };
        let security_hint = if !self.services.security_service.enabled() {
            crate::security::policy::SecurityDecisionHint {
                action: crate::security::policy::SecurityAction::Observe,
                reason: String::new(),
                finding: None,
            }
        } else if let Some(ref cmd) = bash_command {
            self.services.security_service.classify_bash(cmd)
        } else if let Some(ref subcommand) = git_subcommand {
            self.services.security_service.classify_git(subcommand)
        } else {
            self.services
                .security_service
                .classify_tool_call(&tc.name, &tc.arguments)
        };
        if let Some(ref finding) = security_hint.finding {
            self.recent_findings.push(finding.clone());
        }
        // Sensitive paths escalate regardless of permission level, but never
        // override an explicit deny (deny is normalized first below).
        let sensitive_match = self.services.config.security.as_ref().and_then(|sec| {
            crate::security::matches_sensitive_path(path.as_deref(), &sec.sensitive_paths)
        });

        let policy_revision = Some(format!("config:{:016x}", self.permission_version()));
        let router = ApprovalRouter::new(snapshot.clone());

        // ── Normalize deterministic policy + security into one verdict ──
        // Hard deny and authority ceilings precede routing: Deny never
        // becomes Escalate/Yolo Allow.
        let verdict = match perm_result {
            PermissionResult::Deny => DeterministicVerdict::Deny {
                reason: format!("Tool '{}' denied by permissions", tc.name),
                source: approval_source::PERMISSION_DENY.to_owned(),
            },
            PermissionResult::Allow | PermissionResult::Ask(_) => {
                if matches!(
                    security_hint.action,
                    crate::security::policy::SecurityAction::Deny
                ) {
                    DeterministicVerdict::Deny {
                        reason: format!(
                            "Tool '{}' denied by security policy: {}",
                            tc.name, security_hint.reason
                        ),
                        source: approval_source::SECURITY_DENY.to_owned(),
                    }
                } else if let Some(sensitive) = sensitive_match.as_ref() {
                    let reason = sensitive
                        .reason
                        .clone()
                        .unwrap_or_else(|| "sensitive path".to_string());
                    let request = ApprovalRequest::new(
                        tc.name.to_string(),
                        path.clone(),
                        bash_command.clone().map(|c| truncate_for_audit(&c)),
                        vec![format!("sensitive path: {reason}")],
                        Some(format!(
                            "review_level:{}",
                            sensitive.review_level.as_deref().unwrap_or("standard")
                        )),
                        policy_revision.clone(),
                        self.session_id.clone(),
                        self.turn_id.clone(),
                    );
                    DeterministicVerdict::Escalate { request }
                } else if matches!(
                    security_hint.action,
                    crate::security::policy::SecurityAction::Ask
                ) {
                    let request = ApprovalRequest::new(
                        tc.name.to_string(),
                        path.clone(),
                        bash_command.clone().map(|c| truncate_for_audit(&c)),
                        vec![format!("security escalation: {}", security_hint.reason)],
                        security_hint
                            .finding
                            .as_ref()
                            .map(|f| format!("{:?}", f.category)),
                        policy_revision.clone(),
                        self.session_id.clone(),
                        self.turn_id.clone(),
                    );
                    DeterministicVerdict::Escalate { request }
                } else if let PermissionResult::Ask(req) = perm_result {
                    // Preserve the narrow local-file UX exception. External
                    // MCP origin is never evidence that an unknown tool is
                    // safe.
                    if is_workspace_file_mutation(
                        tc.name.as_str(),
                        req.path.as_deref(),
                        &self.workspace_root,
                    ) && is_path_within_workspace(req.path.as_deref(), &self.workspace_root)
                        && sensitive_match.is_none()
                    {
                        return ToolPermissionOutcome::Allowed {
                            tool_call: tc.clone(),
                            receipt: self.accepted_permission_receipt(
                                approval_source::WORKSPACE_FILE_MUTATION,
                            ),
                        };
                    }
                    let dialog_args = req.args.clone();
                    let request = ApprovalRequest::new(
                        req.tool.clone(),
                        req.path.clone(),
                        req.args.clone().map(|a| truncate_for_audit(&a.to_string())),
                        vec!["permission policy ask".to_owned()],
                        None,
                        policy_revision.clone(),
                        self.session_id.clone(),
                        self.turn_id.clone(),
                    );
                    // Stash the original dialog payload alongside the
                    // normalized request via the escalation path below.
                    // The router carries only the bounded summary; the
                    // human dialog below uses the original args.
                    return self
                        .resolve_general_ask_via_human(tc, &request, dialog_args, &router, snapshot)
                        .await;
                } else {
                    return ToolPermissionOutcome::Allowed {
                        tool_call: tc.clone(),
                        receipt: self
                            .accepted_permission_receipt(approval_source::PERMISSION_EVALUATION),
                    };
                }
            }
        };

        // ── Route through the single ApprovalRouter ──
        match verdict {
            DeterministicVerdict::Allow => ToolPermissionOutcome::Allowed {
                tool_call: tc.clone(),
                receipt: self.accepted_permission_receipt(approval_source::PERMISSION_EVALUATION),
            },
            DeterministicVerdict::Deny { reason, .. } => ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message: reason,
            },
            DeterministicVerdict::Escalate { request } => {
                match snapshot.approval_mode() {
                    ApprovalMode::Yolo => {
                        // Yolo auto-allows only Escalate within the already
                        // resolved ceiling; Deny never reaches here.
                        ToolPermissionOutcome::Allowed {
                            tool_call: tc.clone(),
                            receipt: self.accepted_permission_receipt(approval_source::YOLO),
                        }
                    }
                    ApprovalMode::Automatic => {
                        // M006: resolve through the bounded read-only
                        // reviewer when configured; otherwise safely defer
                        // to the human (M003 behavior preserved). Never
                        // auto-allow without a reviewer verdict.
                        self.resolve_automatic_escalation(tc, &request, &router, snapshot, None)
                            .await
                    }
                    ApprovalMode::Interactive => {
                        self.resolve_escalation_via_human(
                            tc,
                            &request,
                            &router,
                            approval_source::USER_CHOICE,
                        )
                        .await
                    }
                }
            }
        }
    }

    /// Single human-wait path for Interactive and Automatic-fallback
    /// escalations. Preserves reason metadata in the dialog payload,
    /// persists `Always` choices with visible failure diagnostics, and
    /// records timeout distinctly from explicit deny.
    async fn resolve_escalation_via_human(
        &mut self,
        tc: &ToolCall,
        request: &ApprovalRequest,
        router: &ApprovalRouter,
        success_source: &str,
    ) -> ToolPermissionOutcome {
        let perm_id = format!("{}-{}", tc.id, tc.name);
        // Rebuild the dialog args with escalation reasons preserved. Raw
        // sensitive args stay out; only bounded summaries cross the bus.
        let args = serde_json::json!({
            "command": request.args_summary.clone().unwrap_or_default(),
            "escalation_reasons": request.escalation_reasons,
            "effect": request.effect_metadata,
        });
        let outcome = router
            .request_human_approval(&perm_id, request, Some(args))
            .await;
        self.apply_human_outcome(tc, request, outcome, success_source)
            .await
    }

    /// Human-wait path for general policy `Ask` that preserves the
    /// original dialog args payload (bounded by the checker) alongside the
    /// normalized redacted summary.
    async fn resolve_general_ask_via_human(
        &mut self,
        tc: &ToolCall,
        request: &ApprovalRequest,
        dialog_args: Option<serde_json::Value>,
        router: &ApprovalRouter,
        snapshot: &ExecutionPolicySnapshot,
    ) -> ToolPermissionOutcome {
        match snapshot.approval_mode() {
            ApprovalMode::Yolo => {
                return ToolPermissionOutcome::Allowed {
                    tool_call: tc.clone(),
                    receipt: self.accepted_permission_receipt(approval_source::YOLO),
                };
            }
            ApprovalMode::Automatic => {
                // M006: reviewer first (preserving the original dialog
                // payload for the human fallback); unconfigured reviewer
                // falls back to the human, never auto-allows.
                return self
                    .resolve_automatic_escalation(tc, request, router, snapshot, Some(dialog_args))
                    .await;
            }
            ApprovalMode::Interactive => {}
        }
        let perm_id = format!("{}-{}", tc.id, tc.name);
        let outcome = router
            .request_human_approval(&perm_id, request, dialog_args)
            .await;
        self.apply_human_outcome(tc, request, outcome, approval_source::USER_CHOICE)
            .await
    }

    /// Apply one human verdict: persist `Always` choices with visible
    /// diagnostics, map timeout vs explicit deny, and preserve
    /// sensitive/security denial messages.
    async fn apply_human_outcome(
        &mut self,
        tc: &ToolCall,
        request: &ApprovalRequest,
        outcome: crate::permission::approval::HumanApprovalOutcome,
        success_source: &str,
    ) -> ToolPermissionOutcome {
        use crate::permission::approval::HumanApprovalOutcome;
        let HumanApprovalOutcome {
            decision,
            persist,
            allow,
        } = outcome;
        // Persist explicit `Always` choices. In-memory decision applies
        // regardless; failure is logged and surfaced via the receipt
        // source, never silent.
        let mut receipt_source = success_source.to_owned();
        if persist {
            // M007: narrow the remembered grant toward the deterministic
            // command family (for example `cmd:cargo`) where the tool
            // call carries structured command/effect data. `None` keeps
            // the legacy broad shape; legacy rows stay readable.
            let scope =
                crate::permission::decision_scope_for_tool_call(&tc.name, Some(&tc.arguments));
            let persisted_source = self
                .persist_always_choice(
                    &tc.name,
                    request.path.as_deref(),
                    scope.as_deref(),
                    &request.session_id,
                    allow,
                )
                .await;
            // Preserve the mode-specific success source when persistence
            // succeeded; surface the unpersisted diagnostic otherwise.
            if persisted_source == approval_source::USER_CHOICE_UNPERSISTED
                || success_source == approval_source::USER_CHOICE
            {
                receipt_source = persisted_source.to_owned();
            }
        }
        match decision {
            ApprovalDecision::Allow { .. } => ToolPermissionOutcome::Allowed {
                tool_call: tc.clone(),
                receipt: self.accepted_permission_receipt(receipt_source.as_str()),
            },
            ApprovalDecision::Deny { reason, source } => {
                // Human timeout vs explicit deny already distinguished by
                // the router source (`timeout_deny` vs `user_choice`).
                let _ = source;
                if reason.contains("timeout") {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!("Tool '{}' denied: approval timeout", tc.name),
                    }
                } else if request
                    .escalation_reasons
                    .iter()
                    .any(|r| r.contains("sensitive path"))
                {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!(
                            "Tool '{}' denied: access to sensitive path refused",
                            tc.name
                        ),
                    }
                } else if request
                    .escalation_reasons
                    .iter()
                    .any(|r| r.contains("security escalation"))
                {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!("Tool '{}' denied by user (security escalation)", tc.name),
                    }
                } else {
                    ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!("Tool '{}' denied by user", tc.name),
                    }
                }
            }
            ApprovalDecision::DeferUser { reason, .. } => ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message: format!("Tool '{}' deferred: {reason}", tc.name),
            },
        }
    }

    /// M006: resolve one `Automatic` escalation through the bounded
    /// read-only reviewer, with safe human fallback.
    ///
    /// - Deterministic `Deny` never reaches here (normalized above); only
    ///   `Escalate` is reviewed.
    /// - Unconfigured/unknown reviewer model → human fallback (interactive)
    ///   or explicit deny (configured headless mode). Never fail-open.
    /// - Reviewer `Allow` applies only when policy/sandbox still match the
    ///   captured snapshot; a mid-review change discards the verdict.
    /// - Reviewer `Deny` returns bounded feedback to the primary model and
    ///   records the equivalent-denial backstop.
    /// - Reviewer `Defer`/failure → human fallback (interactive) or deny
    ///   (headless). Cancellation never falls back to a human wait.
    /// - `general_ask_dialog_args`: `Some` preserves the original dialog
    ///   payload for the general-`Ask` human fallback; `None` uses the
    ///   escalation-path payload.
    async fn resolve_automatic_escalation(
        &mut self,
        tc: &ToolCall,
        request: &ApprovalRequest,
        router: &ApprovalRouter,
        snapshot: &ExecutionPolicySnapshot,
        general_ask_dialog_args: Option<Option<serde_json::Value>>,
    ) -> ToolPermissionOutcome {
        let reviewer_config = approval_reviewer::ReviewerConfig::from_config(&self.services.config);
        let headless = reviewer_config.headless_deny;
        let max_denials = reviewer_config.max_equivalent_denials;

        // Equivalent-denial backstop before spending a reviewer call.
        let denial_key = approval_reviewer::denial_key_for(
            &tc.name,
            request.path.as_deref(),
            request.args_summary.as_deref(),
        );
        let prior = self
            .reviewer_denial_counts
            .get(&denial_key)
            .copied()
            .unwrap_or(0);
        if prior >= max_denials {
            if headless {
                return ToolPermissionOutcome::Denied {
                    tool_id: tc.id.to_string(),
                    message: format!(
                        "Tool '{}' denied: repeated reviewer denials ({prior})",
                        tc.name
                    ),
                };
            }
            return self
                .automatic_human_fallback(tc, request, router, general_ask_dialog_args)
                .await;
        }

        // Reviewer availability: a configured, registry-validated model id.
        // Absent/unknown → documented defer behavior (human or headless
        // deny), never a silent provider switch or auto-allow.
        let model_id = match reviewer_config.resolve_model(&self.services.provider_registry) {
            Ok(model) => model,
            Err(_) => {
                if headless {
                    return ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!(
                            "Tool '{}' denied: automatic reviewer unavailable",
                            tc.name
                        ),
                    };
                }
                return self
                    .automatic_human_fallback(tc, request, router, general_ask_dialog_args)
                    .await;
            }
        };

        // Snapshot owned inputs so the review phase holds no `&mut` use.
        let workspace_root = self.workspace_root.clone();
        let session_id = self.session_id.clone();
        let user_objective = self.original_user_prompt.clone();
        let provider_box = self.services.provider.clone_box();
        let cancel = self.cancel_rx.clone();
        let snapshot_before = snapshot.clone();
        let request_owned = request.clone();

        let outcome = {
            let investigator = approval_reviewer::RegistryReviewerInvestigator::new(
                &self.services.tool_registry,
                workspace_root.clone(),
                session_id.clone(),
            );
            let backend = approval_reviewer::ProviderReviewerBackend::new(provider_box, model_id)
                .with_max_output_chars(reviewer_config.max_output_chars);
            let reviewer_request = approval_reviewer::ReviewerRequest::from_approval_request(
                format!("{}-{}", tc.id, tc.name),
                &request_owned,
                &snapshot_before,
                user_objective,
                None,
                None,
                None,
                Some(workspace_root.to_string_lossy().into_owned()),
            );
            approval_reviewer::resolve_automatic_escalation(
                &snapshot_before,
                &request_owned,
                &reviewer_request,
                &reviewer_config,
                &backend,
                &investigator,
                cancel,
                snapshot_before.policy_revision().map(str::to_owned),
            )
            .await
        };

        // Cancellation of the primary turn cancels the reviewer: never fall
        // back to a human wait after cancel.
        if self
            .cancel_rx
            .as_ref()
            .map(|rx| *rx.borrow())
            .unwrap_or(false)
        {
            return ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message: format!("Tool '{}' denied: turn cancelled during review", tc.name),
            };
        }

        // A policy/sandbox change during review invalidates a stale Allow:
        // discard and defer/re-review rather than apply it.
        if outcome.decision.allowed() {
            let fresh = self.capture_execution_snapshot();
            let changed = fresh.approval_mode() != snapshot_before.approval_mode()
                || fresh.sandbox_profile() != snapshot_before.sandbox_profile()
                || fresh.policy_revision() != snapshot_before.policy_revision();
            if changed {
                tracing::warn!(
                    tool = %tc.name,
                    "automatic reviewer allow discarded: policy changed during review"
                );
                if headless {
                    return ToolPermissionOutcome::Denied {
                        tool_id: tc.id.to_string(),
                        message: format!("Tool '{}' denied: policy changed during review", tc.name),
                    };
                }
                return self
                    .automatic_human_fallback(tc, request, router, general_ask_dialog_args)
                    .await;
            }
            return ToolPermissionOutcome::Allowed {
                tool_call: tc.clone(),
                receipt: self.accepted_permission_receipt(approval_source::REVIEWER_ALLOW),
            };
        }

        // Reviewer deny: record the backstop and return bounded feedback to
        // the primary model so it can choose a safer alternative.
        if outcome.decision.source() == approval_source::REVIEWER_DENY {
            let entry = self.reviewer_denial_counts.entry(denial_key).or_insert(0);
            *entry = entry.saturating_add(1);
            let mut message = format!("Tool '{}' denied by approval reviewer", tc.name);
            if let Some(feedback) = outcome.primary_feedback.as_deref() {
                let bounded = feedback.chars().take(512).collect::<String>();
                if !bounded.trim().is_empty() {
                    message.push_str(&format!(": {bounded}"));
                }
            }
            return ToolPermissionOutcome::Denied {
                tool_id: tc.id.to_string(),
                message,
            };
        }

        // Defer or fail-closed error: human fallback (interactive) or deny
        // (headless). `fall_back_to_human` is false only in headless mode.
        if outcome.fall_back_to_human {
            return self
                .automatic_human_fallback(tc, request, router, general_ask_dialog_args)
                .await;
        }
        ToolPermissionOutcome::Denied {
            tool_id: tc.id.to_string(),
            message: format!("Tool '{}' denied: automatic reviewer unavailable", tc.name),
        }
    }

    /// Human fallback for `Automatic` when the reviewer defers, is
    /// unavailable, or hits the equivalent-denial backstop. Preserves the
    /// M003 `automatic_defer` receipt source.
    async fn automatic_human_fallback(
        &mut self,
        tc: &ToolCall,
        request: &ApprovalRequest,
        router: &ApprovalRouter,
        general_ask_dialog_args: Option<Option<serde_json::Value>>,
    ) -> ToolPermissionOutcome {
        match general_ask_dialog_args {
            Some(dialog_args) => {
                let perm_id = format!("{}-{}", tc.id, tc.name);
                let outcome = router
                    .request_human_approval(&perm_id, request, dialog_args)
                    .await;
                self.apply_human_outcome(tc, request, outcome, approval_source::AUTOMATIC_DEFER)
                    .await
            }
            None => {
                self.resolve_escalation_via_human(
                    tc,
                    request,
                    router,
                    approval_source::AUTOMATIC_DEFER,
                )
                .await
            }
        }
    }

    /// Persist an explicit `Always` human choice with visible diagnostics.
    /// Returns the receipt source to record (`user_choice` vs
    /// `user_choice_unpersisted`). In-memory decision applies regardless;
    /// `false` persistence is logged and surfaced, never silent.
    async fn persist_always_choice(
        &self,
        tool: &str,
        path: Option<&str>,
        scope: Option<&str>,
        session_id: &str,
        allow: bool,
    ) -> &'static str {
        let persisted = if allow {
            self.services
                .permission_checker
                .always_allow_scoped(tool, path, scope, Some(session_id))
                .await
        } else {
            self.services
                .permission_checker
                .always_deny_scoped(tool, path, scope, Some(session_id))
                .await
        };
        if persisted {
            approval_source::USER_CHOICE
        } else {
            tracing::warn!(
                tool = %tool,
                "permission Always decision applied in-memory but NOT persisted"
            );
            approval_source::USER_CHOICE_UNPERSISTED
        }
    }

    #[allow(clippy::incompatible_msrv)]
    pub(super) async fn execute_tool_calls_impl(
        &mut self,
        tool_calls: &[ToolCall],
    ) -> Result<Vec<(String, ToolExecutionOutcome)>, AppError> {
        let mut tool_results = Vec::with_capacity(16);
        let mut has_pending_question = false;

        // M003: fixed snapshot for the accepted tool batch. Mode changes
        // from another frontend apply on the next boundary and cannot
        // retroactively bless pending actions in this batch.
        let batch_snapshot = self.capture_execution_snapshot();
        let mut allowed_tools = Vec::with_capacity(tool_calls.len());
        for (idx, tc) in tool_calls.iter().enumerate() {
            match self
                .check_tool_permission_with_snapshot(tc, &batch_snapshot)
                .await
            {
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

        // --- Edit checkpoint: derive affected paths and capture pre-state ---
        // Durable checkpoint capture is scoped to workspace/session/turn/batch and
        // does not depend on unscoped global FileChanged draining.
        let mut checkpoint_ctx: Option<CheckpointContext> = None;
        let mut checkpoint_guard = None;
        let mut checkpoint_serialize = false;
        let has_restorable = allowed_tools
            .iter()
            .any(|(_, tc, _)| crate::snapshot::affected_paths::is_restorable_tool(&tc.name));
        let checkpoint_batch_is_eligible = has_restorable
            && allowed_tools.iter().all(|(_, tc, _)| {
                crate::snapshot::affected_paths::is_restorable_tool(&tc.name)
                    || is_affirmatively_read_only_tool(tc)
            });
        if checkpoint_batch_is_eligible {
            let manager = self.services.checkpoint_manager.as_ref();
            let workspace_id = self.workspace_id.as_ref();
            let workspace_locks = self.workspace_locks.as_ref();
            if let (Some(mgr), Some(workspace_id), Some(workspace_locks)) =
                (manager, workspace_id, workspace_locks)
            {
                let batch_calls: Vec<(String, serde_json::Value)> = allowed_tools
                    .iter()
                    .map(|(_, tc, _)| (tc.name.to_string(), tc.arguments.clone()))
                    .collect();
                match crate::snapshot::affected_paths::extract_batch_affected_paths_with_read_only(
                    &batch_calls,
                    is_affirmatively_read_only_name,
                ) {
                    Ok(Some(raw_paths)) => {
                        let raw_len = raw_paths.len();
                        match crate::snapshot::affected_paths::normalize_and_dedup(
                            raw_paths,
                            &self.workspace_root,
                        ) {
                            Ok(normalized) => {
                                checkpoint_serialize =
                                    crate::snapshot::affected_paths::has_overlapping_paths(
                                        raw_len,
                                        normalized.len(),
                                    );
                                // Hold the canonical per-repository authority from
                                // the first pre-state read through native execution,
                                // post-state capture, and durable persistence. The
                                // workspace service lease keeps this lock table from
                                // being evicted and replaced while the detached loop
                                // runs.
                                checkpoint_guard = Some(
                                    workspace_locks
                                        .acquire_repository(&self.workspace_root)
                                        .await,
                                );
                                match mgr.capture_states(&normalized).await {
                                    Ok(pre_states) => {
                                        let ws_id = workspace_id.to_string();
                                        let sess_id = self.session_id.clone();
                                        let turn = self.turn_id.clone();
                                        let seq = self.checkpoint_batch_seq as i64;
                                        self.checkpoint_batch_seq =
                                            self.checkpoint_batch_seq.wrapping_add(1);
                                        checkpoint_ctx = Some(CheckpointContext {
                                            paths: normalized,
                                            pre_states,
                                            workspace_id: ws_id,
                                            session_id: sess_id,
                                            turn_id: turn,
                                            batch_seq: seq,
                                        });
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "checkpoint pre-state capture failed, batch non-restorable: {}",
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                "checkpoint path normalization failed, batch non-restorable: {}",
                                e
                            );
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(
                            "checkpoint affected-path extraction failed, batch non-restorable: {}",
                            e
                        );
                    }
                }
            }
        } else if has_restorable {
            tracing::debug!(
                "checkpoint batch contains unknown side effects or lacks workspace authority; batch non-restorable"
            );
        }
        // Clear stale FileChanged events for hygiene but do not use for durability.
        // drain returns drained events (Vec); dropped count logged for watch-lag visibility.
        let drained = self.drain_file_change_events();
        if !drained.is_empty() {
            tracing::debug!("drained {} stale file-change events", drained.len());
        }

        let _timeout_secs = self.tool_timeout();
        let max_parallel = self.max_parallel_tools();
        const MAX_PARALLEL_DEFAULT: usize = 100;
        let mut effective_max = if max_parallel == usize::MAX {
            MAX_PARALLEL_DEFAULT
        } else {
            max_parallel
        };
        if checkpoint_serialize {
            effective_max = 1;
        }
        let regular_tool_count = allowed_tools.len();
        let registry = &self.services.tool_registry;

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
            let mcp_arc = self.services.mcp_service.clone();
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
        let hook_registry = self.services.hook_registry.as_ref().map(Arc::clone);
        let plugin_service = self.plugin_service.as_ref().map(Arc::clone);
        let event_store = self.services.event_store.clone();
        let tool_broker = Arc::clone(&self.services.tool_broker);
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
        let batch_snapshot_for_ctx = batch_snapshot.clone();
        for (orig_idx, tc, receipt) in regular_tools {
            // Build the structured-execution context here (before
            // `tc` is moved into an Arc) so the helper, which takes
            // `&self`, can read live state without forcing the
            // `async move` closure to capture `self` by move.
            let tool_name_for_ctx = tc.name.clone();
            let timeout = self.get_tool_timeout(&tool_name_for_ctx);
            let exec_ctx = self.build_tool_execution_context(
                &tc,
                orig_idx,
                Some(timeout.as_millis() as u64),
                &receipt,
                &batch_snapshot_for_ctx,
            );
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
                                    // Preserve the accepted model tool-call
                                    // identity through the broker into
                                    // structured tools such as TaskTool.
                                    submission_key: exec_ctx.invocation_key.clone(),
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
                                    format!("{}...", crate::util::truncate_prefix(o, 497))
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
                            format!("{}...", crate::util::truncate_prefix(o, 197))
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
            .services
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

        // --- Edit checkpoint: capture post-state and persist ---
        if let Some(CheckpointContext {
            paths: normalized,
            pre_states,
            workspace_id: ws_id,
            session_id: sess_id,
            turn_id,
            batch_seq,
        }) = checkpoint_ctx.take()
        {
            if let Some(mgr) = &self.services.checkpoint_manager {
                match mgr.capture_states(&normalized).await {
                    Ok(post_states) => {
                        let mut files = Vec::new();
                        let mut has_change = false;
                        for path in &normalized {
                            let pre = pre_states
                                .get(path)
                                .cloned()
                                .unwrap_or(crate::snapshot::checkpoint::FileState::Absent);
                            let post = post_states
                                .get(path)
                                .cloned()
                                .unwrap_or(crate::snapshot::checkpoint::FileState::Absent);
                            if pre != post {
                                has_change = true;
                            }
                            files.push(crate::snapshot::checkpoint::EditFileState {
                                path: path.clone(),
                                pre,
                                post,
                            });
                        }
                        // Persist only when the batch meaningfully represents a mutation
                        // and does not exceed existing snapshot bounds. Oversized or
                        // unsafe post-states would have already errored during capture.
                        if has_change && !files.is_empty() {
                            let checkpoint = crate::snapshot::checkpoint::EditCheckpoint {
                                id: uuid::Uuid::new_v4().to_string(),
                                workspace_id: ws_id,
                                session_id: sess_id,
                                turn_id,
                                batch_seq,
                                created_at: chrono::Utc::now().timestamp_millis(),
                                files,
                            };
                            if let Err(e) = mgr.persist_checkpoint(checkpoint).await {
                                tracing::warn!("checkpoint persist failed: {}", e);
                            } else {
                                tracing::info!(
                                    "edit checkpoint persisted for batch {} ({} files)",
                                    batch_seq,
                                    normalized.len()
                                );
                            }
                        } else if !has_change {
                            tracing::debug!(
                                "checkpoint: no file state change for batch {}, skipping persist",
                                batch_seq
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "checkpoint post-state capture failed, batch non-restorable: {}",
                            e
                        );
                    }
                }
            }
        }

        // Do not hold a workspace mutation guard while waiting for a question
        // response or performing any later turn bookkeeping.
        drop(checkpoint_guard);

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

fn invocation_key_for(
    session_id: &str,
    turn_id: Option<&str>,
    run_id: Option<&codegg_core::identity::AgentRunId>,
    provider_turn: usize,
    provider_call_id: &str,
    accepted_call_ordinal: usize,
) -> String {
    let owner_scope = if let Some(run_id) = run_id {
        format!("run:{run_id}")
    } else if let Some(turn_id) = turn_id {
        format!("turn:{session_id}:{turn_id}")
    } else {
        format!("session:{session_id}")
    };
    let invocation_material = format!(
        "agent-invocation-v2/{owner_scope}/provider-turn/{provider_turn}/tool-call/{provider_call_id}/ordinal/{accepted_call_ordinal}"
    );
    use sha2::Digest;
    format!(
        "agent-invocation-{:x}",
        sha2::Sha256::digest(invocation_material.as_bytes())
    )
}

/// Bounded redacted summary for audit/bus payloads. Callers must not pass
/// raw secrets; this truncates and strips NULs as a second bound.
fn truncate_for_audit(value: &str) -> String {
    const MAX: usize = 512;
    let mut out = value.to_owned();
    if out.len() > MAX {
        out.truncate(MAX);
    }
    out.replace('\0', "")
}

#[cfg(test)]
mod invocation_tests {
    use super::{invocation_key_for, is_affirmatively_read_only_name};
    use codegg_core::identity::AgentRunId;
    use serde_json::json;

    #[test]
    fn invocation_identity_is_scoped_to_owner_and_provider_turn() {
        let first_turn = invocation_key_for("session", Some("turn-1"), None, 1, "text-repair-0", 0);
        let second_turn =
            invocation_key_for("session", Some("turn-2"), None, 1, "text-repair-0", 0);
        let later_provider_turn =
            invocation_key_for("session", Some("turn-1"), None, 2, "text-repair-0", 0);
        let run = AgentRunId::new();
        let child =
            invocation_key_for("session", Some("turn-1"), Some(&run), 1, "text-repair-0", 0);

        assert_ne!(first_turn, second_turn);
        assert_ne!(first_turn, later_provider_turn);
        assert_ne!(first_turn, child);
        assert_eq!(
            first_turn,
            invocation_key_for("session", Some("turn-1"), None, 1, "text-repair-0", 0)
        );
    }

    #[test]
    fn duplicate_provider_ids_are_separated_by_accepted_ordinal() {
        assert_ne!(
            invocation_key_for("session", Some("turn"), None, 1, "duplicate", 0),
            invocation_key_for("session", Some("turn"), None, 1, "duplicate", 1)
        );
    }

    #[test]
    fn checkpoint_batch_classification_fails_closed_for_unknown_effects() {
        assert!(is_affirmatively_read_only_name(
            "read",
            &json!({"path": "a.txt"})
        ));
        assert!(is_affirmatively_read_only_name(
            "bash",
            &json!({"command": "ls a.txt"})
        ));
        assert!(!is_affirmatively_read_only_name(
            "bash",
            &json!({"command": "touch a.txt"})
        ));
        assert!(!is_affirmatively_read_only_name(
            "mcp__server__write",
            &json!({"path": "a.txt"})
        ));
        assert!(!is_affirmatively_read_only_name(
            "plugin_mutation",
            &json!({"path": "a.txt"})
        ));
    }
}
