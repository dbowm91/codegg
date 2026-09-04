//! Canonical command pipeline entry point.
//!
//! The pipeline owns the single production data flow from normalized command
//! input through semantic intent, validated backend planning, and the typed
//! executor target. Authorization remains at the daemon/tool boundary; the
//! plan only reports whether active routing is eligible for its preflight.

use super::plan::{plan_execution_with_context, CommandDispatchTarget, CommandPlan};
use super::{classify_command_with_context, CommandIntent, CommandIntentContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPipelineResult {
    pub intent: CommandIntent,
    pub plan: CommandPlan,
    pub dispatch: CommandDispatchTarget,
}

impl CommandPipelineResult {
    pub fn is_rejected(&self) -> bool {
        !self.plan.is_executable()
    }

    pub fn validate_for_active_routing(&self) -> Result<(), String> {
        self.plan.validate_for_active_routing()
    }
}

/// Normalize, classify, plan, and produce the one typed dispatch target for a
/// command. Callers that already have a typed intent can continue at
/// `plan_execution_with_context` and use `CommandPlan::dispatch_target()`.
pub fn prepare_command(command: &str, context: &CommandIntentContext) -> CommandPipelineResult {
    let intent = classify_command_with_context(command, context);
    let plan = plan_execution_with_context(&intent, context);
    let dispatch = plan.dispatch_target();
    CommandPipelineResult {
        intent,
        plan,
        dispatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_intent::plan::CommandDispatchTarget;

    #[test]
    fn pipeline_preserves_explicit_execution_cwd() {
        let context = CommandIntentContext {
            workspace_root: Some("/workspace".into()),
            cwd: Some("/workspace/project".into()),
        };
        let result = prepare_command("rg pattern src", &context);
        assert_eq!(result.plan.cwd, Some("/workspace/project".into()));
        match result.dispatch {
            CommandDispatchTarget::RouteToManagedProcess { cwd, .. } => {
                assert_eq!(cwd, std::path::PathBuf::from("/workspace/project"));
            }
            other => panic!("expected managed process, got {other:?}"),
        }
    }

    #[test]
    fn aliases_and_invalid_input_share_the_same_pipeline() {
        let context = CommandIntentContext::default();
        let cargo = prepare_command("cargo test --lib", &context);
        let pytest = prepare_command("pytest tests/", &context);
        assert!(matches!(
            cargo.dispatch,
            CommandDispatchTarget::RouteToTestRunner { .. }
        ));
        assert!(matches!(
            pytest.dispatch,
            CommandDispatchTarget::RouteToTestRunner { .. }
        ));

        let invalid = prepare_command("", &context);
        assert!(invalid.is_rejected());
        assert!(matches!(
            invalid.dispatch,
            CommandDispatchTarget::Rejected { .. }
        ));
    }

    #[test]
    fn typed_git_and_shell_targets_remain_distinct() {
        let context = CommandIntentContext::default();
        let git = prepare_command("git status", &context);
        let shell = prepare_command("echo hi | cat", &context);
        assert!(matches!(
            git.dispatch,
            CommandDispatchTarget::RouteToGit { .. }
        ));
        assert!(matches!(
            shell.dispatch,
            CommandDispatchTarget::RouteToShell { .. }
        ));
    }

    #[test]
    fn authorization_preflight_is_plan_data_not_a_second_router() {
        let result = prepare_command("git commit -m fix", &CommandIntentContext::default());
        assert_eq!(
            result.plan.command_family(),
            Some(crate::config::schema::CommandIntentFamily::GitLocalMutation)
        );
        assert!(result.plan.requires_any_permission());
        assert!(result.validate_for_active_routing().is_err());
        assert!(matches!(
            result.dispatch,
            CommandDispatchTarget::RouteToGit { .. }
        ));
    }
}
