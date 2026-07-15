use std::path::PathBuf;

use codegg_git::risk::RiskSet;
use codegg_git::{GitCommandOrigin, GitOperation, GitRiskClass};

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

/// A typed Git execution request carrying the parsed operation, argv,
/// repository context, and risk metadata. This is the unified Git backend
/// that replaces both `NativeTool { "egggit" }` for reads and
/// legacy `GitMutating` for mutations.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GitExecutionRequest {
    /// The typed parsed operation from codegg-git.
    pub operation: GitOperation,
    /// The original tokenized argv (preserved for audit and re-execution).
    pub argv: Vec<String>,
    /// The raw command string as entered by the user or model.
    pub command: String,
    /// Where this git command originated.
    pub origin: GitCommandOrigin,
    /// Risk classes derived from the typed operation.
    pub risk_set: RiskSet,
    /// Whether the operation is read-only.
    pub is_read_only: bool,
    /// Canonical repository root (resolved before planning).
    pub repository_root: Option<PathBuf>,
    /// The fallback argv for managed-unsupported operations (when the
    /// typed parser produces `ManagedGitArgv` or `RawShellRequired`).
    pub managed_argv: Option<Vec<String>>,
}

impl GitExecutionRequest {
    /// Create a request from parsed argv using the typed parser.
    pub fn from_argv(
        argv: Vec<String>,
        command: String,
        origin: GitCommandOrigin,
    ) -> Result<Self, codegg_git::ParseError> {
        let operation = codegg_git::parse_git_argv(&argv)?;
        let risk_set = operation.risk_classes();
        let is_read_only = risk_set.contains(&GitRiskClass::ReadOnly)
            && !risk_set
                .classes()
                .iter()
                .any(|c| *c != GitRiskClass::ReadOnly);
        let managed_argv = match &operation {
            GitOperation::ManagedGitArgv { argv, .. } => Some(argv.clone()),
            GitOperation::RawShellRequired { argv } => Some(argv.clone()),
            _ => None,
        };
        Ok(Self {
            operation,
            argv,
            command,
            origin,
            risk_set,
            is_read_only,
            repository_root: None,
            managed_argv,
        })
    }

    /// Create a request from an already-parsed `GitOperation`.
    pub fn from_operation(
        operation: GitOperation,
        argv: Vec<String>,
        command: String,
        origin: GitCommandOrigin,
    ) -> Self {
        let risk_set = operation.risk_classes();
        let is_read_only = risk_set.contains(&GitRiskClass::ReadOnly)
            && !risk_set
                .classes()
                .iter()
                .any(|c| *c != GitRiskClass::ReadOnly);
        let managed_argv = match &operation {
            GitOperation::ManagedGitArgv { argv, .. } => Some(argv.clone()),
            GitOperation::RawShellRequired { argv } => Some(argv.clone()),
            _ => None,
        };
        Self {
            operation,
            argv,
            command,
            origin,
            risk_set,
            is_read_only,
            repository_root: None,
            managed_argv,
        }
    }

    /// Whether this request requires network access.
    pub fn requires_network(&self) -> bool {
        self.risk_set.requires_network()
    }

    /// Whether this request involves destructive operations.
    pub fn is_destructive(&self) -> bool {
        self.risk_set.is_destructive()
    }

    /// Derive a `RiskLevel` from the risk set.
    pub fn risk_level(&self) -> RiskLevel {
        if self.is_read_only {
            RiskLevel::Safe
        } else if self.is_destructive() {
            RiskLevel::High
        } else if self.requires_network() {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        }
    }

    /// Derive `ExecutionCapability` flags from the risk set.
    pub fn capabilities(&self) -> Vec<ExecutionCapability> {
        let mut caps = vec![ExecutionCapability::ReadWorkspace];
        if !self.is_read_only {
            caps.push(ExecutionCapability::GitMutation);
        }
        if self.requires_network() {
            caps.push(ExecutionCapability::Network);
        }
        if self.is_destructive() {
            caps.push(ExecutionCapability::DestructiveFileMutation);
        }
        caps
    }
}

/// Map a typed `GitOperation` and its risk set to the appropriate
/// `CommandIntentFamily` for routing configuration lookup.
///
/// Risk precedence (highest wins):
///   `Destructive` > `Network` > `LocalMutation` > `Read`
///
/// This is the authoritative mapper — adding a new `GitOperation` variant
/// that should not be classified as `None` requires updating this match.
pub fn git_operation_family(
    operation: &GitOperation,
    risks: &RiskSet,
) -> Option<crate::config::schema::CommandIntentFamily> {
    use crate::config::schema::CommandIntentFamily;
    use codegg_git::GitRiskClass;

    if risks.is_destructive() {
        return Some(CommandIntentFamily::GitDestructive);
    }
    if risks.requires_network() {
        return Some(CommandIntentFamily::GitNetwork);
    }

    // A read-only request is the GitRead family.
    if risks.contains(&GitRiskClass::ReadOnly)
        && !risks.classes().iter().any(|c| *c != GitRiskClass::ReadOnly)
    {
        return Some(CommandIntentFamily::GitRead);
    }

    // Otherwise it's a local mutation (IndexMutation, WorktreeMutation,
    // RefMutation, HistoryIntegration, RepositoryConfigMutation).
    // The exact class doesn't matter for family assignment — anything
    // that mutates only the local repository is GitLocalMutation.
    match operation {
        // Pure reads already returned above.
        GitOperation::Status { .. }
        | GitOperation::Diff { .. }
        | GitOperation::DiffStaged { .. }
        | GitOperation::Show { .. }
        | GitOperation::Log { .. }
        | GitOperation::Blame { .. }
        | GitOperation::ChangedFiles { .. }
        | GitOperation::BranchList { .. }
        | GitOperation::RemoteList
        | GitOperation::RemoteGetUrl { .. }
        | GitOperation::TagList
        | GitOperation::WorktreeList
        | GitOperation::StashList
        | GitOperation::StashShow { .. }
        | GitOperation::ConfigGet { .. } => None,
        // Managed/unknown plumbing falls through — caller decides fallback.
        GitOperation::ManagedGitArgv { .. } | GitOperation::RawShellRequired { .. } => None,
        // Every other typed variant is a local mutation by definition:
        // Add, Reset (non-hard/merge/keep), Commit, StashPush/Apply/Pop/Drop,
        // Checkout (non-force paths), Switch, Restore, BranchCreate/Delete/Rename,
        // TagCreate/Delete/ForceDelete, Merge, Rebase, CherryPick, Revert,
        // ConfigSet/Unset, RemoteAdd/Remove/SetUrl, Abort/Continue/Skip.
        _ => Some(CommandIntentFamily::GitLocalMutation),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Unified Git backend — carries a typed request for all Git operations.
    Git {
        request: GitExecutionRequest,
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
            Self::Git { .. } => "git",
            Self::Reject { .. } => "reject",
        }
    }

    pub fn is_executable(&self) -> bool {
        !matches!(self, Self::Reject { .. })
    }

    /// If this is a `Git` backend, return the request.
    pub fn as_git_request(&self) -> Option<&GitExecutionRequest> {
        match self {
            Self::Git { request } => Some(request),
            _ => None,
        }
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
        CommandIntentKind::GitReadOnly => {
            // Unified Git backend: parse argv into typed request.
            if let Some(argv) = &intent.parsed_argv {
                match GitExecutionRequest::from_argv(
                    argv.clone(),
                    intent.command.clone(),
                    GitCommandOrigin::BashTranslation,
                ) {
                    Ok(request) => ExecutionBackend::Git { request },
                    Err(_) => {
                        // Parser failure: conservative fallback to managed argv.
                        ExecutionBackend::ManagedArgv {
                            argv: argv.clone(),
                            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                        }
                    }
                }
            } else {
                ExecutionBackend::NativeTool {
                    tool_name: "egggit".to_string(),
                }
            }
        }
        CommandIntentKind::GitMutating => {
            // Unified Git backend: parse argv into typed request.
            if let Some(argv) = &intent.parsed_argv {
                match GitExecutionRequest::from_argv(
                    argv.clone(),
                    intent.command.clone(),
                    GitCommandOrigin::BashTranslation,
                ) {
                    Ok(request) => ExecutionBackend::Git { request },
                    Err(_) => {
                        // Parser failure: conservative fallback to raw shell.
                        ExecutionBackend::RawShell {
                            command: intent.command.clone(),
                        }
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
    // Walk past simple global options (`-C <path>`, `--git-dir=<p>`,
    // etc.) to find the subcommand position. Track U: a `git -C <repo>
    // add ...` invocation should still classify as a safe mutation
    // because `add` itself is non-destructive. Only the subcommand
    // value determines safety, not the preceding flags.
    let argv = match intent.parsed_argv.as_ref() {
        Some(v) => v,
        None => return false,
    };
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg == "add" {
            return true;
        }
        // Skip `-C <path>` form
        if arg == "-C" {
            i += 2;
            continue;
        }
        // Skip `-C<path>` joined form
        if let Some(_rest) = arg.strip_prefix("-C") {
            i += 1;
            continue;
        }
        // Skip `--git-dir=<p>` or `-c <key>=<value>` (boolean flags we
        // don't expect to gate safety).
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        // First non-flag token is the subcommand; anything other than
        // `add` is not on the safe list.
        return false;
    }
    false
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
            if matches!(backend, ExecutionBackend::Git { .. }) {
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
    fn git_status_routes_to_git_backend() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
        assert!(!plan.requires_any_permission());
    }

    #[test]
    fn git_diff_routes_to_git_backend_with_diff_projector() {
        let intent = classify_command("git diff HEAD~1");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
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
    fn git_add_routes_to_git_backend() {
        let intent = classify_command("git add src/main.rs");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::Git { request } if request.argv[0] == "git" && request.argv[1] == "add"
        ));
    }

    #[test]
    fn git_commit_routes_to_git_backend() {
        let intent = classify_command("git commit -m 'fix'");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::Git { request } if request.argv[1] == "commit"
        ));
    }

    #[test]
    fn git_stash_routes_to_git_backend() {
        let intent = classify_command("git stash push -m 'wip'");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::Git { request } if request.argv[1] == "stash"
        ));
    }

    #[test]
    fn git_checkout_routes_to_git_backend() {
        let intent = classify_command("git checkout main");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::Git { request } if request.argv[1] == "checkout"
        ));
    }

    #[test]
    fn git_switch_routes_to_git_backend() {
        let intent = classify_command("git switch -c new-branch");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::Git { request } if request.argv[1] == "switch"
        ));
    }

    #[test]
    fn git_restore_routes_to_git_backend() {
        let intent = classify_command("git restore src/main.rs");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::Git { request } if request.argv[1] == "restore"
        ));
    }

    #[test]
    fn git_push_routes_to_git_backend() {
        let intent = classify_command("git push origin main");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
    }

    #[test]
    fn git_reset_hard_routes_to_git_backend() {
        let intent = classify_command("git reset --hard HEAD~1");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
    }

    #[test]
    fn git_clean_f_routes_to_git_backend() {
        let intent = classify_command("git clean -f");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
    }

    #[test]
    fn git_branch_d_routes_to_git_backend() {
        let intent = classify_command("git branch -D old-branch");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
    }

    #[test]
    fn git_merge_routes_to_git_backend() {
        let intent = classify_command("git merge feature-branch");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));
    }

    #[test]
    fn git_backend_label() {
        let request = GitExecutionRequest::from_argv(
            vec!["git".to_string(), "add".to_string()],
            "git add".to_string(),
            GitCommandOrigin::BashTranslation,
        )
        .unwrap();
        let backend = ExecutionBackend::Git { request };
        assert_eq!(backend.label(), "git");
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
        // push requires permission (GitMutation → Ask), so fails validation
        let result = plan.validate_for_active_routing();
        assert!(result.is_err());
    }

    #[test]
    fn git_reset_hard_fails_active_routing_validation() {
        let intent = classify_command("git reset --hard HEAD~1");
        let plan = plan_execution(&intent);
        // reset --hard has DestructiveFileMutation capability → Deny, so fails
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

    // ── Track U: git_operation_family unit tests ────────────────────────

    use super::git_operation_family;
    use crate::config::schema::CommandIntentFamily;

    fn family_for_argv(argv: &[&str]) -> Option<CommandIntentFamily> {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let op = codegg_git::parse_git_argv(&argv).expect("argv must parse");
        let risks = op.risk_classes();
        git_operation_family(&op, &risks)
    }

    #[test]
    fn family_read_for_status_diff_log() {
        assert_eq!(
            family_for_argv(&["git", "status"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "diff"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "log", "--oneline", "-10"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "show", "HEAD"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "blame", "src/main.rs"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "branch", "--list"]),
            Some(CommandIntentFamily::GitRead)
        );
        // `git remote` with no args lists remotes.
        assert_eq!(
            family_for_argv(&["git", "remote"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "tag", "--list"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "stash", "list"]),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            family_for_argv(&["git", "config", "--get", "user.name"]),
            Some(CommandIntentFamily::GitRead)
        );
    }

    #[test]
    fn family_local_mutation_for_add_commit_branch_stash_restore() {
        assert_eq!(
            family_for_argv(&["git", "add", "src/main.rs"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "commit", "-m", "fix"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "switch", "-c", "feature/x"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "checkout", "main"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "restore", "src/main.rs"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "stash", "push", "-m", "wip"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "branch", "feature/x"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "merge", "feature/y"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "rebase", "main"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "cherry-pick", "abc123"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "revert", "abc123"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
        assert_eq!(
            family_for_argv(&["git", "config", "user.name", "Alice"]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
    }

    #[test]
    fn family_network_for_fetch_pull_push_remote() {
        assert_eq!(
            family_for_argv(&["git", "fetch", "origin"]),
            Some(CommandIntentFamily::GitNetwork)
        );
        assert_eq!(
            family_for_argv(&["git", "pull", "origin", "main"]),
            Some(CommandIntentFamily::GitNetwork)
        );
        assert_eq!(
            family_for_argv(&["git", "push", "origin", "main"]),
            Some(CommandIntentFamily::GitNetwork)
        );
        assert_eq!(
            family_for_argv(&[
                "git",
                "remote",
                "add",
                "upstream",
                "https://example.com/r.git"
            ]),
            Some(CommandIntentFamily::GitLocalMutation)
        );
    }

    #[test]
    fn family_destructive_for_reset_hard_clean_force_push() {
        assert_eq!(
            family_for_argv(&["git", "reset", "--hard", "HEAD~1"]),
            Some(CommandIntentFamily::GitDestructive)
        );
        assert_eq!(
            family_for_argv(&["git", "clean", "-f"]),
            Some(CommandIntentFamily::GitDestructive)
        );
        assert_eq!(
            family_for_argv(&["git", "push", "--force", "origin", "main"]),
            Some(CommandIntentFamily::GitDestructive)
        );
        assert_eq!(
            family_for_argv(&["git", "push", "--force-with-lease", "origin", "main"]),
            Some(CommandIntentFamily::GitDestructive)
        );
    }

    #[test]
    fn family_destructive_beats_network_when_both_present() {
        // force push is BOTH network write AND destructive — destructive wins.
        let op = codegg_git::parse_git_argv(&[
            "git".to_string(),
            "push".to_string(),
            "--force".to_string(),
            "origin".to_string(),
            "main".to_string(),
        ])
        .unwrap();
        let risks = op.risk_classes();
        assert_eq!(
            git_operation_family(&op, &risks),
            Some(CommandIntentFamily::GitDestructive)
        );
    }

    #[test]
    fn family_none_for_unknown_plumbing() {
        let op = codegg_git::parse_git_argv(&[
            "git".to_string(),
            "rev-list".to_string(),
            "--left-right".to_string(),
            "main...HEAD".to_string(),
        ])
        .unwrap();
        let risks = op.risk_classes();
        // ManagedGitArgv has non-empty WorktreeMutation risk but is fallback.
        assert_eq!(git_operation_family(&op, &risks), None);
    }
}
