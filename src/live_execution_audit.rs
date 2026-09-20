//! Live execution audit hooks (identity/audit corrective M002).
//!
//! Canonical ownership for single-host live `command_execute` events:
//!
//! ```text
//! ToolBroker::execute/execute_with_retry (model/tool path)
//!   ├─ bash tool      → family "shell" / "test" / "process" (this module)
//!   ├─ terminal tool  → family "process" (direct native spawn)
//!   ├─ test tool      → family "test" (scheduler-owned test dispatch)
//!   └─ git tool / bash-routed git → NO command event;
//!        GitMutationExecutor owns the single `git_operation` event
//! interactive create → family "interactive" (daemon pre-router path)
//! ```
//!
//! Every helper emits at most one structural event per real dispatched
//! execution and only when the M001 trusted context plus the shared
//! bounded emitter are both present. Digests are SHA-256 hex over
//! normalized command/argv bytes; command text, argv, terminal input,
//! URLs, and tool output never enter audit metadata. Retries of one
//! logical invocation reuse a deterministic event id so the store
//! returns the stored row instead of duplicating it.
//!
//! Denied / pre-dispatch failures emit nothing: the authorization
//! denial itself is audited through `authorization_decision` elsewhere.

use codegg_core::audit::AuditAction;
use codegg_core::audit_instrumentation::{
    command_execute_event, deterministic_event_id, structural_digest, ExecutionAuditEmitter,
    TrustedExecutionAuditContext,
};

use crate::command_intent::plan::CommandDispatchTarget;
use crate::command_outcome::ActualExecutor;

/// Terminal outcome vocabulary for live `command_execute` events.
///
/// Bounded literals only: success, execution failure, timeout,
/// cancellation, or an uncertain commit. Never invent success for a
/// dispatch that never occurred.
pub const OUTCOME_SUCCESS: &str = "success";
pub const OUTCOME_FAILURE: &str = "failure";
pub const OUTCOME_TIMEOUT: &str = "timeout";
pub const OUTCOME_CANCELLED: &str = "cancelled";
pub const OUTCOME_UNCERTAIN: &str = "uncertain";

/// Canonical `command_execute` family for one model-facing tool.
///
/// Returns `None` for every tool that does not dispatch a real
/// command/process (read-only tools, chat, artifacts, git — the Git
/// executor owns `git_operation` for those). The broker path never
/// emits directly; each owning tool emits through this map so the
/// ownership matrix stays testable in one place.
pub fn command_family_for_tool(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "bash" => Some("shell"),
        "terminal" => Some("process"),
        "test" => Some("test"),
        _ => None,
    }
}

/// SHA-256 hex digest over normalized command/argv bytes.
///
/// Join argv entries with NUL so `["a b", "c"]` and `["a", "b c"]`
/// digest differently. The preimage never leaves this function.
pub fn command_digest_argv(argv: &[String]) -> String {
    structural_digest(argv.join("\0").as_bytes())
}

/// SHA-256 hex digest over one normalized command string.
pub fn command_digest_str(command: &str) -> String {
    structural_digest(command.as_bytes())
}

/// Deterministic idempotency scope for one logical invocation.
///
/// Retries share the invocation key (broker `submission_key` /
/// `invocation_key`) so a replayed terminal outcome reuses the event
/// id; distinct invocations and distinct terminal outcomes scope
/// separately so two real executions never collapse into one row.
/// Legacy callers without a key fall back to the command digest.
pub fn invocation_scope(invocation_key: Option<&str>, digest_hex: &str, outcome: &str) -> String {
    let key = invocation_key
        .filter(|key| !key.is_empty())
        .unwrap_or(digest_hex);
    format!("{key}:{outcome}")
}

/// Emit one live `command_execute` structural event.
///
/// `scope` must come from [`invocation_scope`]. Best-effort through
/// the shared emitter: store failure/timeout surfaces counters plus a
/// warn and never fails the owning execution.
pub async fn emit_command_execute(
    emitter: &ExecutionAuditEmitter,
    audit: &TrustedExecutionAuditContext,
    digest_hex: &str,
    family: &str,
    outcome: &str,
    scope: &str,
) {
    let correlation = audit
        .chain()
        .correlation_id
        .as_deref()
        .filter(|correlation| !correlation.is_empty())
        .unwrap_or_else(|| audit.provenance().correlation_id());
    let event_id = deterministic_event_id(
        audit.provenance().decision_id(),
        &AuditAction::CommandExecute,
        correlation,
        scope,
    );
    emitter
        .emit_with(audit, |principal, provenance, chain| {
            command_execute_event(principal, provenance, chain, digest_hex, family, outcome)
                .with_event_id(event_id.clone())
        })
        .await;
}

/// Audit shape (family + command digest) for one completed bash
/// dispatch.
///
/// Returns `None` for the unified Git route: the
/// `GitMutationExecutor` owns that single `git_operation` event, so
/// the shell layer stays silent and native/bash-routed git converge
/// on one audit event. `Rejected` never dispatched: no event.
pub fn audit_shape_for_executor(executor: &ActualExecutor) -> Option<(&'static str, String)> {
    match executor {
        ActualExecutor::RawShell { command, .. } => Some(("shell", command_digest_str(command))),
        ActualExecutor::TestRunner { argv, .. } => Some(("test", command_digest_argv(argv))),
        ActualExecutor::NativeTool { argv, .. } => Some(("process", command_digest_argv(argv))),
        ActualExecutor::ManagedArgv { argv, .. } => Some(("process", command_digest_argv(argv))),
        ActualExecutor::PythonScript { script_hash, .. } => Some((
            "process",
            script_hash
                .clone()
                .unwrap_or_else(|| command_digest_str("")),
        )),
        ActualExecutor::Git { .. } | ActualExecutor::Rejected { .. } => None,
    }
}

/// Planned audit shape for one bash dispatch target, used when the
/// dispatch times out after the process was spawned.
///
/// Mirrors [`audit_shape_for_executor`] except the Git route maps to
/// the shell surface: the executor emits nothing on its error paths,
/// so a timed-out git dispatch is described exactly once as a shell
/// timeout over the argv digest (never a fabricated git outcome).
pub fn planned_audit_shape(target: &CommandDispatchTarget) -> Option<(&'static str, String)> {
    match target {
        CommandDispatchTarget::RouteToTestRunner { argv, .. } => {
            Some(("test", command_digest_argv(argv)))
        }
        CommandDispatchTarget::RouteToNativeTool { command, .. } => {
            Some(("process", command_digest_argv(&command.full_argv())))
        }
        CommandDispatchTarget::RouteToPythonScripting { script, .. } => {
            Some(("process", command_digest_str(script)))
        }
        CommandDispatchTarget::RouteToManagedProcess { command, .. } => {
            Some(("process", command_digest_argv(&command.full_argv())))
        }
        CommandDispatchTarget::RouteToGit { request, .. } => {
            Some(("shell", command_digest_argv(&request.argv)))
        }
        CommandDispatchTarget::RouteToShell { command, .. } => {
            Some(("shell", command_digest_str(command)))
        }
        CommandDispatchTarget::Rejected { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_matrix_covers_only_process_dispatching_tools() {
        assert_eq!(command_family_for_tool("bash"), Some("shell"));
        assert_eq!(command_family_for_tool("terminal"), Some("process"));
        assert_eq!(command_family_for_tool("test"), Some("test"));
        // Read-only, chat, artifact, planning, and git tools must never
        // produce command_execute: git mutations belong to the Git
        // executor's single git_operation event.
        for tool in [
            "read",
            "glob",
            "grep",
            "git",
            "edit",
            "write",
            "context_read",
            "tool_program",
            "question",
            "todowrite",
            "websearch",
            "webfetch",
            "lsp",
            "",
        ] {
            assert_eq!(
                command_family_for_tool(tool),
                None,
                "tool {tool} must not own command_execute"
            );
        }
    }

    #[test]
    fn argv_digest_distinguishes_joined_boundaries() {
        let left = command_digest_argv(&["a b".to_owned(), "c".to_owned()]);
        let right = command_digest_argv(&["a".to_owned(), "b c".to_owned()]);
        assert_ne!(left, right);
        assert!(left.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn invocation_scope_keys_retries_and_separates_outcomes() {
        let retry = invocation_scope(Some("inv-1"), "digest", OUTCOME_SUCCESS);
        assert_eq!(
            retry,
            invocation_scope(Some("inv-1"), "digest", OUTCOME_SUCCESS)
        );
        assert_ne!(
            retry,
            invocation_scope(Some("inv-1"), "digest", OUTCOME_FAILURE)
        );
        assert_ne!(
            retry,
            invocation_scope(Some("inv-2"), "digest", OUTCOME_SUCCESS)
        );
        // Legacy callers without a key fall back to the digest.
        assert_eq!(
            invocation_scope(None, "digest", OUTCOME_SUCCESS),
            invocation_scope(Some(""), "digest", OUTCOME_SUCCESS)
        );
    }

    #[test]
    fn executor_shapes_route_git_to_silence() {
        use crate::command_outcome::ActualExecutor;
        use std::path::PathBuf;
        // Real dispatches map to bounded families with digests.
        let (family, digest) = audit_shape_for_executor(&ActualExecutor::RawShell {
            command: "echo hi".to_owned(),
            argv: vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
        })
        .expect("raw shell dispatches");
        assert_eq!(family, "shell");
        assert_eq!(digest, command_digest_str("echo hi"));
        let (family, _) = audit_shape_for_executor(&ActualExecutor::TestRunner {
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            cwd: PathBuf::from("."),
        })
        .expect("test dispatches");
        assert_eq!(family, "test");
        let (family, _) = audit_shape_for_executor(&ActualExecutor::NativeTool {
            tool_name: "eggsearch".to_owned(),
            argv: vec!["eggsearch".to_owned()],
        })
        .expect("native dispatches");
        assert_eq!(family, "process");
        // The Git route stays silent: the executor owns git_operation.
        assert_eq!(
            audit_shape_for_executor(&ActualExecutor::Git {
                argv: vec!["git".to_owned(), "add".to_owned()],
                operation_label: "add".to_owned(),
            }),
            None
        );
        assert_eq!(
            audit_shape_for_executor(&ActualExecutor::Rejected {
                reason: "no".to_owned(),
            }),
            None
        );
    }

    #[test]
    fn planned_shapes_cover_every_dispatch_target() {
        use crate::command_intent::plan::{CommandDispatchTarget, NativeCommand};
        use std::path::PathBuf;
        let shell = CommandDispatchTarget::RouteToShell {
            command: "echo hi".to_owned(),
            timeout_secs: None,
        };
        assert_eq!(planned_audit_shape(&shell).expect("shell").0, "shell");
        let git = CommandDispatchTarget::RouteToGit {
            request: crate::command_intent::plan::GitExecutionRequest {
                operation: codegg_git::GitOperation::Status { short: false },
                argv: vec!["git".to_owned(), "status".to_owned()],
                command: "git status".to_owned(),
                origin: codegg_git::GitCommandOrigin::NativeTool,
                risk_set: codegg_git::RiskSet::read_only(),
                is_read_only: true,
                repository_root: None,
                managed_argv: None,
            },
            timeout_secs: None,
        };
        // Planned git shape exists for the timeout path (the executor
        // emits nothing on its error paths, so the shell timeout event
        // is the single record); the completed Git executor shape stays
        // silent above.
        assert_eq!(planned_audit_shape(&git).expect("git timeout").0, "shell");
        assert_eq!(
            planned_audit_shape(&CommandDispatchTarget::Rejected {
                reason: "no".to_owned(),
            }),
            None
        );
        let native = CommandDispatchTarget::RouteToNativeTool {
            tool_name: "eggsearch".to_owned(),
            command: NativeCommand {
                executable: "eggsearch".to_owned(),
                argv: vec![],
            },
        };
        assert_eq!(planned_audit_shape(&native).expect("native").0, "process");
        let managed = CommandDispatchTarget::RouteToManagedProcess {
            command: NativeCommand {
                executable: "cargo".to_owned(),
                argv: vec!["check".to_owned()],
            },
            cwd: PathBuf::from("."),
            timeout_secs: None,
        };
        assert_eq!(planned_audit_shape(&managed).expect("managed").0, "process");
    }
}
