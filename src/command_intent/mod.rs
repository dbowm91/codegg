pub mod plan;

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

    if trimmed.is_empty() {
        return CommandIntent {
            kind: CommandIntentKind::Rejected,
            confidence: IntentConfidence::High,
            risk: RiskAssessment::safe(),
            source: CommandSource::Unknown,
            command: trimmed.to_string(),
            context_policy: ContextPolicy::LocalOnly,
        };
    }

    if has_shell_operators(trimmed) {
        return CommandIntent {
            kind: CommandIntentKind::RawShell,
            confidence: IntentConfidence::Low,
            risk: RiskAssessment::medium("complex shell with operators"),
            source: CommandSource::AgentTool,
            command: trimmed.to_string(),
            context_policy: ContextPolicy::ProjectToModel,
        };
    }

    if looks_like_test_command(trimmed) {
        return classify_test(trimmed);
    }

    if looks_like_python(trimmed) {
        return classify_python(trimmed);
    }

    if looks_like_git(trimmed) {
        return classify_git(trimmed);
    }

    if looks_like_file_read(trimmed) {
        return classify_file_read(trimmed);
    }

    if looks_like_search(trimmed) {
        return classify_search(trimmed);
    }

    if looks_like_build(trimmed) {
        return classify_build(trimmed);
    }

    CommandIntent {
        kind: CommandIntentKind::RawShell,
        confidence: IntentConfidence::Low,
        risk: RiskAssessment::medium("unclassified command"),
        source: CommandSource::AgentTool,
        command: trimmed.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
    }
}

fn has_shell_operators(command: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if in_single || in_double {
            continue;
        }
        if ch == '|' || ch == ';' || ch == '$' || ch == '`' || ch == '&' {
            return true;
        }
    }
    false
}

fn looks_like_python(command: &str) -> bool {
    command.starts_with("python ")
        || command.starts_with("python3 ")
        || command.starts_with("uv run python")
        || command.starts_with("uv run pytest")
        || command.starts_with("pytest")
        || command.ends_with(".py")
}

fn classify_python(command: &str) -> CommandIntent {
    let kind = if command.starts_with("pytest") || command.starts_with("uv run pytest") {
        CommandIntentKind::PythonVerify
    } else if command.starts_with("python -c") || command.starts_with("python3 -c") {
        CommandIntentKind::PythonAnalyze
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
    }
}

fn looks_like_test_command(command: &str) -> bool {
    command.starts_with("cargo test")
        || command.starts_with("cargo nextest")
        || command.starts_with("pytest")
        || command.starts_with("uv run pytest")
        || command.starts_with("go test")
        || command.starts_with("npm test")
        || command.starts_with("pnpm test")
        || command.starts_with("yarn test")
        || command.starts_with("bun test")
        || command.starts_with("make test")
        || command.starts_with("make check")
}

fn classify_test(command: &str) -> CommandIntent {
    let risk = if command.contains("--force") || command.contains("-y") {
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
    }
}

fn looks_like_git(command: &str) -> bool {
    command.starts_with("git ")
}

fn classify_git(command: &str) -> CommandIntent {
    let is_readonly = command.starts_with("git status")
        || command.starts_with("git diff")
        || command.starts_with("git log")
        || command.starts_with("git show")
        || command.starts_with("git branch")
        || command.starts_with("git remote")
        || command.starts_with("git stash list")
        || command.starts_with("git tag");

    let kind = if is_readonly {
        CommandIntentKind::GitReadOnly
    } else {
        CommandIntentKind::GitMutating
    };

    let risk = if is_readonly {
        RiskAssessment::safe()
    } else if command.starts_with("git push") || command.starts_with("git reset --hard") {
        let mut r = RiskAssessment::high("git push or hard reset");
        r.capabilities.push(ExecutionCapability::GitMutation);
        r
    } else {
        let mut r = RiskAssessment::low("git mutating command");
        r.capabilities.push(ExecutionCapability::GitMutation);
        r
    };

    CommandIntent {
        kind,
        confidence: IntentConfidence::High,
        risk,
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: if is_readonly {
            ContextPolicy::ProjectToModel
        } else {
            ContextPolicy::Promote
        },
    }
}

fn looks_like_search(command: &str) -> bool {
    command.starts_with("rg ")
        || command.starts_with("grep ")
        || command.starts_with("find ")
        || command.starts_with("ls")
        || command.starts_with("ls ")
        || command.starts_with("tree")
        || command.starts_with("tree ")
        || command.starts_with("cat ")
        || command.starts_with("head ")
        || command.starts_with("tail ")
        || command.starts_with("wc ")
        || command.starts_with("which ")
        || command.starts_with("whereis ")
}

fn classify_search(command: &str) -> CommandIntent {
    CommandIntent {
        kind: CommandIntentKind::SearchReadOnly,
        confidence: IntentConfidence::High,
        risk: RiskAssessment::safe(),
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
    }
}

fn looks_like_build(command: &str) -> bool {
    command.starts_with("cargo build")
        || command.starts_with("cargo check")
        || command.starts_with("cargo clippy")
        || command.starts_with("cargo fmt")
        || command.starts_with("cargo run")
        || command.starts_with("make")
        || command.starts_with("cmake")
        || command.starts_with("npm run")
        || command.starts_with("pnpm run")
}

fn classify_build(command: &str) -> CommandIntent {
    let kind = if command.starts_with("cargo fmt") || command.starts_with("cargo clippy") {
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
    }
}

fn looks_like_file_read(command: &str) -> bool {
    command.starts_with("cat ")
        || command.starts_with("less ")
        || command.starts_with("more ")
        || command.starts_with("head ")
        || command.starts_with("tail ")
        || command.starts_with("type ")
}

fn classify_file_read(command: &str) -> CommandIntent {
    CommandIntent {
        kind: CommandIntentKind::FileRead,
        confidence: IntentConfidence::High,
        risk: RiskAssessment::safe(),
        source: CommandSource::AgentTool,
        command: command.to_string(),
        context_policy: ContextPolicy::ProjectToModel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn git_status_is_readonly() {
        let intent = classify_command("git status");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }

    #[test]
    fn git_push_is_mutating() {
        let intent = classify_command("git push origin main");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
        assert!(intent.requires_permission());
    }

    #[test]
    fn git_reset_hard_is_high_risk() {
        let intent = classify_command("git reset --hard HEAD~1");
        assert_eq!(intent.risk.level, RiskLevel::High);
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

    #[test]
    fn git_diff_is_readonly() {
        let intent = classify_command("git diff HEAD~1");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
    }
}
