use std::path::PathBuf;

use codegg_core::run_store::{ActualBackend, FallbackRecord, PlannedBackend, RunOwnership};
use serde::{Deserialize, Serialize};

use crate::command_intent::CommandIntentKind;

/// Exact invocation details for what was actually run.
///
/// `RunInvocation` in `run_store` is the persistence-side mirror of this type.
/// `ActualInvocation` exists at the routing layer so the dispatcher can record
/// what *really* happened independent of any planning metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActualInvocation {
    /// `sh -c <command>` — canonical raw-shell path.
    RawShell {
        command: String,
        argv: Vec<String>,
    },
    /// Direct `Command::new(argv[0]).args(argv[1..])` — managed process path.
    ManagedArgv {
        argv: Vec<String>,
        cwd: Option<PathBuf>,
    },
    /// Native tool dispatch (e.g., egggit).
    NativeTool {
        tool_name: String,
        argv: Vec<String>,
    },
    /// Canonical TestRunner invocation — argv used to spawn the test child.
    TestRunner {
        argv: Vec<String>,
        cwd: PathBuf,
        scope_label: String,
    },
    /// Canonical Python subsystem invocation — script body + mode.
    PythonScript {
        script_hash: Option<String>,
        mode: String,
        argv: Vec<String>,
    },
}

impl ActualInvocation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RawShell { .. } => "raw_shell",
            Self::ManagedArgv { .. } => "managed_argv",
            Self::NativeTool { .. } => "native_tool",
            Self::TestRunner { .. } => "test_runner",
            Self::PythonScript { .. } => "python_script",
        }
    }
}

/// What was actually used to execute the command.
///
/// This is distinct from `ActualBackend` because:
///   - `ActualBackend` is the persistence-side enum for RunStore
///   - `ActualExecutor` carries the *full* argv + cwd + invocation details
///     needed to faithfully reconstruct or rerun what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActualExecutor {
    RawShell {
        command: String,
        argv: Vec<String>,
    },
    ManagedArgv {
        argv: Vec<String>,
        cwd: Option<PathBuf>,
    },
    NativeTool {
        tool_name: String,
        argv: Vec<String>,
    },
    /// TestRunner with the actual argv that was executed.
    /// Note: TestRunner has its own canonical RunStore record; this variant
    /// is set on the BashTool side ONLY if BashTool does NOT delegate
    /// (i.e., when BashTool decides to route through TestRunner but
    /// TestRunner is unavailable, so BashTool falls back to raw shell).
    TestRunner {
        argv: Vec<String>,
        cwd: PathBuf,
    },
    PythonScript {
        script_hash: Option<String>,
        mode: String,
    },
    Rejected {
        reason: String,
    },
}

impl ActualExecutor {
    pub fn into_backend(&self) -> ActualBackend {
        match self {
            Self::RawShell { .. } => ActualBackend::RawShell,
            Self::ManagedArgv { .. } => ActualBackend::ManagedArgv,
            Self::NativeTool { .. } => ActualBackend::NativeTool,
            Self::TestRunner { .. } => ActualBackend::TestRunner,
            Self::PythonScript { .. } => ActualBackend::PythonScript,
            Self::Rejected { reason } => ActualBackend::Rejected { reason: reason.clone() },
        }
    }

    pub fn label(&self) -> &'static str {
        self.into_backend().label()
    }

    pub fn into_invocation(&self) -> ActualInvocation {
        match self {
            Self::RawShell { command, argv } => ActualInvocation::RawShell {
                command: command.clone(),
                argv: argv.clone(),
            },
            Self::ManagedArgv { argv, cwd } => ActualInvocation::ManagedArgv {
                argv: argv.clone(),
                cwd: cwd.clone(),
            },
            Self::NativeTool { tool_name, argv } => ActualInvocation::NativeTool {
                tool_name: tool_name.clone(),
                argv: argv.clone(),
            },
            Self::TestRunner { argv, cwd } => ActualInvocation::TestRunner {
                argv: argv.clone(),
                cwd: cwd.clone(),
                scope_label: "command-intent:test".to_string(),
            },
            Self::PythonScript { script_hash, mode } => ActualInvocation::PythonScript {
                script_hash: script_hash.clone(),
                mode: mode.clone(),
                argv: vec!["python3".to_string(), "<script>".to_string()],
            },
            Self::Rejected { .. } => ActualInvocation::RawShell {
                command: "<rejected>".to_string(),
                argv: vec!["sh".to_string(), "-c".to_string(), "<rejected>".to_string()],
            },
        }
    }
}

/// The full outcome of a dispatch attempt: what was planned, what ran, and
/// whether any fallback occurred.
#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    /// The routing decision the planner selected.
    pub planned: PlannedBackend,
    /// What actually executed (or was rejected).
    pub actual: ActualExecutor,
    /// True iff the actual executor diverged from the planned backend.
    pub fallback: bool,
    /// Reason text for the fallback, when `fallback == true`.
    pub fallback_reason: Option<String>,
}

impl ExecutionOutcome {
    /// Build an outcome where the planned executor matches the actual executor.
    pub fn identity(planned: PlannedBackend, actual: ActualExecutor) -> Self {
        Self {
            planned,
            actual,
            fallback: false,
            fallback_reason: None,
        }
    }

    /// Build a fallback outcome: planned backend X failed, actual executor Y ran.
    pub fn with_fallback(
        planned: PlannedBackend,
        actual: ActualExecutor,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            planned,
            actual,
            fallback: true,
            fallback_reason: Some(reason.into()),
        }
    }

    /// Rejected without execution.
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            planned: PlannedBackend::Unrouted,
            actual: ActualExecutor::Rejected {
                reason: reason.into(),
            },
            fallback: false,
            fallback_reason: None,
        }
    }

    /// Convert to the persistence-side `FallbackRecord` if a fallback occurred.
    pub fn fallback_record(&self) -> Option<FallbackRecord> {
        if !self.fallback {
            return None;
        }
        let actual = self.actual.into_backend();
        Some(FallbackRecord {
            planned: self.planned.clone(),
            actual,
            reason: self.fallback_reason.clone().unwrap_or_default(),
        })
    }
}

/// Map an `ExecutionOutcome` to a `RunOwnership` decision.
///
/// Rules:
///   - `ActualExecutor::TestRunner` or `PythonScript` → `DelegatedBackend`
///     (BashTool MUST NOT persist its own record).
///   - Everything else (raw shell, managed argv, native tool) → `Caller`.
pub fn ownership_for_outcome(outcome: &ExecutionOutcome) -> RunOwnership {
    match &outcome.actual {
        ActualExecutor::TestRunner { .. } | ActualExecutor::PythonScript { .. } => {
            RunOwnership::DelegatedBackend
        }
        ActualExecutor::RawShell { .. }
        | ActualExecutor::ManagedArgv { .. }
        | ActualExecutor::NativeTool { .. }
        | ActualExecutor::Rejected { .. } => RunOwnership::Caller,
    }
}

/// Map an `ExecutionOutcome` to a `RunKind` based on what actually executed.
///
/// Workstream D: when the actual executor is `RawShell`, `RunKind` is
/// unconditionally `raw_shell` regardless of the classified intent. Semantic
/// intent remains available separately through `planned_backend`, routing
/// metadata, and intent kind. This keeps `RunKind` a faithful record of the
/// execution substrate — not an overloading of semantic intent.
pub fn run_kind_for_outcome(outcome: &ExecutionOutcome, intent_kind: CommandIntentKind) -> String {
    use crate::command_intent::CommandIntentKind::*;
    let _ = intent_kind; // unused after Workstream D — preserved for API stability
    match &outcome.actual {
        ActualExecutor::RawShell { .. } => "raw_shell".to_string(),
        ActualExecutor::ManagedArgv { .. } => match intent_kind {
            GitMutating => "git_mutation".to_string(),
            SearchReadOnly | FileRead => "search".to_string(),
            _ => "managed_process".to_string(),
        },
        ActualExecutor::NativeTool { .. } => match intent_kind {
            GitReadOnly => "git_read".to_string(),
            _ => "native_tool".to_string(),
        },
        ActualExecutor::TestRunner { .. } => "test".to_string(),
        ActualExecutor::PythonScript { .. } => "python".to_string(),
        ActualExecutor::Rejected { .. } => "raw_shell".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_outcome_has_no_fallback() {
        let outcome = ExecutionOutcome::identity(
            PlannedBackend::RawShell,
            ActualExecutor::RawShell {
                command: "echo hi".to_string(),
                argv: vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()],
            },
        );
        assert!(!outcome.fallback);
        assert!(outcome.fallback_record().is_none());
        assert_eq!(
            ownership_for_outcome(&outcome),
            RunOwnership::Caller
        );
    }

    #[test]
    fn with_fallback_records_reason() {
        let outcome = ExecutionOutcome::with_fallback(
            PlannedBackend::TestRunner,
            ActualExecutor::RawShell {
                command: "cargo test".to_string(),
                argv: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "cargo test".to_string(),
                ],
            },
            "test runner unavailable",
        );
        assert!(outcome.fallback);
        let rec = outcome.fallback_record().unwrap();
        assert_eq!(rec.planned, PlannedBackend::TestRunner);
        assert_eq!(rec.actual, ActualBackend::RawShell);
        assert_eq!(rec.reason, "test runner unavailable");
    }

    #[test]
    fn delegated_backends_get_delegated_ownership() {
        let outcome = ExecutionOutcome::identity(
            PlannedBackend::PythonScript,
            ActualExecutor::PythonScript {
                script_hash: Some("abc".to_string()),
                mode: "analyze".to_string(),
            },
        );
        assert_eq!(
            ownership_for_outcome(&outcome),
            RunOwnership::DelegatedBackend
        );

        let outcome = ExecutionOutcome::identity(
            PlannedBackend::TestRunner,
            ActualExecutor::TestRunner {
                argv: vec!["cargo".to_string(), "test".to_string()],
                cwd: PathBuf::from("."),
            },
        );
        assert_eq!(
            ownership_for_outcome(&outcome),
            RunOwnership::DelegatedBackend
        );
    }

    #[test]
    fn rejected_outcome_is_caller_owned() {
        let outcome = ExecutionOutcome::rejected("test");
        assert_eq!(
            ownership_for_outcome(&outcome),
            RunOwnership::Caller
        );
    }

    #[test]
    fn run_kind_for_outcome_resolves_correctly() {
        let outcome = ExecutionOutcome::identity(
            PlannedBackend::ManagedArgv,
            ActualExecutor::ManagedArgv {
                argv: vec!["rg".to_string(), "pattern".to_string()],
                cwd: None,
            },
        );
        assert_eq!(
            run_kind_for_outcome(&outcome, CommandIntentKind::SearchReadOnly),
            "search"
        );

        let outcome = ExecutionOutcome::identity(
            PlannedBackend::GitMutating,
            ActualExecutor::ManagedArgv {
                argv: vec!["git".to_string(), "commit".to_string()],
                cwd: None,
            },
        );
        assert_eq!(
            run_kind_for_outcome(&outcome, CommandIntentKind::GitMutating),
            "git_mutation"
        );
    }

    /// Workstream D: raw-shell actual executor must ALWAYS produce RunKind
    /// "raw_shell", regardless of intent. Semantic intent remains accessible
    /// via `planned_backend` and `intent_kind`.
    #[test]
    fn run_kind_for_outcome_raw_shell_unconditional() {
        let intent_kinds = [
            CommandIntentKind::GitReadOnly,
            CommandIntentKind::GitMutating,
            CommandIntentKind::SearchReadOnly,
            CommandIntentKind::FileRead,
            CommandIntentKind::Test,
            CommandIntentKind::PythonAnalyze,
            CommandIntentKind::Build,
            CommandIntentKind::RawShell,
        ];
        for kind in intent_kinds {
            let outcome = ExecutionOutcome::identity(
                PlannedBackend::RawShell,
                ActualExecutor::RawShell {
                    command: "echo".to_string(),
                    argv: vec!["sh".to_string(), "-c".to_string(), "echo".to_string()],
                },
            );
            assert_eq!(
                run_kind_for_outcome(&outcome, kind),
                "raw_shell",
                "RawShell actual executor MUST produce RunKind::raw_shell regardless of intent ({:?})",
                kind
            );
        }
    }
}