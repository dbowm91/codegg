use std::path::PathBuf;

use super::{CommandIntent, CommandIntentKind, ExecutionCapability, RiskLevel};

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
        CommandIntentKind::GitMutating => ExecutionBackend::RawShell {
            command: intent.command.clone(),
        },
        CommandIntentKind::SearchReadOnly | CommandIntentKind::FileRead => {
            ExecutionBackend::ManagedArgv {
                argv: intent
                    .command
                    .split_whitespace()
                    .map(String::from)
                    .collect(),
                cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            }
        }
        CommandIntentKind::Build | CommandIntentKind::Lint | CommandIntentKind::Format => {
            ExecutionBackend::ManagedArgv {
                argv: intent
                    .command
                    .split_whitespace()
                    .map(String::from)
                    .collect(),
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
            ExecutionCapability::WriteWorkspace => (
                None,
                RiskLevel::Medium,
                "write workspace files".to_string(),
                PermissionDefault::Ask,
            ),
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
                PermissionDefault::Ask,
            ),
            ExecutionCapability::OutsideWorkspace => (
                None,
                RiskLevel::High,
                "access files outside workspace".to_string(),
                PermissionDefault::Ask,
            ),
            ExecutionCapability::DestructiveFileMutation => (
                None,
                RiskLevel::High,
                "destructive file mutation".to_string(),
                PermissionDefault::Ask,
            ),
            ExecutionCapability::GitMutation => (
                None,
                RiskLevel::Medium,
                "git mutation".to_string(),
                PermissionDefault::Ask,
            ),
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

fn select_projector(intent: &CommandIntent, _backend: &ExecutionBackend) -> ProjectorRoute {
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
        CommandIntentKind::GitMutating => ProjectorRoute::Raw,
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
        assert!(plan.requires_any_permission());
        assert!(plan
            .permission_requests
            .iter()
            .any(|p| matches!(p.default_decision, PermissionDefault::Ask)));
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
        assert!(plan.permission_requests.iter().any(|p| {
            p.risk_level == RiskLevel::High && p.default_decision == PermissionDefault::Ask
        }));
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
}
