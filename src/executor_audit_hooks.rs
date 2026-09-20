//! Executor-owned live audit hook pins (identity/audit M004).
//!
//! Declarative owner table consumed by the coverage guard
//! (`scripts/check_audit_coverage.py`) and the M004 qualification matrix
//! (`tests/identity_live_execution_audit.rs`). Each row names one live
//! executor hook by audit action, canonical owner file, and emit symbol.
//!
//! The guard parses this table (not ad-hoc call text) and fails closed if
//! an owner file moves or an emit symbol disappears. Removing a row -- or
//! the hook it pins -- must fail the guard; adding the action name to
//! `UNINSTRUMENTED_OPERATIONS` never satisfies it (pinned by
//! `codegg-core::audit_instrumentation` unit tests).

/// One pinned executor hook site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutorHookPin {
    /// Canonical audit action wire name.
    pub action: &'static str,
    /// Canonical owner file, relative to the repository root.
    pub owner_file: &'static str,
    /// Canonical emit symbol that must appear in the owner file.
    pub emit_symbol: &'static str,
}

/// Declarative executor-hook owner table (M004).
///
/// Three live actions, one row per canonical emit owner:
///
/// - `command_execute` at the tool dispatch owners (bash/terminal/test
///   plus interactive create) through `live_execution_audit`;
/// - `git_operation` at `GitMutationExecutor` (typed/network/recovery plus
///   raw paths);
/// - `job_complete` at the scheduler terminal transition.
pub const EXECUTOR_HOOK_OWNER_PINS: &[ExecutorHookPin] = &[
    ExecutorHookPin {
        action: "command_execute",
        owner_file: "src/live_execution_audit.rs",
        emit_symbol: "emit_command_execute",
    },
    ExecutorHookPin {
        action: "command_execute",
        owner_file: "src/tool/bash.rs",
        emit_symbol: "emit_command_audit",
    },
    ExecutorHookPin {
        action: "command_execute",
        owner_file: "src/tool/terminal.rs",
        emit_symbol: "emit_command_audit",
    },
    ExecutorHookPin {
        action: "command_execute",
        owner_file: "src/tool/test.rs",
        emit_symbol: "emit_test_audit",
    },
    ExecutorHookPin {
        action: "command_execute",
        owner_file: "src/interactive_process_attach.rs",
        emit_symbol: "emit_interactive_create_audit",
    },
    ExecutorHookPin {
        action: "git_operation",
        owner_file: "src/git_mutations.rs",
        emit_symbol: "emit_git_operation",
    },
    ExecutorHookPin {
        action: "git_operation",
        owner_file: "src/tool/git.rs",
        emit_symbol: "emit_raw_git_audit",
    },
    ExecutorHookPin {
        action: "job_complete",
        owner_file: "src/scheduler/job_complete_audit.rs",
        emit_symbol: "emit_terminal_completion",
    },
    ExecutorHookPin {
        action: "job_complete",
        owner_file: "src/scheduler/scheduler.rs",
        emit_symbol: "emit_terminal_completion",
    },
];

/// Actions that must have at least one owner pin in this table.
pub const PINNED_ACTIONS: &[&str] = &["command_execute", "git_operation", "job_complete"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_pins_cover_exactly_the_executor_table() {
        use codegg_core::audit_instrumentation as instr;
        // Every pinned action is an executor-live action in core, and every
        // executor-live action has at least one pin here.
        for action in PINNED_ACTIONS {
            assert!(
                instr::is_executor_live_action(action),
                "pinned action {action} must be executor-live in core"
            );
        }
        assert_eq!(instr::EXECUTOR_LIVE_AUDIT_HOOKS.len(), PINNED_ACTIONS.len());
        for hook in instr::EXECUTOR_LIVE_AUDIT_HOOKS {
            assert!(
                PINNED_ACTIONS.contains(&hook.action),
                "executor hook {} needs an owner pin",
                hook.action
            );
            assert!(
                EXECUTOR_HOOK_OWNER_PINS
                    .iter()
                    .any(|pin| pin.action == hook.action),
                "no owner pin for {}",
                hook.action
            );
        }
        // Future/distributed actions must never gain a pin.
        for future in instr::FUTURE_DISTRIBUTED_AUDIT_ACTIONS {
            assert!(
                !EXECUTOR_HOOK_OWNER_PINS
                    .iter()
                    .any(|pin| pin.action == *future),
                "future action {future} must not have an owner pin"
            );
        }
    }
}
