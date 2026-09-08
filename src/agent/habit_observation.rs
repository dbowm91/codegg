//! Host-owned habit observation adapter for the agent turn.
//!
//! Physical decomposition of [`super::r#loop`] (M003): collects allowlisted
//! structural action metadata for successful turns. Raw calls/results never
//! reach this collector; persistence stays owned by the habit store.

use super::r#loop::AgentLoop;
use super::tool_inspect::{is_soft_stop_reason, tool_outcome_is_success};
use crate::agent::progress_recovery::ToolExecutionOutcome;
use crate::provider::{ChatEvent, ToolCall};

impl AgentLoop {
    fn habit_action_for_tool(
        &self,
        tool_call: &ToolCall,
    ) -> Option<codegg_core::memory::habit::WorkflowAction> {
        use crate::tool::contract::ToolEffectClass;
        use codegg_core::memory::habit::{WorkflowAction, WorkflowActionKind, WorkflowEffectClass};

        let tool_name = tool_call.name.as_str();
        let (kind, variant) = match tool_name {
            "read" => (WorkflowActionKind::FileRead, None),
            "glob" | "grep" | "list" | "diff" | "codesearch" | "repo_search" | "repo_map"
            | "security_search" | "websearch" | "webfetch" => (WorkflowActionKind::Search, None),
            // Historical name tolerance: `multiedit` was removed from the
            // registry in M001, but stored runs may still name it.
            "edit" | "write" | "replace" | "multiedit" => (WorkflowActionKind::Edit, None),
            "apply_patch" => (WorkflowActionKind::Patch, None),
            "test" => (WorkflowActionKind::Test, None),
            "git" => {
                let subcommand = tool_call
                    .arguments
                    .get("subcommand")
                    .and_then(serde_json::Value::as_str);
                match subcommand {
                    Some(
                        subcommand @ ("status" | "diff" | "show" | "log" | "blame"
                        | "changed-files" | "branch" | "tag" | "remote" | "worktree"
                        | "stash"),
                    ) => (WorkflowActionKind::GitRead, Some(subcommand.to_string())),
                    Some(
                        subcommand @ ("add" | "commit" | "reset" | "checkout" | "merge" | "rebase"
                        | "fetch" | "pull" | "push" | "clean"),
                    ) => (WorkflowActionKind::GitWrite, Some(subcommand.to_string())),
                    _ => (WorkflowActionKind::GitWrite, None),
                }
            }
            "lsp" => (WorkflowActionKind::LspRead, None),
            "skill" => (WorkflowActionKind::SkillActivate, None),
            "task" => (WorkflowActionKind::Delegate, None),
            "bash" | "terminal" => (WorkflowActionKind::ShellExec, None),
            name if matches!(
                name,
                "text_equal"
                    | "text_diff_explain"
                    | "text_replace_check"
                    | "validate_json"
                    | "validate_toml"
                    | "command_preflight"
                    | "path_normalize"
                    | "text_security_inspect"
            ) =>
            {
                (
                    WorkflowActionKind::DeterministicValidate,
                    Some(name.to_string()),
                )
            }
            _ => return None,
        };

        let effect = if kind == WorkflowActionKind::GitRead {
            WorkflowEffectClass::ReadOnly
        } else {
            self.services
                .tool_registry
                .get(tool_name)
                .map(|tool| tool.contract(tool_name, tool.parameters()).effect_class)
                .map(|class| match class {
                    ToolEffectClass::ReadOnly => WorkflowEffectClass::ReadOnly,
                    ToolEffectClass::ReadValidate => WorkflowEffectClass::ReadValidate,
                    ToolEffectClass::SafeRepeat => WorkflowEffectClass::SafeRepeat,
                    ToolEffectClass::IdempotentMutating | ToolEffectClass::NonIdempotent => {
                        WorkflowEffectClass::Mutating
                    }
                    ToolEffectClass::ProcessExec => WorkflowEffectClass::ProcessExec,
                })
                .unwrap_or_else(|| match tool_name {
                    "read" | "glob" | "grep" | "list" | "diff" => WorkflowEffectClass::ReadOnly,
                    "bash" | "terminal" | "test" => WorkflowEffectClass::ProcessExec,
                    _ => WorkflowEffectClass::Mutating,
                })
        };

        Some(WorkflowAction::new(kind, variant, effect))
    }

    /// Collect safe action metadata for one completed tool batch. This is the
    /// sole observation adapter; individual tools never know about the habit
    /// store. Failed results invalidate the enclosing occurrence.
    pub(super) fn record_habit_tool_results(
        &mut self,
        tool_calls: &[ToolCall],
        tool_results: &[(String, ToolExecutionOutcome)],
    ) {
        for tool_call in tool_calls {
            let Some((_, outcome)) = tool_results
                .iter()
                .find(|(id, _)| id == tool_call.id.as_ref())
            else {
                self.habit_had_failure = true;
                continue;
            };
            if !tool_outcome_is_success(outcome) {
                self.habit_had_failure = true;
                continue;
            }
            if let Some(action) = self.habit_action_for_tool(tool_call) {
                if self.habit_actions.len() < codegg_core::memory::habit::MAX_WORKFLOW_ACTIONS * 2 {
                    self.habit_actions.push(action);
                }
            }
        }
    }

    pub(super) fn finalize_habit_observation(&mut self, events: &[ChatEvent]) {
        let explicit_success = events.iter().rev().find_map(|event| match event {
            ChatEvent::Finish { stop_reason, .. } => {
                Some(is_soft_stop_reason(Some(stop_reason.as_str())))
            }
            _ => None,
        }) == Some(true);
        if !explicit_success || self.habit_had_failure || self.habit_actions.is_empty() {
            return;
        }
        let Some(store) = self.services.habit_store.clone() else {
            return;
        };
        let occurrence = codegg_core::memory::habit::WorkflowOccurrence {
            project_namespace: self.habit_project_namespace.clone(),
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            root_or_run_id: self.run_id.as_ref().map(ToString::to_string),
            actions: self.habit_actions.clone(),
            outcome: codegg_core::memory::habit::WorkflowOutcome::Succeeded,
            occurred_at: chrono::Utc::now().timestamp_millis(),
        };
        if let Err(error) = store.observe(occurrence) {
            tracing::warn!(error = %error, "failed to persist habit observation");
        }
    }
}
