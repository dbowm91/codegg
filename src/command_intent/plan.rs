use std::path::PathBuf;

use super::{CommandIntent, CommandIntentKind, ExecutionCapability, IntentConfidence, RiskLevel};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProjectorRoute {
    Raw,
    Truncated,
    ErrorRetention,
    GitStatus,
    GitDiff,
    GitLog,
    TestReport,
    FileSearch,
    PythonRun,
    RtkEligible(Box<ProjectorRoute>),
}

impl ProjectorRoute {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Truncated => "truncated",
            Self::ErrorRetention => "error-retention",
            Self::GitStatus => "git-status",
            Self::GitDiff => "git-diff",
            Self::GitLog => "git-log",
            Self::TestReport => "test-report",
            Self::FileSearch => "file-search",
            Self::PythonRun => "python-run",
            Self::RtkEligible(inner) => inner.label(),
        }
    }

    pub fn is_rtk_eligible(&self) -> bool {
        matches!(self, Self::RtkEligible(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PlanRtkPolicy {
    Disabled,
    Eligible {
        min_raw_bytes: usize,
        preserve_exact_spans: Vec<ProjectionSpanKind>,
        goal: CompressionGoal,
    },
    RequiredForPromotion,
}

impl PlanRtkPolicy {
    pub fn is_rtk_eligible(&self) -> bool {
        matches!(self, Self::Eligible { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProjectionSpanKind {
    CompilerErrors,
    TestFailureNames,
    FilePaths,
    LineNumbers,
    DiffHunks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CompressionGoal {
    ReduceTokens,
    PreserveSemantics,
    Maximal,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PermissionDefault {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandPermissionRequest {
    pub capability: ExecutionCapability,
    pub path: Option<PathBuf>,
    pub risk_level: RiskLevel,
    pub reason: String,
    pub default_decision: PermissionDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExecutionBackend {
    RawShell {
        command: String,
    },
    ManagedArgv {
        argv: Vec<String>,
        cwd: PathBuf,
    },
    NativeTool {
        tool_name: String,
    },
    TestRunner {
        validated_command: Option<String>,
    },
    PythonScript {
        script: String,
        mode_guess: PythonModeGuess,
    },
    GitMutating {
        tool_name: String,
        argv: Vec<String>,
    },
    Reject {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PythonModeGuess {
    Analyze,
    Transform,
    Verify,
    Unknown,
}

impl ExecutionBackend {
    pub fn label(&self) -> &str {
        match self {
            Self::RawShell { .. } => "raw-shell",
            Self::ManagedArgv { .. } => "managed-argv",
            Self::NativeTool { .. } => "native-tool",
            Self::TestRunner { .. } => "test-runner",
            Self::PythonScript { .. } => "python-script",
            Self::GitMutating { .. } => "git-mutating",
            Self::Reject { .. } => "reject",
        }
    }

    pub fn is_executable(&self) -> bool {
        !matches!(self, Self::Reject { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandPlan {
    pub intent: CommandIntent,
    pub backend: ExecutionBackend,
    pub permission_requests: Vec<CommandPermissionRequest>,
    pub projector: ProjectorRoute,
    pub rtk_policy: PlanRtkPolicy,
    pub context_policy: super::ContextPolicy,
    pub timeout_secs: Option<u64>,
    pub cwd: Option<PathBuf>,
    pub notes: Vec<String>,
}

impl CommandPlan {
    pub fn is_executable(&self) -> bool {
        self.backend.is_executable()
    }

    pub fn requires_any_permission(&self) -> bool {
        self.permission_requests.iter().any(|p| {
            matches!(
                p.default_decision,
                PermissionDefault::Ask | PermissionDefault::Deny
            )
        })
    }

    /// Validate that this plan is safe to execute via active routing.
    /// Returns Ok(()) if safe, Err(reason) if not.
    pub fn validate_for_active_routing(&self) -> Result<(), String> {
        // 1. Shell shape must be SimpleArgv (no complex shell)
        if self.intent.parsed_argv.is_none() {
            return Err("complex shell command not eligible for active routing".to_string());
        }

        // 2. Confidence must be High
        if self.intent.confidence != IntentConfidence::High {
            return Err(format!(
                "confidence {:?} is not High",
                self.intent.confidence
            ));
        }

        // 3. Backend must not be Reject or RawShell
        if matches!(
            self.backend,
            ExecutionBackend::Reject { .. } | ExecutionBackend::RawShell { .. }
        ) {
            return Err(format!(
                "backend {} is not eligible for active routing",
                self.backend.label()
            ));
        }

        // 4. Risk level must not be Critical
        if self.intent.risk.level == RiskLevel::Critical {
            return Err("critical risk level not eligible for active routing".to_string());
        }

        // 5. No DestructiveFileMutation capability
        if self
            .intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::DestructiveFileMutation)
        {
            return Err(
                "destructive file mutation capability not eligible for active routing".to_string(),
            );
        }

        // 6. No OutsideWorkspace capability
        if self
            .intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::OutsideWorkspace)
        {
            return Err("outside workspace capability not eligible for active routing".to_string());
        }

        // 7. Permissions must be resolved (no pending Ask/Deny permissions)
        if self.requires_any_permission() {
            return Err("pending permissions not eligible for active routing".to_string());
        }

        Ok(())
    }
}

pub fn plan_execution(intent: &CommandIntent) -> CommandPlan {
    let backend = select_backend(intent);
    let permission_requests = generate_permission_requests(intent, &backend);
    let projector = select_projector(intent, &backend);
    let rtk_policy = select_rtk_policy(intent, &backend);
    let timeout_secs = select_timeout(intent);
    let notes = Vec::new();

    CommandPlan {
        intent: intent.clone(),
        backend,
        permission_requests,
        projector,
        rtk_policy,
        context_policy: intent.context_policy,
        timeout_secs,
        cwd: None,
        notes,
    }
}

fn select_backend(intent: &CommandIntent) -> ExecutionBackend {
    match intent.kind {
        CommandIntentKind::Test => {
            let validated = validate_test_command(&intent.command);
            ExecutionBackend::TestRunner {
                validated_command: validated,
            }
        }
        CommandIntentKind::PythonAnalyze => ExecutionBackend::PythonScript {
            script: intent.command.clone(),
            mode_guess: PythonModeGuess::Analyze,
        },
        CommandIntentKind::PythonTransform => ExecutionBackend::PythonScript {
            script: intent.command.clone(),
            mode_guess: PythonModeGuess::Transform,
        },
        CommandIntentKind::PythonVerify => ExecutionBackend::PythonScript {
            script: intent.command.clone(),
            mode_guess: PythonModeGuess::Verify,
        },
        CommandIntentKind::GitReadOnly => ExecutionBackend::NativeTool {
            tool_name: "egggit".to_string(),
        },
        CommandIntentKind::GitMutating => {
            // Route safe git mutations to structured GitMutating backend;
            // dangerous mutations (push, reset --hard, clean -f, etc.) stay as RawShell.
            if let Some(argv) = &intent.parsed_argv {
                let subcmd = argv.get(1).map(String::as_str).unwrap_or("");
                let is_dangerous = matches!(subcmd, "push" | "pull" | "remote" | "tag")
                    || (subcmd == "reset" && argv.iter().any(|a| a == "--hard"))
                    || (subcmd == "clean"
                        && argv.iter().any(|a| a == "-f" || a == "-fd" || a == "-fx"))
                    || (subcmd == "branch" && argv.iter().any(|a| a == "-D" || a == "--delete"));

                if is_dangerous {
                    ExecutionBackend::RawShell {
                        command: intent.command.clone(),
                    }
                } else {
                    ExecutionBackend::GitMutating {
                        tool_name: "git".to_string(),
                        argv: argv.clone(),
                    }
                }
            } else {
                ExecutionBackend::RawShell {
                    command: intent.command.clone(),
                }
            }
        }
        CommandIntentKind::SearchReadOnly | CommandIntentKind::FileRead => {
            ExecutionBackend::ManagedArgv {
                argv: intent.parsed_argv.clone().unwrap_or_else(|| {
                    intent
                        .command
                        .split_whitespace()
                        .map(String::from)
                        .collect()
                }),
                cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            }
        }
        CommandIntentKind::Build | CommandIntentKind::Lint | CommandIntentKind::Format => {
            ExecutionBackend::ManagedArgv {
                argv: intent.parsed_argv.clone().unwrap_or_else(|| {
                    intent
                        .command
                        .split_whitespace()
                        .map(String::from)
                        .collect()
                }),
                cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            }
        }
        CommandIntentKind::FileWrite | CommandIntentKind::FileEdit => ExecutionBackend::RawShell {
            command: intent.command.clone(),
        },
        CommandIntentKind::RawShell => ExecutionBackend::RawShell {
            command: intent.command.clone(),
        },
        CommandIntentKind::Rejected => ExecutionBackend::Reject {
            reason: "command rejected by classifier".to_string(),
        },
    }
}

fn validate_test_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let test_prefixes = [
        "cargo test",
        "cargo nextest",
        "pytest",
        "uv run pytest",
        "go test",
        "npm test",
        "pnpm test",
        "yarn test",
        "bun test",
        "make test",
        "make check",
    ];
    for prefix in &test_prefixes {
        if trimmed.starts_with(prefix) {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn is_safe_git_subcommand(intent: &CommandIntent) -> bool {
    let subcmd = match intent.parsed_argv.as_ref().and_then(|v| v.get(1)) {
        Some(s) => s.as_str(),
        None => return false,
    };
    // Only git add is safe to auto-allow. Commit may run hooks,
    // checkout/switch/restore may overwrite worktree, stash push mutates state.
    subcmd == "add"
}

fn is_formatter_command(intent: &CommandIntent) -> bool {
    if intent.kind == CommandIntentKind::Format {
        return true;
    }
    let cmd = intent.command.to_lowercase();
    cmd.contains("cargo fmt")
        || cmd.contains("prettier")
        || cmd.contains("black ")
        || cmd.contains("isort")
        || cmd.contains("rustfmt")
}

fn is_read_only_formatter(intent: &CommandIntent) -> bool {
    let cmd = intent.command.to_lowercase();
    cmd.contains("--check") || cmd.contains("--diff") || cmd.contains("checkfmt")
}

fn generate_permission_requests(
    intent: &CommandIntent,
    backend: &ExecutionBackend,
) -> Vec<CommandPermissionRequest> {
    if matches!(backend, ExecutionBackend::Reject { .. }) {
        return Vec::new();
    }

    let mut perms = Vec::new();

    for cap in &intent.risk.capabilities {
        let (path, risk_level, reason, default_decision) = match cap {
            ExecutionCapability::ReadWorkspace => (
                None,
                RiskLevel::Safe,
                "read workspace files".to_string(),
                PermissionDefault::Allow,
            ),
            ExecutionCapability::WriteWorkspace => {
                // Writing formatters (cargo fmt, black, prettier --write, isort) mutate workspace files.
                // Default to Ask; read-only formatters (--check, --diff) don't write.
                let default = if is_formatter_command(intent) && !is_read_only_formatter(intent) {
                    PermissionDefault::Ask
                } else if is_formatter_command(intent) {
                    PermissionDefault::Allow
                } else {
                    PermissionDefault::Ask
                };
                (
                    None,
                    RiskLevel::Medium,
                    "write workspace files".to_string(),
                    default,
                )
            }
            ExecutionCapability::Subprocess => (
                None,
                RiskLevel::Low,
                "spawn subprocess".to_string(),
                PermissionDefault::Allow,
            ),
            ExecutionCapability::Network => (
                None,
                RiskLevel::Medium,
                "access network".to_string(),
                PermissionDefault::Ask,
            ),
            ExecutionCapability::EnvAccess => (
                None,
                RiskLevel::Low,
                "access environment variables".to_string(),
                PermissionDefault::Allow,
            ),
            ExecutionCapability::DependencyInstall => (
                None,
                RiskLevel::Medium,
                "install dependencies".to_string(),
                PermissionDefault::Deny,
            ),
            ExecutionCapability::OutsideWorkspace => (
                None,
                RiskLevel::High,
                "access files outside workspace".to_string(),
                PermissionDefault::Deny,
            ),
            ExecutionCapability::DestructiveFileMutation => (
                None,
                RiskLevel::High,
                "destructive file mutation".to_string(),
                PermissionDefault::Deny,
            ),
            ExecutionCapability::GitMutation => {
                let default = if is_safe_git_subcommand(intent) {
                    PermissionDefault::Allow
                } else {
                    PermissionDefault::Ask
                };
                (None, RiskLevel::Medium, "git mutation".to_string(), default)
            }
            ExecutionCapability::ContextPromotion => (
                None,
                RiskLevel::Low,
                "promote output to model context".to_string(),
                PermissionDefault::Allow,
            ),
        };

        perms.push(CommandPermissionRequest {
            capability: *cap,
            path,
            risk_level,
            reason,
            default_decision,
        });
    }

    perms
}

fn select_projector(intent: &CommandIntent, backend: &ExecutionBackend) -> ProjectorRoute {
    match intent.kind {
        CommandIntentKind::GitReadOnly => {
            if intent.command.starts_with("git diff") {
                ProjectorRoute::GitDiff
            } else if intent.command.starts_with("git log") {
                ProjectorRoute::GitLog
            } else {
                ProjectorRoute::GitStatus
            }
        }
        CommandIntentKind::GitMutating => {
            if matches!(backend, ExecutionBackend::GitMutating { .. }) {
                ProjectorRoute::Raw
            } else {
                ProjectorRoute::Truncated
            }
        }
        CommandIntentKind::Test => ProjectorRoute::TestReport,
        CommandIntentKind::SearchReadOnly | CommandIntentKind::FileRead => {
            ProjectorRoute::FileSearch
        }
        CommandIntentKind::PythonAnalyze
        | CommandIntentKind::PythonTransform
        | CommandIntentKind::PythonVerify => ProjectorRoute::PythonRun,
        CommandIntentKind::Build | CommandIntentKind::Lint | CommandIntentKind::Format => {
            ProjectorRoute::ErrorRetention
        }
        CommandIntentKind::FileWrite | CommandIntentKind::FileEdit => ProjectorRoute::Raw,
        CommandIntentKind::RawShell => ProjectorRoute::Truncated,
        CommandIntentKind::Rejected => ProjectorRoute::Raw,
    }
}

fn select_rtk_policy(intent: &CommandIntent, backend: &ExecutionBackend) -> PlanRtkPolicy {
    if matches!(backend, ExecutionBackend::Reject { .. }) {
        return PlanRtkPolicy::Disabled;
    }

    match intent.kind {
        CommandIntentKind::Test => PlanRtkPolicy::Eligible {
            min_raw_bytes: 4096,
            preserve_exact_spans: vec![
                ProjectionSpanKind::TestFailureNames,
                ProjectionSpanKind::FilePaths,
                ProjectionSpanKind::LineNumbers,
            ],
            goal: CompressionGoal::PreserveSemantics,
        },
        CommandIntentKind::GitReadOnly => {
            if intent.command.starts_with("git diff") {
                PlanRtkPolicy::Eligible {
                    min_raw_bytes: 2048,
                    preserve_exact_spans: vec![
                        ProjectionSpanKind::DiffHunks,
                        ProjectionSpanKind::FilePaths,
                        ProjectionSpanKind::LineNumbers,
                    ],
                    goal: CompressionGoal::PreserveSemantics,
                }
            } else {
                PlanRtkPolicy::Disabled
            }
        }
        CommandIntentKind::PythonAnalyze
        | CommandIntentKind::PythonTransform
        | CommandIntentKind::PythonVerify => PlanRtkPolicy::Eligible {
            min_raw_bytes: 2048,
            preserve_exact_spans: vec![
                ProjectionSpanKind::CompilerErrors,
                ProjectionSpanKind::FilePaths,
                ProjectionSpanKind::LineNumbers,
            ],
            goal: CompressionGoal::PreserveSemantics,
        },
        CommandIntentKind::RawShell => PlanRtkPolicy::Eligible {
            min_raw_bytes: 4096,
            preserve_exact_spans: vec![
                ProjectionSpanKind::FilePaths,
                ProjectionSpanKind::LineNumbers,
            ],
            goal: CompressionGoal::ReduceTokens,
        },
        CommandIntentKind::SearchReadOnly => PlanRtkPolicy::Eligible {
            min_raw_bytes: 4096,
            preserve_exact_spans: vec![ProjectionSpanKind::FilePaths],
            goal: CompressionGoal::ReduceTokens,
        },
        _ => PlanRtkPolicy::Disabled,
    }
}

fn select_timeout(intent: &CommandIntent) -> Option<u64> {
    match intent.kind {
        CommandIntentKind::Test => Some(300),
        CommandIntentKind::Build => Some(120),
        CommandIntentKind::PythonAnalyze | CommandIntentKind::PythonTransform => Some(60),
        CommandIntentKind::PythonVerify => Some(300),
        CommandIntentKind::GitReadOnly => Some(30),
        CommandIntentKind::GitMutating => Some(60),
        CommandIntentKind::SearchReadOnly => Some(30),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_intent::classify_command;

    #[test]
    fn cargo_test_routes_to_test_runner() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
        assert!(plan.is_executable());
    }

    #[test]
    fn cargo_nextest_routes_to_test_runner() {
        let intent = classify_command("cargo nextest run");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
    }

    #[test]
    fn pytest_routes_to_test_runner() {
        let intent = classify_command("pytest tests/");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
    }

    #[test]
    fn uv_run_pytest_routes_to_test_runner() {
        let intent = classify_command("uv run pytest");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
    }

    #[test]
    fn git_status_routes_to_native() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::NativeTool { ref tool_name } if tool_name == "egggit"
        ));
        assert!(!plan.requires_any_permission());
    }

    #[test]
    fn git_diff_routes_to_native_with_diff_projector() {
        let intent = classify_command("git diff HEAD~1");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::NativeTool { .. }));
        assert_eq!(plan.projector, ProjectorRoute::GitDiff);
    }

    #[test]
    fn git_log_routes_to_native_with_log_projector() {
        let intent = classify_command("git log --oneline -10");
        let plan = plan_execution(&intent);
        assert_eq!(plan.projector, ProjectorRoute::GitLog);
    }

    #[test]
    fn git_commit_requires_permission() {
        let intent = classify_command("git commit -m 'fix'");
        let plan = plan_execution(&intent);
        // commit may run hooks and mutate state; defaults to Ask
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_push_requires_permission() {
        let intent = classify_command("git push origin main");
        let plan = plan_execution(&intent);
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn rg_routes_to_managed_argv() {
        let intent = classify_command("rg 'fn main' src/");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::ManagedArgv { .. }));
        assert_eq!(plan.projector, ProjectorRoute::FileSearch);
    }

    #[test]
    fn python_inline_routes_to_analyze() {
        let intent = classify_command("python3 -c 'print(1)'");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::PythonScript {
                mode_guess: PythonModeGuess::Analyze,
                ..
            }
        ));
    }

    #[test]
    fn python_with_subprocess_is_high_risk() {
        let intent = classify_command("python3 -c 'import subprocess; subprocess.run([\"ls\"])'");
        let plan = plan_execution(&intent);
        // DestructiveFileMutation is now Deny (not Ask), but still High risk
        assert!(plan
            .permission_requests
            .iter()
            .any(|p| p.risk_level == RiskLevel::High));
    }

    #[test]
    fn cargo_test_and_rm_is_rejected_or_raw_shell() {
        let intent = classify_command("cargo test && rm -rf .");
        let plan = plan_execution(&intent);
        assert!(!matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
    }

    #[test]
    fn rejected_command_not_executable() {
        let intent = classify_command("");
        let plan = plan_execution(&intent);
        assert!(!plan.is_executable());
        assert!(matches!(plan.backend, ExecutionBackend::Reject { .. }));
    }

    #[test]
    fn test_command_has_test_report_projector() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert_eq!(plan.projector, ProjectorRoute::TestReport);
    }

    #[test]
    fn test_command_has_rtk_eligible_policy() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.rtk_policy, PlanRtkPolicy::Eligible { .. }));
    }

    #[test]
    fn git_diff_has_rtk_eligible_policy() {
        let intent = classify_command("git diff");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.rtk_policy, PlanRtkPolicy::Eligible { .. }));
    }

    #[test]
    fn short_git_status_rtk_disabled() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        assert_eq!(plan.rtk_policy, PlanRtkPolicy::Disabled);
    }

    #[test]
    fn command_plan_has_timeout() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert_eq!(plan.timeout_secs, Some(300));
    }

    #[test]
    fn build_command_has_error_retention_projector() {
        let intent = classify_command("cargo build");
        let plan = plan_execution(&intent);
        assert_eq!(plan.projector, ProjectorRoute::ErrorRetention);
    }

    #[test]
    fn raw_shell_has_truncated_projector() {
        let intent = classify_command("echo hello");
        let plan = plan_execution(&intent);
        assert_eq!(plan.projector, ProjectorRoute::Truncated);
    }

    #[test]
    fn raw_shell_has_rtk_eligible_policy() {
        let intent = classify_command("echo hello world this is a longer command");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.rtk_policy, PlanRtkPolicy::Eligible { .. }));
    }

    #[test]
    fn context_policy_preserved() {
        let intent = classify_command("git commit -m 'fix'");
        let plan = plan_execution(&intent);
        assert_eq!(plan.context_policy, intent.context_policy);
    }

    #[test]
    fn cat_routes_to_managed_argv() {
        let intent = classify_command("cat README.md");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::ManagedArgv { .. }));
    }

    #[test]
    fn cargo_fmt_routes_to_managed_argv() {
        let intent = classify_command("cargo fmt");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::ManagedArgv { .. }));
    }

    #[test]
    fn notes_starts_empty() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert!(plan.notes.is_empty());
    }

    #[test]
    fn rg_single_quoted_argv_flows_to_backend() {
        let intent = classify_command("rg 'fn main' src/");
        let plan = plan_execution(&intent);
        match &plan.backend {
            ExecutionBackend::ManagedArgv { argv, .. } => {
                assert_eq!(argv, &vec!["rg", "fn main", "src/"]);
            }
            other => panic!("expected ManagedArgv, got {:?}", other),
        }
    }

    #[test]
    fn cat_double_quoted_argv_flows_to_backend() {
        let intent = classify_command("cat \"my file.txt\"");
        let plan = plan_execution(&intent);
        match &plan.backend {
            ExecutionBackend::ManagedArgv { argv, .. } => {
                assert_eq!(argv, &vec!["cat", "my file.txt"]);
            }
            other => panic!("expected ManagedArgv, got {:?}", other),
        }
    }

    #[test]
    fn cargo_build_argv_flows_to_backend() {
        let intent = classify_command("cargo build --release");
        let plan = plan_execution(&intent);
        match &plan.backend {
            ExecutionBackend::ManagedArgv { argv, .. } => {
                assert_eq!(argv, &vec!["cargo", "build", "--release"]);
            }
            other => panic!("expected ManagedArgv, got {:?}", other),
        }
    }

    #[test]
    fn test_runner_uses_parsed_argv() {
        let intent = classify_command("cargo test --lib -p foo");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
    }

    // ── Git mutation routing tests (Workstream E) ────────────────────

    #[test]
    fn git_add_routes_to_git_mutating() {
        let intent = classify_command("git add src/main.rs");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::GitMutating {
                tool_name,
                argv,
            } if tool_name == "git" && argv[0] == "git" && argv[1] == "add"
        ));
    }

    #[test]
    fn git_commit_routes_to_git_mutating() {
        let intent = classify_command("git commit -m 'fix'");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::GitMutating {
                tool_name,
                argv,
            } if tool_name == "git" && argv[1] == "commit"
        ));
    }

    #[test]
    fn git_stash_routes_to_git_mutating() {
        let intent = classify_command("git stash push -m 'wip'");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::GitMutating {
                tool_name,
                argv,
            } if tool_name == "git" && argv[1] == "stash"
        ));
    }

    #[test]
    fn git_checkout_routes_to_git_mutating() {
        let intent = classify_command("git checkout main");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::GitMutating {
                tool_name,
                argv,
            } if tool_name == "git" && argv[1] == "checkout"
        ));
    }

    #[test]
    fn git_switch_routes_to_git_mutating() {
        let intent = classify_command("git switch -c new-branch");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::GitMutating {
                tool_name,
                argv,
            } if tool_name == "git" && argv[1] == "switch"
        ));
    }

    #[test]
    fn git_restore_routes_to_git_mutating() {
        let intent = classify_command("git restore src/main.rs");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::GitMutating {
                tool_name,
                argv,
            } if tool_name == "git" && argv[1] == "restore"
        ));
    }

    #[test]
    fn git_push_stays_raw_shell() {
        let intent = classify_command("git push origin main");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::RawShell { .. }));
    }

    #[test]
    fn git_reset_hard_stays_raw_shell() {
        let intent = classify_command("git reset --hard HEAD~1");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::RawShell { .. }));
    }

    #[test]
    fn git_clean_f_stays_raw_shell() {
        let intent = classify_command("git clean -f");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::RawShell { .. }));
    }

    #[test]
    fn git_branch_d_stays_raw_shell() {
        let intent = classify_command("git branch -D old-branch");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::RawShell { .. }));
    }

    #[test]
    fn git_merge_routes_to_git_mutating() {
        let intent = classify_command("git merge feature-branch");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::GitMutating { .. }));
    }

    #[test]
    fn git_mutating_backend_label() {
        let backend = ExecutionBackend::GitMutating {
            tool_name: "git".to_string(),
            argv: vec!["git".to_string(), "add".to_string()],
        };
        assert_eq!(backend.label(), "git-mutating");
        assert!(backend.is_executable());
    }

    // ── Validation tests (Workstream M) ──────────────────────────────

    #[test]
    fn build_command_passes_active_routing_validation() {
        let intent = classify_command("cargo build");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_ok());
    }

    #[test]
    fn test_command_passes_active_routing_validation() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_ok());
    }

    #[test]
    fn git_add_passes_active_routing_validation() {
        let intent = classify_command("git add src/main.rs");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_ok());
    }

    #[test]
    fn rejected_command_fails_active_routing_validation() {
        let intent = classify_command("");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_err());
    }

    #[test]
    fn raw_shell_fails_active_routing_validation() {
        let intent = classify_command("echo hello");
        let plan = plan_execution(&intent);
        let result = plan.validate_for_active_routing();
        assert!(result.is_err());
    }

    #[test]
    fn complex_shell_fails_active_routing_validation() {
        let intent = classify_command("cargo test && rm -rf .");
        let plan = plan_execution(&intent);
        let result = plan.validate_for_active_routing();
        assert!(result.is_err());
    }

    #[test]
    fn git_push_fails_active_routing_validation() {
        let intent = classify_command("git push origin main");
        let plan = plan_execution(&intent);
        let result = plan.validate_for_active_routing();
        assert!(result.is_err());
    }

    #[test]
    fn git_reset_hard_fails_active_routing_validation() {
        let intent = classify_command("git reset --hard HEAD~1");
        let plan = plan_execution(&intent);
        let result = plan.validate_for_active_routing();
        assert!(result.is_err());
    }

    #[test]
    fn lint_command_passes_active_routing_validation() {
        let intent = classify_command("cargo clippy");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_ok());
    }

    #[test]
    fn format_command_passes_active_routing_validation() {
        let intent = classify_command("cargo fmt");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_ok());
    }

    #[test]
    fn mypy_passes_active_routing_validation() {
        let intent = classify_command("mypy src/");
        let plan = plan_execution(&intent);
        assert!(plan.validate_for_active_routing().is_ok());
    }

    // ── Workstream B: Permission tightening tests ──────────────────

    #[test]
    fn git_merge_requires_permission() {
        let intent = classify_command("git merge feature-branch");
        let plan = plan_execution(&intent);
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_rebase_requires_permission() {
        let intent = classify_command("git rebase main");
        let plan = plan_execution(&intent);
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_cherry_pick_requires_permission() {
        let intent = classify_command("git cherry-pick abc123");
        let plan = plan_execution(&intent);
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_revert_requires_permission() {
        let intent = classify_command("git revert abc123");
        let plan = plan_execution(&intent);
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_checkout_requires_permission() {
        let intent = classify_command("git checkout main");
        let plan = plan_execution(&intent);
        // checkout may overwrite worktree; defaults to Ask
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_switch_requires_permission() {
        let intent = classify_command("git switch -c new-branch");
        let plan = plan_execution(&intent);
        // switch may overwrite worktree; defaults to Ask
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn git_stash_push_requires_permission() {
        let intent = classify_command("git stash push -m 'wip'");
        let plan = plan_execution(&intent);
        // stash push mutates state; defaults to Ask
        assert!(plan.requires_any_permission());
    }

    #[test]
    fn destructive_file_mutation_is_denied() {
        let intent = classify_command("rm -rf tmp/");
        let plan = plan_execution(&intent);
        for p in &plan.permission_requests {
            assert_ne!(p.capability, ExecutionCapability::DestructiveFileMutation);
        }
    }

    #[test]
    fn cargo_fmt_has_no_pending_permissions() {
        let intent = classify_command("cargo fmt");
        let plan = plan_execution(&intent);
        assert!(!plan.requires_any_permission());
    }

    #[test]
    fn prettier_format_has_no_pending_permissions() {
        let intent = classify_command("prettier --write src/");
        let plan = plan_execution(&intent);
        assert!(!plan.requires_any_permission());
    }

    #[test]
    fn black_format_has_no_pending_permissions() {
        let intent = classify_command("black src/");
        let plan = plan_execution(&intent);
        assert!(!plan.requires_any_permission());
    }
}
