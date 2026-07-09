pub mod plan;
pub mod shell_shape;

// Re-export CommandIntentMode from the config schema crate.
pub use crate::config::schema::CommandIntentMode;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CommandSource {
    AgentTool,
    HumanShell,
    TestRunner,
    PythonScript,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CommandOrigin {
    BashTool,
    TestSlashCommand,
    HumanShellBang,
    HumanShellDoubleBang,
    PythonScripting,
    DirectExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CommandIntentKind {
    Test,
    GitReadOnly,
    GitMutating,
    SearchReadOnly,
    FileRead,
    FileWrite,
    FileEdit,
    Build,
    Lint,
    Format,
    PythonAnalyze,
    PythonTransform,
    PythonVerify,
    RawShell,
    Rejected,
}

impl CommandIntentKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::GitReadOnly => "git-readonly",
            Self::GitMutating => "git-mutating",
            Self::SearchReadOnly => "search-readonly",
            Self::FileRead => "file-read",
            Self::FileWrite => "file-write",
            Self::FileEdit => "file-edit",
            Self::Build => "build",
            Self::Lint => "lint",
            Self::Format => "format",
            Self::PythonAnalyze => "python-analyze",
            Self::PythonTransform => "python-transform",
            Self::PythonVerify => "python-verify",
            Self::RawShell => "raw-shell",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntentConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RiskLevel {
    Safe,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExecutionCapability {
    ReadWorkspace,
    WriteWorkspace,
    Subprocess,
    Network,
    EnvAccess,
    DependencyInstall,
    OutsideWorkspace,
    DestructiveFileMutation,
    GitMutation,
    ContextPromotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ContextPolicy {
    ProjectToModel,
    LocalOnly,
    StoreOnly,
    Promote,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub reasons: Vec<String>,
    pub capabilities: Vec<ExecutionCapability>,
}

impl RiskAssessment {
    pub fn safe() -> Self {
        Self {
            level: RiskLevel::Safe,
            reasons: vec![],
            capabilities: vec![ExecutionCapability::ReadWorkspace],
        }
    }

    pub fn low(reason: &str) -> Self {
        Self {
            level: RiskLevel::Low,
            reasons: vec![reason.to_string()],
            capabilities: vec![
                ExecutionCapability::ReadWorkspace,
                ExecutionCapability::Subprocess,
            ],
        }
    }

    pub fn medium(reason: &str) -> Self {
        Self {
            level: RiskLevel::Medium,
            reasons: vec![reason.to_string()],
            capabilities: vec![
                ExecutionCapability::ReadWorkspace,
                ExecutionCapability::Subprocess,
            ],
        }
    }

    pub fn high(reason: &str) -> Self {
        Self {
            level: RiskLevel::High,
            reasons: vec![reason.to_string()],
            capabilities: vec![
                ExecutionCapability::ReadWorkspace,
                ExecutionCapability::Subprocess,
                ExecutionCapability::DestructiveFileMutation,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandIntent {
    pub kind: CommandIntentKind,
    pub confidence: IntentConfidence,
    pub risk: RiskAssessment,
    pub source: CommandSource,
    pub command: String,
    pub context_policy: ContextPolicy,
    /// Parsed argv from shell word parsing. `None` for complex shell commands
    /// where argv parsing failed or was not applicable.
    pub parsed_argv: Option<Vec<String>>,
}

impl CommandIntent {
    pub fn is_safe_for_model_context(&self) -> bool {
        matches!(self.risk.level, RiskLevel::Safe | RiskLevel::Low)
            && matches!(
                self.context_policy,
                ContextPolicy::ProjectToModel | ContextPolicy::Promote
            )
    }

    pub fn requires_permission(&self) -> bool {
        matches!(
            self.risk.level,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        )
    }
}

pub fn classify_command(command: &str) -> CommandIntent {
    let trimmed = command.trim();

    let shape = shell_shape::parse_shell_words(trimmed);

    match shape {
        shell_shape::ShellShape::Empty => CommandIntent {
            kind: CommandIntentKind::Rejected,
            confidence: IntentConfidence::High,
            risk: RiskAssessment::safe(),
            source: CommandSource::Unknown,
            command: trimmed.to_string(),
            context_policy: ContextPolicy::LocalOnly,
            parsed_argv: None,
        },
        shell_shape::ShellShape::ComplexShell { reasons } => {
            let reason_str = reasons
                .iter()
                .map(|r| format!("{:?}", r))
                .collect::<Vec<_>>()
                .join(", ");
            CommandIntent {
                kind: CommandIntentKind::RawShell,
                confidence: IntentConfidence::Low,
                risk: RiskAssessment::medium(&format!("complex shell: {}", reason_str)),
                source: CommandSource::AgentTool,
                command: trimmed.to_string(),
                context_policy: ContextPolicy::ProjectToModel,
                parsed_argv: None,
            }
        }
        shell_shape::ShellShape::SimpleArgv(argv) => {
            let first = argv.first().map(String::as_str).unwrap_or("");

            if looks_like_test_command(first, &argv) {
                return classify_test(trimmed, &argv);
            }

            if looks_like_python(first, &argv) {
                return classify_python(trimmed, &argv);
            }

            if looks_like_git(first) {
                return classify_git(trimmed, &argv);
            }

            if looks_like_file_read(first) {
                if let Some(intent) = classify_file_read(trimmed, &argv) {
                    return intent;
                }
            }

            if looks_like_search(first) {
                if let Some(intent) = classify_search(trimmed, &argv) {
                    return intent;
                }
            }

            if looks_like_build(first, &argv) {
                return classify_build(trimmed, &argv);
            }

            CommandIntent {
                kind: CommandIntentKind::RawShell,
                confidence: IntentConfidence::Low,
                risk: RiskAssessment::medium("unclassified command"),
                source: CommandSource::AgentTool,
                command: trimmed.to_string(),
                context_policy: ContextPolicy::ProjectToModel,
                parsed_argv: Some(argv),
            }
        }
    }
}

/// Check if the first argument matches.
fn first_arg_is(argv: &[String], name: &str) -> bool {
    argv.first().map(String::as_str) == Some(name)
}

/// Check for `cmd subcmd subsubcmd` pattern (e.g., `uv run pytest`).
fn has_subcommand(argv: &[String], cmd: &str, subcmd: &str, subsubcmd: &str) -> bool {
    argv.len() >= 3 && argv[0] == cmd && argv[1] == subcmd && argv[2] == subsubcmd
}

fn looks_like_python(first: &str, argv: &[String]) -> bool {
    matches!(first, "python" | "python3")
        || (first == "uv"
            && argv.len() >= 3
            && argv[1] == "run"
            && (argv[2] == "python" || argv[2] == "pytest"))
        || first == "pytest"
}

fn classify_python(command: &str, argv: &[String]) -> CommandIntent {
    let kind = if first_arg_is(argv, "pytest") || has_subcommand(argv, "uv", "run", "pytest") {
        CommandIntentKind::PythonVerify
    } else if first_arg_is(argv, "python") || first_arg_is(argv, "python3") {
        if argv.iter().any(|a| a == "-c") {
            CommandIntentKind::PythonAnalyze
        } else {
            CommandIntentKind::PythonTransform
        }
    } else {
        CommandIntentKind::PythonTransform
    };

    let risk = if command.contains("open(") || command.contains("write(") {
        RiskAssessment::medium("python script with file I/O")
    } else if command.contains("subprocess") || command.contains("os.system") {
        RiskAssessment::high("python script with subprocess calls")
    } else {
        RiskAssessment::low("python script")
    };

    CommandIntent {
        kind,
        confidence: IntentConfidence::High,
        risk,
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
        parsed_argv: Some(argv.to_vec()),
    }
}

fn looks_like_test_command(first: &str, argv: &[String]) -> bool {
    matches!(
        first,
        "cargo" | "pytest" | "go" | "npm" | "pnpm" | "yarn" | "bun" | "make"
    ) && (first == "cargo" && argv.len() >= 2 && (argv[1] == "test" || argv[1] == "nextest"))
        || (first == "pytest")
        || (first == "make" && argv.len() >= 2 && (argv[1] == "test" || argv[1] == "check"))
        || (first == "go" && argv.len() >= 2 && argv[1] == "test")
        || (matches!(first, "npm" | "pnpm" | "yarn" | "bun")
            && argv.len() >= 2
            && argv[1] == "test")
        || has_subcommand(argv, "uv", "run", "pytest")
}

fn classify_test(command: &str, argv: &[String]) -> CommandIntent {
    let risk = if argv.iter().any(|a| a == "--force" || a == "-y") {
        RiskAssessment::low("test command with force flag")
    } else {
        RiskAssessment::safe()
    };

    CommandIntent {
        kind: CommandIntentKind::Test,
        confidence: IntentConfidence::High,
        risk,
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
        parsed_argv: Some(argv.to_vec()),
    }
}

fn looks_like_git(first: &str) -> bool {
    first == "git"
}

fn classify_git(command: &str, argv: &[String]) -> CommandIntent {
    let subcmd = argv.get(1).map(String::as_str).unwrap_or("");

    let result = match subcmd {
        // Always read-only subcommands
        "status" | "diff" | "log" | "show" => GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        },

        // git branch — inspect flags/args
        "branch" => classify_git_branch(argv),

        // git tag — inspect flags
        "tag" => classify_git_tag(argv),

        // git remote — inspect subcommand
        "remote" => classify_git_remote(argv),

        // git stash — only `stash list` is read-only
        "stash" => {
            let third = argv.get(2).map(String::as_str);
            if third == Some("list") || third == Some("ls") {
                GitClass {
                    readonly: true,
                    risk: RiskLevel::Safe,
                    reason: None,
                }
            } else {
                GitClass {
                    readonly: false,
                    risk: RiskLevel::Medium,
                    reason: Some("git stash mutates state"),
                }
            }
        }

        // Always-mutating subcommands (low risk)
        "add" | "restore" | "checkout" | "switch" | "commit" | "merge" | "rebase"
        | "cherry-pick" | "revert" => GitClass {
            readonly: false,
            risk: RiskLevel::Low,
            reason: Some("git mutating command"),
        },

        // Push — high risk
        "push" => GitClass {
            readonly: false,
            risk: RiskLevel::High,
            reason: Some("git push"),
        },

        // Pull — medium risk (can auto-merge)
        "pull" => GitClass {
            readonly: false,
            risk: RiskLevel::Medium,
            reason: Some("git pull"),
        },

        // Reset — check for --hard
        "reset" => {
            if argv.iter().any(|a| a == "--hard") {
                GitClass {
                    readonly: false,
                    risk: RiskLevel::High,
                    reason: Some("git reset --hard"),
                }
            } else {
                GitClass {
                    readonly: false,
                    risk: RiskLevel::Medium,
                    reason: Some("git reset"),
                }
            }
        }

        // Clean — check for -f
        "clean" => {
            if argv.iter().any(|a| a == "-f" || a == "-fd" || a == "-fx") {
                GitClass {
                    readonly: false,
                    risk: RiskLevel::High,
                    reason: Some("git clean -f"),
                }
            } else {
                GitClass {
                    readonly: false,
                    risk: RiskLevel::Medium,
                    reason: Some("git clean"),
                }
            }
        }

        // Everything else (e.g., git rm, git mv, unknown subcommands)
        _ => GitClass {
            readonly: false,
            risk: RiskLevel::Low,
            reason: Some("git command"),
        },
    };

    let kind = if result.readonly {
        CommandIntentKind::GitReadOnly
    } else {
        CommandIntentKind::GitMutating
    };

    let risk = if result.readonly {
        RiskAssessment::safe()
    } else {
        let mut r = match result.risk {
            RiskLevel::High => RiskAssessment::high(result.reason.unwrap_or("git mutating")),
            RiskLevel::Medium => RiskAssessment::medium(result.reason.unwrap_or("git mutating")),
            _ => RiskAssessment::low(result.reason.unwrap_or("git mutating")),
        };
        r.capabilities.push(ExecutionCapability::GitMutation);
        r
    };

    CommandIntent {
        kind,
        confidence: IntentConfidence::High,
        risk,
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: if result.readonly {
            ContextPolicy::ProjectToModel
        } else {
            ContextPolicy::Promote
        },
        parsed_argv: Some(argv.to_vec()),
    }
}

struct GitClass {
    readonly: bool,
    risk: RiskLevel,
    reason: Option<&'static str>,
}

fn classify_git_branch(argv: &[String]) -> GitClass {
    // git branch (no args beyond subcommand) — list branches
    if argv.len() <= 2 {
        return GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        };
    }

    let third = argv.get(2).map(String::as_str).unwrap_or("");

    // Read-only flags
    if argv.iter().any(|a| {
        a == "--list"
            || a == "-l"
            || a == "--show-current"
            || a == "--contains"
            || a == "--merged"
            || a == "--no-merged"
            || a == "--all"
            || a == "--remotes"
    }) {
        return GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        };
    }

    // `git branch --sort=...` without a name arg — read-only
    if third.starts_with("--sort=") && !argv.iter().skip(3).any(|a| !a.starts_with('-')) {
        return GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        };
    }

    // Mutating flags: -d, -D, -m, -M, --delete, --move, --edit-description, --set-upstream-to, --unset-upstream
    if argv.iter().any(|a| {
        matches!(
            a.as_str(),
            "-d" | "-D"
                | "-m"
                | "-M"
                | "--delete"
                | "--move"
                | "--edit-description"
                | "--set-upstream-to"
                | "--unset-upstream"
        )
    }) {
        return GitClass {
            readonly: false,
            risk: RiskLevel::Medium,
            reason: Some("git branch delete/rename"),
        };
    }

    // A non-flag argument means creating a branch: `git branch <name>`
    if !third.is_empty() && !third.starts_with('-') {
        return GitClass {
            readonly: false,
            risk: RiskLevel::Low,
            reason: Some("git branch create"),
        };
    }

    // Default: treat as mutating (safest fallback for unknown flags)
    GitClass {
        readonly: false,
        risk: RiskLevel::Low,
        reason: Some("git branch"),
    }
}

fn classify_git_tag(argv: &[String]) -> GitClass {
    // git tag (no args beyond subcommand) — list tags
    if argv.len() <= 2 {
        return GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        };
    }

    // Read-only flags
    if argv.iter().any(|a| a == "--list" || a == "-l") {
        return GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        };
    }

    // `git tag -l 'pattern'` — still read-only (handled above)

    // Delete flag
    if argv.iter().any(|a| a == "-d" || a == "--delete") {
        return GitClass {
            readonly: false,
            risk: RiskLevel::Medium,
            reason: Some("git tag delete"),
        };
    }

    // Creating a tag: `git tag <name>` (non-flag arg)
    let third = argv.get(2).map(String::as_str).unwrap_or("");
    if !third.is_empty() && !third.starts_with('-') {
        return GitClass {
            readonly: false,
            risk: RiskLevel::Low,
            reason: Some("git tag create"),
        };
    }

    // Default: mutating
    GitClass {
        readonly: false,
        risk: RiskLevel::Low,
        reason: Some("git tag"),
    }
}

fn classify_git_remote(argv: &[String]) -> GitClass {
    // git remote (no args beyond subcommand) — list remotes
    if argv.len() <= 2 {
        return GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        };
    }

    let third = argv.get(2).map(String::as_str).unwrap_or("");

    match third {
        // Read-only subcommands
        "-v" | "show" | "get-url" | "prune" => GitClass {
            readonly: true,
            risk: RiskLevel::Safe,
            reason: None,
        },
        // Mutating subcommands
        "add" | "remove" | "rm" | "rename" | "set-url" => GitClass {
            readonly: false,
            risk: RiskLevel::Medium,
            reason: Some("git remote mutate"),
        },
        _ => GitClass {
            readonly: false,
            risk: RiskLevel::Low,
            reason: Some("git remote"),
        },
    }
}

fn looks_like_search(first: &str) -> bool {
    matches!(first, "rg" | "grep" | "fd" | "find" | "ls" | "pwd" | "wc")
}

/// Check if a find-style flag is destructive.
fn is_find_destructive_flag(arg: &str) -> bool {
    matches!(arg, "-exec" | "-delete" | "-ok" | "-execdir")
        || (arg.starts_with("-exec") && arg.len() > 5 && !arg.as_bytes()[5].is_ascii_alphanumeric())
        || (arg.starts_with("-ok") && arg.len() > 3 && !arg.as_bytes()[3].is_ascii_alphanumeric())
}

fn classify_search(command: &str, argv: &[String]) -> Option<CommandIntent> {
    let first = argv.first().map(String::as_str).unwrap_or("");

    // For find: reject if argv contains destructive flags
    if first == "find" && argv.iter().any(|a| is_find_destructive_flag(a)) {
        return None;
    }

    // Reject if any path argument is absolute and outside workspace
    if argv
        .iter()
        .skip(1)
        .any(|a| !a.starts_with('-') && is_absolute_path_outside_workspace(a))
    {
        return None;
    }

    Some(CommandIntent {
        kind: CommandIntentKind::SearchReadOnly,
        confidence: IntentConfidence::High,
        risk: RiskAssessment::safe(),
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
        parsed_argv: Some(argv.to_vec()),
    })
}

fn looks_like_build(first: &str, argv: &[String]) -> bool {
    if first == "cargo" && argv.len() >= 2 {
        matches!(
            argv[1].as_str(),
            "build" | "check" | "clippy" | "fmt" | "run"
        )
    } else {
        matches!(first, "make" | "cmake" | "npm" | "pnpm")
    }
}

fn classify_build(command: &str, argv: &[String]) -> CommandIntent {
    let second = argv.get(1).map(String::as_str).unwrap_or("");
    let kind = if matches!(second, "fmt" | "clippy") {
        CommandIntentKind::Lint
    } else {
        CommandIntentKind::Build
    };

    CommandIntent {
        kind,
        confidence: IntentConfidence::High,
        risk: RiskAssessment::safe(),
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
        parsed_argv: Some(argv.to_vec()),
    }
}

fn looks_like_file_read(first: &str) -> bool {
    matches!(first, "cat" | "less" | "more" | "head" | "tail")
}

fn classify_file_read(command: &str, argv: &[String]) -> Option<CommandIntent> {
    // Reject if any path argument is absolute and outside workspace
    if argv
        .iter()
        .skip(1)
        .any(|a| !a.starts_with('-') && is_absolute_path_outside_workspace(a))
    {
        return None;
    }

    Some(CommandIntent {
        kind: CommandIntentKind::FileRead,
        confidence: IntentConfidence::High,
        risk: RiskAssessment::safe(),
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
        parsed_argv: Some(argv.to_vec()),
    })
}

/// Check if a path is absolute and resolves outside the current workspace.
fn is_absolute_path_outside_workspace(path: &str) -> bool {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match std::fs::canonicalize(p) {
        Ok(canonical) => !canonical.starts_with(&cwd),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── General classification tests ──────────────────────────────────

    #[test]
    fn empty_command_is_rejected() {
        let intent = classify_command("");
        assert_eq!(intent.kind, CommandIntentKind::Rejected);
        assert_eq!(intent.confidence, IntentConfidence::High);
    }

    #[test]
    fn cargo_test_classified() {
        let intent = classify_command("cargo test");
        assert_eq!(intent.kind, CommandIntentKind::Test);
        assert_eq!(intent.confidence, IntentConfidence::High);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn pytest_classified() {
        let intent = classify_command("pytest tests/");
        assert_eq!(intent.kind, CommandIntentKind::Test);
        assert_eq!(intent.confidence, IntentConfidence::High);
    }

    #[test]
    fn rg_is_search() {
        let intent = classify_command("rg 'fn main' src/");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn cargo_build_is_build() {
        let intent = classify_command("cargo build --release");
        assert_eq!(intent.kind, CommandIntentKind::Build);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn cargo_fmt_is_lint() {
        let intent = classify_command("cargo fmt");
        assert_eq!(intent.kind, CommandIntentKind::Lint);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn python_script_classified() {
        let intent = classify_command("python3 script.py");
        assert_eq!(intent.kind, CommandIntentKind::PythonTransform);
        assert_eq!(intent.confidence, IntentConfidence::High);
    }

    #[test]
    fn python_with_subprocess_is_high_risk() {
        let intent = classify_command("python3 -c 'import subprocess; subprocess.run([\"ls\"])'");
        assert_eq!(intent.risk.level, RiskLevel::High);
    }

    #[test]
    fn unclassified_is_raw_shell() {
        let intent = classify_command("echo hello world");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
        assert_eq!(intent.confidence, IntentConfidence::Low);
    }

    #[test]
    fn cat_is_file_read() {
        let intent = classify_command("cat README.md");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn make_test_is_test() {
        let intent = classify_command("make test");
        assert_eq!(intent.kind, CommandIntentKind::Test);
    }

    // ── Shell parsing tests ───────────────────────────────────────────

    #[test]
    fn rg_single_quoted_arg_parsed_correctly() {
        let intent = classify_command("rg 'fn main' src/");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        let argv = intent.parsed_argv.unwrap();
        assert_eq!(argv, vec!["rg", "fn main", "src/"]);
    }

    #[test]
    fn cargo_test_and_rm_is_complex_shell() {
        let intent = classify_command("cargo test && rm -rf .");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
        assert!(intent.parsed_argv.is_none());
    }

    #[test]
    fn python_inline_single_quotes_parsed_correctly() {
        let intent = classify_command("python -c 'print(1)'");
        assert_eq!(intent.kind, CommandIntentKind::PythonAnalyze);
        let argv = intent.parsed_argv.unwrap();
        assert_eq!(argv, vec!["python", "-c", "print(1)"]);
    }

    #[test]
    fn echo_double_quoted_space_parsed_correctly() {
        let intent = classify_command("echo \"hello world\"");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
        let argv = intent.parsed_argv.unwrap();
        assert_eq!(argv, vec!["echo", "hello world"]);
    }

    #[test]
    fn redirection_command_is_complex() {
        let intent = classify_command("echo hello > file.txt");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
        assert!(intent.parsed_argv.is_none());
    }

    #[test]
    fn unbalanced_quote_is_complex() {
        let intent = classify_command("echo 'hello");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
        assert!(intent.parsed_argv.is_none());
    }

    #[test]
    fn empty_command_has_no_argv() {
        let intent = classify_command("");
        assert_eq!(intent.kind, CommandIntentKind::Rejected);
        assert!(intent.parsed_argv.is_none());
    }

    #[test]
    fn parsed_argv_present_for_simple_commands() {
        let intent = classify_command("cargo test --lib");
        assert!(intent.parsed_argv.is_some());
        assert_eq!(intent.parsed_argv.unwrap(), vec!["cargo", "test", "--lib"]);
    }

    #[test]
    fn cargo_build_argv_parsed() {
        let intent = classify_command("cargo build --release");
        assert_eq!(intent.kind, CommandIntentKind::Build);
        let argv = intent.parsed_argv.unwrap();
        assert_eq!(argv, vec!["cargo", "build", "--release"]);
    }

    // ── Git read-only tests ───────────────────────────────────────────

    #[test]
    fn git_status_is_readonly() {
        let intent = classify_command("git status");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
        assert!(intent.parsed_argv.is_some());
    }

    #[test]
    fn git_diff_is_readonly() {
        let intent = classify_command("git diff HEAD~1");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_diff_unstaged_is_readonly() {
        let intent = classify_command("git diff --stat");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_log_is_readonly() {
        let intent = classify_command("git log --oneline -10");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_show_is_readonly() {
        let intent = classify_command("git show HEAD");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_branch_show_current_is_readonly() {
        let intent = classify_command("git branch --show-current");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_branch_list_is_readonly() {
        let intent = classify_command("git branch --list");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_branch_l_is_readonly() {
        let intent = classify_command("git branch -l");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_branch_no_args_is_readonly() {
        let intent = classify_command("git branch");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_branch_contains_is_readonly() {
        let intent = classify_command("git branch --contains HEAD");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_branch_merged_is_readonly() {
        let intent = classify_command("git branch --merged main");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_branch_all_is_readonly() {
        let intent = classify_command("git branch --all");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_stash_list_is_readonly() {
        let intent = classify_command("git stash list");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_stash_ls_is_readonly() {
        let intent = classify_command("git stash ls");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_remote_v_is_readonly() {
        let intent = classify_command("git remote -v");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_remote_show_is_readonly() {
        let intent = classify_command("git remote show origin");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_remote_get_url_is_readonly() {
        let intent = classify_command("git remote get-url origin");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_remote_no_args_is_readonly() {
        let intent = classify_command("git remote");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_tag_list_is_readonly() {
        let intent = classify_command("git tag --list");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_tag_l_is_readonly() {
        let intent = classify_command("git tag -l");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    #[test]
    fn git_tag_no_args_is_readonly() {
        let intent = classify_command("git tag");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
    }

    // ── Git mutating tests ────────────────────────────────────────────

    #[test]
    fn git_branch_create_is_mutating() {
        let intent = classify_command("git branch my-feature");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
        assert!(intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::GitMutation));
    }

    #[test]
    fn git_branch_d_is_mutating() {
        let intent = classify_command("git branch -d old-branch");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_branch_D_is_mutating() {
        let intent = classify_command("git branch -D force-delete");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_branch_m_is_mutating() {
        let intent = classify_command("git branch -m old new");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_branch_delete_flag_is_mutating() {
        let intent = classify_command("git branch --delete old-branch");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_tag_create_is_mutating() {
        let intent = classify_command("git tag v1.0");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
        assert!(intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::GitMutation));
    }

    #[test]
    fn git_tag_d_is_mutating() {
        let intent = classify_command("git tag -d v1.0");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_remote_add_is_mutating() {
        let intent = classify_command("git remote add origin https://example.com");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_remote_remove_is_mutating() {
        let intent = classify_command("git remote remove origin");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_remote_set_url_is_mutating() {
        let intent = classify_command("git remote set-url origin https://new.com");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_stash_is_mutating() {
        let intent = classify_command("git stash");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_stash_push_is_mutating() {
        let intent = classify_command("git stash push -m 'wip'");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
    }

    #[test]
    fn git_push_is_mutating_high_risk() {
        let intent = classify_command("git push origin main");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::High);
        assert!(intent.requires_permission());
        assert!(intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::GitMutation));
    }

    #[test]
    fn git_push_tags_is_mutating_high_risk() {
        let intent = classify_command("git push --tags");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::High);
    }

    #[test]
    fn git_pull_is_mutating_medium_risk() {
        let intent = classify_command("git pull origin main");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_pull_rebase_is_mutating() {
        let intent = classify_command("git pull --rebase");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_merge_is_mutating() {
        let intent = classify_command("git merge feature-branch");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_rebase_is_mutating() {
        let intent = classify_command("git rebase main");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_cherry_pick_is_mutating() {
        let intent = classify_command("git cherry-pick abc123");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_revert_is_mutating() {
        let intent = classify_command("git revert abc123");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_reset_is_mutating_medium() {
        let intent = classify_command("git reset HEAD~1");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_reset_hard_is_mutating_high() {
        let intent = classify_command("git reset --hard HEAD~1");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::High);
    }

    #[test]
    fn git_reset_soft_is_mutating_medium() {
        let intent = classify_command("git reset --soft HEAD~1");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_clean_is_mutating_medium() {
        let intent = classify_command("git clean -n");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Medium);
    }

    #[test]
    fn git_clean_f_is_mutating_high() {
        let intent = classify_command("git clean -f");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::High);
    }

    #[test]
    fn git_clean_fd_is_mutating_high() {
        let intent = classify_command("git clean -fd");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::High);
    }

    #[test]
    fn git_add_is_mutating() {
        let intent = classify_command("git add src/main.rs");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_commit_is_mutating() {
        let intent = classify_command("git commit -m 'fix'");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_restore_is_mutating() {
        let intent = classify_command("git restore src/main.rs");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_checkout_is_mutating() {
        let intent = classify_command("git checkout main");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn git_switch_is_mutating() {
        let intent = classify_command("git switch -c new-branch");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    // ── Git capability and context policy tests ────────────────────────

    #[test]
    fn git_readonly_has_no_git_mutation_capability() {
        let intent = classify_command("git status");
        assert!(!intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::GitMutation));
    }

    #[test]
    fn git_readonly_context_is_project_to_model() {
        let intent = classify_command("git log");
        assert_eq!(intent.context_policy, ContextPolicy::ProjectToModel);
    }

    #[test]
    fn git_mutating_context_is_promote() {
        let intent = classify_command("git commit -m 'fix'");
        assert_eq!(intent.context_policy, ContextPolicy::Promote);
    }

    #[test]
    fn git_push_has_high_risk_capabilities() {
        let intent = classify_command("git push");
        assert!(intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::GitMutation));
        assert!(intent
            .risk
            .capabilities
            .contains(&ExecutionCapability::DestructiveFileMutation));
    }

    #[test]
    fn git_branch_create_argv_parsed() {
        let intent = classify_command("git branch my-feature");
        let argv = intent.parsed_argv.unwrap();
        assert_eq!(argv, vec!["git", "branch", "my-feature"]);
    }

    #[test]
    fn git_push_argv_parsed() {
        let intent = classify_command("git push origin main");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        let argv = intent.parsed_argv.unwrap();
        assert_eq!(argv, vec!["git", "push", "origin", "main"]);
    }

    // ── Search/read classification tests (Workstream F) ─────────────

    #[test]
    fn find_simple_is_search() {
        let intent = classify_command("find . -name '*.rs'");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn find_exec_is_not_safe() {
        let intent = classify_command("find . -exec rm {} \\;");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn find_delete_is_not_safe() {
        let intent = classify_command("find . -delete");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn find_ok_is_not_safe() {
        let intent = classify_command("find . -ok rm {} \\;");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn find_execdir_is_not_safe() {
        let intent = classify_command("find . -execdir rm {} \\;");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn find_absolute_path_outside_workspace_is_not_safe() {
        let intent = classify_command("find /etc -name passwd");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn cat_relative_path_is_file_read() {
        let intent = classify_command("cat src/main.rs");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn rg_is_search_read_only() {
        let intent = classify_command("rg 'fn main' src/");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn ls_is_search_read_only() {
        let intent = classify_command("ls -la");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn which_is_raw_shell() {
        let intent = classify_command("which python3");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn whereis_is_raw_shell() {
        let intent = classify_command("whereis rustc");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn cat_absolute_outside_workspace_is_raw_shell() {
        let intent = classify_command("cat /etc/passwd");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn head_absolute_outside_workspace_is_raw_shell() {
        let intent = classify_command("head -n 10 /var/log/syslog");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn type_is_raw_shell() {
        let intent = classify_command("type ls");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn grep_is_search() {
        let intent = classify_command("grep -r 'TODO' src/");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn wc_is_search() {
        let intent = classify_command("wc -l src/main.rs");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn find_delete_flag_not_confused_with_exec() {
        // -delete should be caught even though it doesn't start with -exec
        let intent = classify_command("find . -delete");
        assert_eq!(intent.kind, CommandIntentKind::RawShell);
    }

    #[test]
    fn is_find_destructive_flag_exact_matches() {
        assert!(is_find_destructive_flag("-exec"));
        assert!(is_find_destructive_flag("-delete"));
        assert!(is_find_destructive_flag("-ok"));
        assert!(is_find_destructive_flag("-execdir"));
    }

    #[test]
    fn is_find_destructive_flag_prefix_non_match() {
        // -executable should NOT match -exec
        assert!(!is_find_destructive_flag("-executable"));
        // -okdir should NOT match -ok
        assert!(!is_find_destructive_flag("-okdir"));
    }

    #[test]
    fn less_is_file_read() {
        let intent = classify_command("less README.md");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn more_is_file_read() {
        let intent = classify_command("more README.md");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn head_is_file_read() {
        let intent = classify_command("head README.md");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn tail_is_file_read() {
        let intent = classify_command("tail README.md");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn head_with_flags_is_file_read() {
        let intent = classify_command("head -n 5 README.md");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn tail_with_flags_is_file_read() {
        let intent = classify_command("tail -f src/main.rs");
        assert_eq!(intent.kind, CommandIntentKind::FileRead);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }
}
