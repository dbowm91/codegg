use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::time::Duration;
use tokio::process::Command;

use crate::command_intent::{classify_command, CommandIntentKind};
use crate::command_outcome::{
    ownership_for_outcome, run_kind_for_outcome, ActualExecutor, ExecutionOutcome,
};
use crate::command_planner::plan_execution;
use crate::command_planner::CommandPlan;
use crate::command_planner::ExecutionBackend;
use crate::command_routing::resolve_routing;
use crate::command_routing::RoutingDecision;
use crate::config::schema::CommandIntentConfig;
use crate::config::schema::CommandIntentFamily;
use crate::config::schema::CommandIntentMode;
use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::python_script::{execute_and_persist_python_script, PythonExecutionMode, PythonScriptRequest};
use crate::security::sandbox::{get_default_allowed_paths, get_sensitive_paths, SandboxConfig};
use crate::test_runner::{resolve_and_run_test, TestRunRequest, TestScope};
use crate::tool::{Tool, ToolCategory};

/// What a single dispatch returned: the result text, raw process output,
/// the actual executor that ran it, and an optional `RunId` proving that
/// a delegated subsystem owns a canonical RunStore record.
///
/// `delegated_run_id == None` paired with `ActualExecutor::TestRunner` or
/// `PythonScript` means the backend executed without obtaining a canonical
/// RunStore record. `BashTool::execute` keeps that result (never retries the
/// command) and uses caller persistence only when a store is available.
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub result: String,
    pub output: std::process::Output,
    pub executor: ActualExecutor,
    pub delegated_run_id: Option<codegg_core::run_store::RunId>,
}

const MAX_COMMAND_LENGTH: usize = 100_000;

/// Routing metadata attached to bash output when command intent routing is enabled.
#[derive(Debug, Clone)]
struct RoutingMetadata {
    intent_kind: String,
    backend_label: String,
    projector_label: String,
    rtk_eligible: bool,
    confidence: String,
    risk_level: String,
    routing_enabled: bool,
    routing_decision: String,
    mode: CommandIntentMode,
}

/// Metrics for routing decisions — recorded per command execution.
#[derive(Debug, Clone)]
struct RoutingMetric {
    family: CommandIntentFamily,
    decision: String,
    fallback: bool,
}

/// Map a `CommandIntentKind` to the corresponding `CommandIntentFamily` for
/// config lookup. Returns `None` for kinds that don't map to a routable family.
fn intent_kind_to_family(kind: CommandIntentKind) -> Option<CommandIntentFamily> {
    match kind {
        CommandIntentKind::Test => Some(CommandIntentFamily::Tests),
        CommandIntentKind::GitReadOnly => Some(CommandIntentFamily::GitRead),
        CommandIntentKind::SearchReadOnly | CommandIntentKind::FileRead => {
            Some(CommandIntentFamily::Search)
        }
        CommandIntentKind::PythonAnalyze
        | CommandIntentKind::PythonTransform
        | CommandIntentKind::PythonVerify => Some(CommandIntentFamily::Python),
        CommandIntentKind::Build => Some(CommandIntentFamily::Build),
        CommandIntentKind::Lint => Some(CommandIntentFamily::Lint),
        CommandIntentKind::Format => Some(CommandIntentFamily::Format),
        _ => None,
    }
}

/// Map an `ExecutionBackend` to a `PlannedBackend` for persistence provenance.
fn plan_to_planned_backend(plan_backend: Option<&ExecutionBackend>) -> codegg_core::run_store::PlannedBackend {
    use codegg_core::run_store::PlannedBackend;
    match plan_backend {
        None => PlannedBackend::Unrouted,
        Some(ExecutionBackend::RawShell { .. }) => PlannedBackend::RawShell,
        Some(ExecutionBackend::TestRunner { .. }) => PlannedBackend::TestRunner,
        Some(ExecutionBackend::PythonScript { .. }) => PlannedBackend::PythonScript,
        Some(ExecutionBackend::NativeTool { .. }) => PlannedBackend::NativeTool,
        Some(ExecutionBackend::ManagedArgv { .. }) => PlannedBackend::ManagedArgv,
        Some(ExecutionBackend::GitMutating { .. }) => PlannedBackend::GitMutating,
        Some(ExecutionBackend::Reject { .. }) => PlannedBackend::Unrouted,
    }
}

/// Map a `RoutingDecision` to a `PlannedBackend` when the planner is not
/// available (legacy/unrouted paths).
#[allow(dead_code)]
fn decision_to_planned_backend(decision: Option<&RoutingDecision>) -> codegg_core::run_store::PlannedBackend {
    use codegg_core::run_store::PlannedBackend;
    match decision {
        None => PlannedBackend::Unrouted,
        Some(RoutingDecision::RouteToTestRunner { .. }) => PlannedBackend::TestRunner,
        Some(RoutingDecision::RouteToPythonScripting { .. }) => PlannedBackend::PythonScript,
        Some(RoutingDecision::RouteToNativeTool { .. }) => PlannedBackend::NativeTool,
        Some(RoutingDecision::RouteToManagedProcess { .. }) => PlannedBackend::ManagedArgv,
        Some(RoutingDecision::RouteToShell { .. }) => PlannedBackend::RawShell,
        Some(RoutingDecision::Rejected { .. }) => PlannedBackend::Unrouted,
    }
}

/// Clone the actual backend from an ExecutionOutcome. Used by the persistence
/// path to set the `actual_backend` field on RunCompletion without consuming
/// the outcome (which is also used for `fallback_record()`).
fn execution_outcome_clone_actual(outcome: &ExecutionOutcome) -> codegg_core::run_store::ActualBackend {
    outcome.actual.into_backend()
}

/// Named command-injection patterns. Each entry is a human-readable name
/// plus a regex. We iterate them individually so we can return which
/// pattern matched (helps users understand why a command was rejected)
/// and so we can fix false positives (e.g. `find -exec`) without
/// weakening security.
static BLOCKED_PATTERNS: &[(&str, &str)] = &[
    ("command substitution $(...)", r"\$\("),
    ("braced command substitution ${...}", r"\$\{"),
    ("backtick substitution", r"`"),
    ("variable expansion $VAR", r"\$[A-Za-z_][A-Za-z0-9_]*"),
    ("pipe to shell |/.*sh", r"\|/.*sh"),
    ("pipe to shell |/.*bash", r"\|/.*bash"),
    ("redirect to /dev", r"> /dev/"),
    ("input redirect from /dev", r"< /dev/"),
    ("stderr redirect to /dev", r"2> /dev/"),
    (
        "fork bomb with rm -rf",
        r"&[\s\n\r]*&[\s\n\r]*rm[\s\n\r]+-rf",
    ),
    ("|| rm -rf", r"\|\|[\s\n\r]*rm[\s\n\r]+-rf"),
    ("printf injection %{...}|&", r"%\{[^}]*\|\s*&"),
    ("eval(", r"eval\s*\("),
    ("eval command", r"(?:^|[\s;&|()<>])eval(?:\s+|\(|$)"),
    ("standalone exec command", r"(?:^|[\s;&|()<>])exec\s+"),
    ("source shell script", r"source\s+.*\.sh"),
    ("dot-source shell script", r"\.\s+.*\.sh"),
    ("base64 -d", r"base64\s+-d"),
    ("xxd -r", r"xxd\s+-r"),
    ("perl -e", r"perl\s+-e"),
    ("python -c", r"python\s+-c"),
    ("ruby -e", r"ruby\s+-e"),
    ("node -e", r"node\s+-e"),
    ("nohup background (trailing &)", r"nohup\s+.*&\s*$"),
    ("nohup with &", r"nohup\s+.*\s+&"),
    ("disown -a", r"disown\s+-a"),
    ("kill -9 -1", r"kill\s+-9\s+-1"),
    ("killall -9", r"killall\s+-9"),
    ("pkill -9", r"pkill\s+-9"),
    ("chmod to /etc", r"chmod\s+[0-7]{4}\s+/etc"),
    ("chmod to /home", r"chmod\s+[0-7]{4}\s+/home"),
    ("chmod to /root", r"chmod\s+[0-7]{4}\s+/root"),
    ("chmod to /var", r"chmod\s+[0-7]{4}\s+/var"),
    ("chmod to /ssh", r"chmod\s+[0-7]{4}\s+/ssh"),
    ("chmod to /proc", r"chmod\s+[0-7]{4}\s+/proc"),
    ("chmod to /sys", r"chmod\s+[0-7]{4}\s+/sys"),
    ("chmod 777 to /", r"chmod\s+777\s+/"),
    ("chown to /etc", r"chown\s+.*\s+/etc"),
    ("chown to /home", r"chown\s+.*\s+/home"),
    ("chown to /root", r"chown\s+.*\s+/root"),
    ("chown to /var", r"chown\s+.*\s+/var"),
    ("chown to /ssh", r"chown\s+.*\s+/ssh"),
    ("chown to /proc", r"chown\s+.*\s+/proc"),
    ("chown to /sys", r"chown\s+.*\s+/sys"),
    ("wget -O /", r"wget\s+.*-O\s+/"),
    ("curl -o /", r"curl\s+.*-o\s+/"),
    ("fork bomb :(){:|:", r":\(\)\s*:\s*\|"),
    ("standalone &", r"(?:^|\s)&(?:[\s]|$)"),
];

static BLOCKED_PATTERN_REGEXES: Lazy<Vec<(&'static str, Regex)>> = Lazy::new(|| {
    BLOCKED_PATTERNS
        .iter()
        .map(|(name, pat)| {
            (
                *name,
                Regex::new(pat).expect("invalid blocked pattern regex"),
            )
        })
        .collect()
});

/// Returns the name of the first matching blocked pattern, or None.
fn find_blocked_pattern(command: &str) -> Option<&'static str> {
    let sanitized = strip_quoted_heredoc_bodies(command);
    for (name, re) in BLOCKED_PATTERN_REGEXES.iter() {
        if re.is_match(&sanitized) {
            return Some(*name);
        }
    }
    None
}

fn strip_quoted_heredoc_bodies(command: &str) -> String {
    let mut output = String::with_capacity(command.len());
    let mut lines = command.lines();

    while let Some(line) = lines.next() {
        output.push_str(line);
        output.push('\n');

        let Some(delimiter) = quoted_heredoc_delimiter(line) else {
            continue;
        };

        for body_line in lines.by_ref() {
            if body_line.trim() == delimiter {
                output.push_str(body_line);
                output.push('\n');
                break;
            }
        }
    }

    if !command.ends_with('\n') {
        output.pop();
    }
    output
}

fn quoted_heredoc_delimiter(line: &str) -> Option<String> {
    let marker = line.find("<<")?;
    let mut rest = line[marker + 2..].trim_start();
    if let Some(stripped) = rest.strip_prefix('-') {
        rest = stripped.trim_start();
    }

    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }

    let end = rest[quote.len_utf8()..].find(quote)?;
    let delimiter = &rest[quote.len_utf8()..quote.len_utf8() + end];
    if delimiter.is_empty() {
        None
    } else {
        Some(delimiter.to_string())
    }
}

/// Derive risk capability flags from a command string for run store records.
/// Returns (has_subprocess, has_git_mutation, has_destructive_mutation).
fn routing_metadata_risk_caps(command: &str) -> (bool, bool, bool) {
    let trimmed = command.trim();
    let has_subprocess = trimmed.contains('|')
        || trimmed.contains('$')
        || trimmed.contains('`')
        || trimmed.starts_with("sudo ");
    let has_git_mutation = trimmed.starts_with("git ")
        && ![
            "git status",
            "git log",
            "git diff",
            "git show",
            "git branch",
            "git remote",
            "git tag",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let has_destructive = trimmed.contains("rm -rf")
        || trimmed.contains("rm -r ")
        || trimmed.contains("git clean -f")
        || trimmed.contains("git reset --hard")
        || trimmed.contains("git checkout --");
    (has_subprocess, has_git_mutation, has_destructive)
}

pub struct BashTool {
    timeout: Duration,
    max_output_lines: usize,
    max_output_bytes: usize,
    blocked_commands: HashSet<&'static str>,
    allowed_paths: Option<Vec<String>>,
    deny_all: bool,
    allowlist: Option<HashSet<&'static str>>,
    landlock_sandbox: Option<SandboxConfig>,
    preflight: Option<Arc<PreflightService>>,
    command_intent_config: Option<CommandIntentConfig>,
    run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_output_lines: 2000,
            max_output_bytes: 50_000,
            blocked_commands: HashSet::from([
                "rm -rf /",
                "rm -rf /*",
                "rm -rf /home",
                "rm -rf /root",
                "rm -rf /var",
                "mkfs",
                "dd if=/dev/zero",
                ":(){:|:&};:",
                "chmod -R 777 /",
                "chown -R",
                "curl -sL | sh",
                "wget -q -O- | sh",
                "bash -c",
                "zcat /dev/urandom",
                "> /dev/sd",
                "fdisk",
                "parted",
                "lsblk",
                "umount /",
                "init 0",
                "shutdown",
                "reboot",
                "systemctl poweroff",
                "telinit 0",
                "poweroff",
                "halt",
                "cat /etc/passwd",
                "cat /etc/shadow",
                "sudo su",
                "sudo -i",
                "sudo bash",
                "su root",
                "pkexec",
            ]),
            allowed_paths: None,
            deny_all: false,
            allowlist: None,
            landlock_sandbox: None,
            preflight: None,
            command_intent_config: None,
            run_store: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_blocked_commands(mut self, commands: Vec<&'static str>) -> Self {
        self.blocked_commands = commands.into_iter().collect();
        self
    }

    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = Some(paths);
        self
    }

    pub fn with_deny_all(mut self) -> Self {
        self.deny_all = true;
        self
    }

    pub fn with_allowlist(mut self, commands: Vec<&'static str>) -> Self {
        self.allowlist = Some(commands.into_iter().collect());
        self
    }

    pub fn with_run_store(mut self, store: Arc<dyn codegg_core::run_store::RunStore>) -> Self {
        self.run_store = Some(store);
        self
    }

    pub fn with_landlock_sandbox(mut self, enabled: bool) -> Self {
        if enabled {
            let mut config = SandboxConfig::new();
            config.enabled = true;
            config.allowed_paths = get_default_allowed_paths();
            config.deny_paths = get_sensitive_paths();
            self.landlock_sandbox = Some(config);
        }
        self
    }

    pub fn with_landlock_sandbox_custom(mut self, config: SandboxConfig) -> Self {
        self.landlock_sandbox = Some(config);
        self
    }

    pub fn with_preflight(mut self, service: PreflightService) -> Self {
        self.preflight = Some(Arc::new(service));
        self
    }

    pub fn with_command_intent_config(mut self, config: CommandIntentConfig) -> Self {
        self.command_intent_config = Some(config);
        self
    }

    pub fn with_sandbox_mode(mut self, mode: crate::security::sandbox::SandboxMode) -> Self {
        if let Some(ref mut config) = self.landlock_sandbox {
            config.mode = mode;
        } else {
            let mut config = SandboxConfig::new();
            config.enabled = true;
            config.mode = mode;
            config.allowed_paths = crate::security::sandbox::get_default_allowed_paths();
            config.deny_paths = crate::security::sandbox::get_sensitive_paths();
            self.landlock_sandbox = Some(config);
        }
        self
    }

    /// Check if active routing is disabled by any kill switch.
    fn check_kill_switches(&self, family: CommandIntentFamily) -> bool {
        // 1. Check env var emergency disable
        if std::env::var("CODEGG_ROUTING_DISABLE").unwrap_or_default() == "1" {
            return true;
        }

        // 2. Check per-family config level
        if let Some(ref cic) = self.command_intent_config {
            if cic.family_level(family) == crate::config::schema::RouteLevel::Off {
                return true;
            }
        }

        false
    }

    /// Record a routing metric for telemetry/debugging.
    fn record_routing_metric(&self, metric: RoutingMetric) {
        tracing::debug!(
            family = ?metric.family,
            decision = %metric.decision,
            fallback = metric.fallback,
            "routing metric"
        );
    }

    /// Execute a command via raw shell (`sh -c`). This is the original behavior
    /// used by observe mode and as a fallback when active routing is disabled
    /// or dispatch fails.
    async fn execute_via_raw_shell(
        &self,
        command: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<(String, std::process::Output), ToolError> {
        let output = tokio::time::timeout(timeout, async {
            let mut cmd = Command::new("sh");
            cmd.env_clear();
            let preserve_vars = [
                "PATH",
                "HOME",
                "USER",
                "SHELL",
                "LANG",
                "LC_ALL",
                "TERM",
                "CARGO_HOME",
                "RUSTUP_HOME",
                "CARGO_INCREMENTAL",
                "CARGO_TERM_COLOR",
                "CARGO_TERM_PROGRESS",
                "RUSTFLAGS",
                "RUSTDOCFLAGS",
                "CARGO_PROFILE_*",
                "npm_config_*",
                "NVM_DIR",
                "PYENV_ROOT",
                "VIRTUAL_ENV",
                "PYTHONPATH",
                "JAVA_HOME",
                "GOPATH",
                "GOBIN",
            ];
            for var in &preserve_vars {
                if let Some(prefix) = var.strip_suffix('*') {
                    for (key, value) in std::env::vars() {
                        if key.starts_with(prefix) {
                            cmd.env(&key, &value);
                        }
                    }
                } else if let Some(value) = std::env::var_os(var) {
                    cmd.env(var, &value);
                }
            }
            cmd.arg("-c").arg(command);
            if let Some(dir) = canonical_workdir {
                cmd.current_dir(dir);
            }
            cmd.kill_on_drop(true);
            cmd.output().await
        })
        .await
        .map_err(|_| ToolError::Timeout(command.to_string()))?
        .map_err(|e| ToolError::Execution(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&truncate_output(
                &stdout,
                self.max_output_lines,
                self.max_output_bytes,
            ));
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n--- stderr ---\n");
            }
            result.push_str(&truncate_output(
                &stderr,
                self.max_output_lines,
                self.max_output_bytes,
            ));
        }
        result.push_str(&format!(
            "\n\n[exit code: {}]",
            output.status.code().unwrap_or(-1)
        ));

        Ok((result, output))
    }

/// Dispatch to the canonical TestRunner backend.
///
/// Routes to `resolve_and_run_test` directly (no shell reconstruction),
/// preserving cwd, timeout, scope, and RunStore ownership. Returns the
/// canonical `DelegatedTestRun` with an optional `RunId` proving the
/// delegated record was begun.
async fn dispatch_to_test_runner(
    &self,
    argv: &[String],
    cwd: Option<&Path>,
    _validated_command: Option<&str>,
    timeout: Duration,
) -> Result<DispatchOutcome, ToolError> {
    self.record_routing_metric(RoutingMetric {
        family: CommandIntentFamily::Tests,
        decision: "test_runner_dispatch".to_string(),
        fallback: false,
    });

    let run_cwd = cwd
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let timeout_secs = timeout.as_secs();
    let stall_timeout_secs = (timeout_secs / 2).max(15);

    // Build a TestRunRequest from the planner-validated argv. We use
    // `TestScope::BashDispatch` so the argv is consumed directly without
    // the strict allowlist re-validation in `TestScope::CustomCommand`
    // (which rejects any non-allowlisted test command prefix).
    let scope = TestScope::BashDispatch(argv.to_vec());

    let request = TestRunRequest {
        scope,
        workdir: run_cwd.clone(),
        timeout_secs: Some(timeout_secs.max(1)),
        stall_timeout_secs: Some(stall_timeout_secs),
        max_report_bytes: None,
        session_id: None,
    };

    let run_store = self.run_store.as_ref();
    let delegated = resolve_and_run_test(
        request,
        Some(&crate::test_runner::BusEventSink),
        run_store,
    )
    .await
    .map_err(|e| ToolError::Execution(format!("test runner error: {e}")))?;

    let report = delegated.report().clone();
    let run_id = delegated.run_id.clone();

    // Project the structured report for model-facing display.
    let result = crate::test_runner::format_test_report_with_cap(
        &report,
        crate::test_runner::report::DEFAULT_MAX_REPORT_BYTES,
    );

    // Synthesize a `std::process::Output`-shaped value for code paths
    // that still inspect it (truncation, exit status, persistence).
    let exit_code = report.exit_code.unwrap_or(match report.status {
        crate::test_runner::types::TestStatus::Passed => 0,
        _ => 1,
    });
    let stdout_bytes = result.as_bytes().to_vec();
    let stderr_bytes: Vec<u8> = Vec::new();
    let output = synth_output(exit_code, stdout_bytes, stderr_bytes);

    let actual = ActualExecutor::TestRunner {
        argv: argv.to_vec(),
        cwd: run_cwd,
    };

    Ok(DispatchOutcome {
        result,
        output,
        executor: actual,
        delegated_run_id: run_id,
    })
}

/// Dispatch to native tool (e.g. egggit). Executes via direct `Command::new`
/// instead of `sh -c`, bypassing shell interpretation.
async fn dispatch_to_native_tool(
    &self,
    command: &str,
    canonical_workdir: Option<&Path>,
    timeout: Duration,
) -> Result<DispatchOutcome, ToolError> {
    let argv: Vec<&str> = command.split_whitespace().collect();
    if argv.is_empty() {
        let (result, output) = self
            .execute_via_raw_shell(command, canonical_workdir, timeout)
            .await?;
        return Ok(DispatchOutcome {
            result,
            output,
            executor: ActualExecutor::RawShell {
                command: command.to_string(),
                argv: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    command.to_string(),
                ],
            },
            delegated_run_id: None,
        });
    }

    let argv_owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let tool_name = argv_owned[0].clone();
    let output = tokio::time::timeout(timeout, async {
        let mut cmd = Command::new(argv[0]);
        cmd.args(&argv[1..]);
        if let Some(dir) = canonical_workdir {
            cmd.current_dir(dir);
        }
        cmd.kill_on_drop(true);
        cmd.output().await
    })
    .await
    .map_err(|_| ToolError::Timeout(command.to_string()))?
    .map_err(|e| ToolError::Execution(e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut result = stdout;
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n--- stderr ---\n");
        }
        result.push_str(&stderr);
    }
    result.push_str(&format!(
        "\n\n[exit code: {}]",
        output.status.code().unwrap_or(-1)
    ));

    self.record_routing_metric(RoutingMetric {
        family: CommandIntentFamily::GitRead,
        decision: "native_tool_dispatch".to_string(),
        fallback: false,
    });

    Ok(DispatchOutcome {
        result,
        output,
        executor: ActualExecutor::NativeTool {
            tool_name,
            argv: argv_owned,
        },
        delegated_run_id: None,
    })
}

/// Dispatch to canonical Python subsystem (`execute_and_persist_python_script`).
/// Replaces direct `python3 -c` invocation so that policy resolution, sandbox,
/// snapshots, and RunStore persistence all run through the canonical path.
async fn dispatch_to_python_script(
    &self,
    script: &str,
    mode: &str,
    canonical_workdir: Option<&Path>,
    timeout: Duration,
) -> Result<DispatchOutcome, ToolError> {
    let exec_mode = match mode {
        "analyze" => PythonExecutionMode::Analyze,
        "transform" => PythonExecutionMode::Transform,
        "verify" => PythonExecutionMode::Verify,
        _ => PythonExecutionMode::Analyze,
    };

    let cwd = canonical_workdir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let request = PythonScriptRequest {
        code: script.to_string(),
        mode: exec_mode,
        cwd,
        workspace_root: None,
        timeout_secs: Some(timeout.as_secs()),
        session_id: None,
        intent: Some(format!("command-intent:{mode}")),
    };

    self.record_routing_metric(RoutingMetric {
        family: CommandIntentFamily::Python,
        decision: "python_script_dispatch".to_string(),
        fallback: false,
    });

    let delegated = execute_and_persist_python_script(&request, self.run_store.as_ref()).await;
    let result = delegated.result.clone();
    let run_id = delegated.run_id.clone();

    let stdout = result.stdout.clone();
    let stderr = result.stderr.clone();
    let mut display = stdout;
    if !stderr.is_empty() {
        if !display.is_empty() {
            display.push_str("\n--- stderr ---\n");
        }
        display.push_str(&stderr);
    }
    let exit_code = result.exit_code();
    display.push_str(&format!("\n\n[exit code: {}]", exit_code.unwrap_or(-1)));

    let exit_code_value = exit_code.unwrap_or(-1);
    let output = synth_output(
        exit_code_value,
        result.stdout.as_bytes().to_vec(),
        result.stderr.as_bytes().to_vec(),
    );

    let actual = ActualExecutor::PythonScript {
        script_hash: result.script_body_hash.clone(),
        mode: mode.to_string(),
    };

    Ok(DispatchOutcome {
        result: display,
        output,
        executor: actual,
        delegated_run_id: run_id,
    })
}

/// Dispatch to managed process via direct `Command::new`. Falls back to
/// raw shell on dispatch failure.
async fn dispatch_to_managed_process(
    &self,
    argv: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<DispatchOutcome, ToolError> {
    if argv.is_empty() {
        return Err(ToolError::Execution("empty argv".to_string()));
    }

    let argv_owned = argv.to_vec();
    let cwd_owned = cwd.map(|p| p.to_path_buf());

    let output = tokio::time::timeout(timeout, async {
        let mut cmd = Command::new(&argv_owned[0]);
        cmd.args(&argv_owned[1..]);
        if let Some(dir) = &cwd_owned {
            cmd.current_dir(dir);
        }
        cmd.kill_on_drop(true);
        cmd.output().await
    })
    .await
    .map_err(|_| ToolError::Timeout(argv.join(" ")))?
    .map_err(|e| ToolError::Execution(e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut result = stdout;
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n--- stderr ---\n");
        }
        result.push_str(&stderr);
    }
    result.push_str(&format!(
        "\n\n[exit code: {}]",
        output.status.code().unwrap_or(-1)
    ));

    self.record_routing_metric(RoutingMetric {
        family: CommandIntentFamily::Search,
        decision: "managed_process_dispatch".to_string(),
        fallback: false,
    });

    Ok(DispatchOutcome {
        result,
        output,
        executor: ActualExecutor::ManagedArgv {
            argv: argv_owned,
            cwd: cwd_owned,
        },
        delegated_run_id: None,
    })
}

/// Dispatch to shell backend (used for RouteToShell decisions).
/// Executes via raw shell since this IS the shell path.
async fn dispatch_to_shell(
    &self,
    command: &str,
    canonical_workdir: Option<&Path>,
    timeout: Duration,
) -> Result<DispatchOutcome, ToolError> {
    self.record_routing_metric(RoutingMetric {
        family: CommandIntentFamily::Tests, // generic
        decision: "shell_dispatch".to_string(),
        fallback: false,
    });
    let argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        command.to_string(),
    ];
    let (result, output) = self
        .execute_via_raw_shell(command, canonical_workdir, timeout)
        .await?;
    Ok(DispatchOutcome {
        result,
        output,
        executor: ActualExecutor::RawShell {
            command: command.to_string(),
            argv,
        },
        delegated_run_id: None,
    })
}

/// Dispatch a routing decision to the appropriate backend.
async fn dispatch_routing_decision(
    &self,
    decision: &RoutingDecision,
    _plan: &CommandPlan,
    canonical_workdir: Option<&Path>,
    timeout: Duration,
) -> Result<DispatchOutcome, ToolError> {
    match decision {
        RoutingDecision::RouteToTestRunner {
            argv,
            validated_command,
            ..
        } => {
            self.dispatch_to_test_runner(
                argv,
                canonical_workdir,
                validated_command.as_deref(),
                timeout,
            )
            .await
        }
        RoutingDecision::RouteToNativeTool { command, .. } => {
            self.dispatch_to_native_tool(command, canonical_workdir, timeout)
                .await
        }
        RoutingDecision::RouteToPythonScripting {
            script,
            mode,
            ..
        } => {
            let mode_str = match mode {
                crate::command_planner::PythonModeGuess::Analyze => "analyze",
                crate::command_planner::PythonModeGuess::Transform => "transform",
                crate::command_planner::PythonModeGuess::Verify => "verify",
                crate::command_planner::PythonModeGuess::Unknown => "analyze",
            };
            self.dispatch_to_python_script(script, mode_str, canonical_workdir, timeout)
                .await
        }
        RoutingDecision::RouteToManagedProcess {
            argv,
            cwd,
            ..
        } => {
            self.dispatch_to_managed_process(argv, Some(cwd), timeout)
                .await
        }
        RoutingDecision::RouteToShell { command, .. } => {
            self.dispatch_to_shell(command, canonical_workdir, timeout)
                .await
        }
        RoutingDecision::Rejected { reason } => Err(ToolError::Execution(format!(
            "command rejected: {}",
            reason
        ))),
    }
}

    fn check_command_security(&self, command: &str, parts: &[&str]) -> Result<(), ToolError> {
        if parts.is_empty() {
            return Ok(());
        }

        let normalized = parts.join(" ");

        // Check blocked commands first (entire command string)
        let blocked = &self.blocked_commands;
        if !blocked.is_empty() {
            for blocked_cmd in blocked {
                if normalized.starts_with(blocked_cmd) {
                    return Err(ToolError::Permission(format!(
                        "command matches blocked list: {}",
                        blocked_cmd
                    )));
                }
            }
        }

        // Check allowlist - must check entire command string
        if let Some(ref allowlist) = self.allowlist {
            let mut cmd_parts = parts.iter().copied();
            let mut cmd = cmd_parts.next().unwrap_or("");

            while ["env", "nohup", "time", "nice", "setuid", "sudo"].contains(&cmd) {
                cmd = cmd_parts.next().unwrap_or("");
            }

            if (cmd == "bash" || cmd == "sh" || cmd == "dash")
                && parts.len() > 2
                && parts[1] == "-c"
            {
                if !allowlist.contains(&cmd) {
                    return Err(ToolError::Permission(format!(
                        "command '{}' not in allowlist",
                        cmd
                    )));
                }

                let full_match = allowlist
                    .iter()
                    .any(|allowed| normalized.starts_with(allowed));
                if !full_match {
                    return Err(ToolError::Permission(format!(
                        "command '{}' not in allowlist",
                        normalized
                    )));
                }
                return Ok(());
            }

            if !allowlist.contains(&cmd) {
                let full_match = allowlist
                    .iter()
                    .any(|allowed| normalized.starts_with(allowed));
                if !full_match {
                    return Err(ToolError::Permission(format!(
                        "command '{}' not in allowlist",
                        normalized
                    )));
                }
            }
        }

        // Check blocked patterns (command injection)
        if let Some(pat) = find_blocked_pattern(command) {
            return Err(ToolError::Permission(format!(
                "command matches blocked pattern: {} (in: {:.80})",
                pat, command
            )));
        }

        Ok(())
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Synthesize a `std::process::Output` with the given exit code, stdout,
/// and stderr bytes. Used by dispatchers that produce structured results
/// but need to fit the legacy `Output` shape.
fn synth_output(exit_code: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> std::process::Output {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(exit_code),
            stdout,
            stderr,
        }
    }
    #[cfg(not(unix))]
    {
        // The bash tool is Unix-only; this branch is unreachable but
        // keeps the type signature portable.
        std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout,
            stderr,
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output. For web search and URL fetching, prefer the `websearch` and `webfetch` tools — they handle rate limits, SSRF protection, and bot detection. `curl`/`wget` to arbitrary URLs is permitted but discouraged; use them only when a tool is genuinely unsuitable."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for command execution"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'command' parameter".to_string()))?;

        if self.deny_all {
            return Err(ToolError::Permission("bash tool is disabled".to_string()));
        }

        if command.len() > MAX_COMMAND_LENGTH {
            return Err(ToolError::Execution(format!(
                "command exceeds maximum length of {} bytes",
                MAX_COMMAND_LENGTH
            )));
        }

        let parts: Vec<&str> = command.split_whitespace().collect();
        self.check_command_security(command, &parts)?;

        let preflight_warning = if let Some(ref svc) = self.preflight {
            match svc.check_command(command).await {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked command: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                PreflightDecision::Warn { findings } => {
                    let warning = PreflightDecision::Warn { findings }.summary();
                    tracing::warn!(target: "preflight", "{}", warning);
                    Some(warning)
                }
                _ => None,
            }
        } else {
            None
        };

        let timeout_secs = input["timeout"].as_u64().unwrap_or(120);
        let execution_timeout = Duration::from_secs(timeout_secs.max(1));

        let workdir = input["workdir"].as_str().map(|s| s.to_string());

        // Phase 04/10: Classify command intent, plan execution, and resolve routing.
        // When active routing is enabled for the command's family AND the plan
        // passes validation AND no kill switch is active, dispatch to the
        // structured backend. Otherwise, execute via raw shell (observe mode).
        let (intent, plan, decision, routing_metadata) =
            if let Some(ref cic) = self.command_intent_config {
                let mode = cic.mode();
                let intent = classify_command(command);
                let plan = plan_execution(&intent);
                let decision = resolve_routing(&plan);

                let family_enabled = intent_kind_to_family(intent.kind)
                    .map(|f| cic.is_enabled(f))
                    .unwrap_or(false);

                let metadata = RoutingMetadata {
                    intent_kind: intent.kind.label().to_string(),
                    backend_label: plan.backend.label().to_string(),
                    projector_label: plan.projector.label().to_string(),
                    rtk_eligible: plan.rtk_policy.is_rtk_eligible(),
                    confidence: format!("{:?}", intent.confidence).to_lowercase(),
                    risk_level: format!("{:?}", intent.risk.level).to_lowercase(),
                    routing_enabled: family_enabled,
                    routing_decision: format!("{:?}", decision),
                    mode,
                };

                (Some(intent), Some(plan), Some(decision), Some(metadata))
            } else {
                (None, None, None, None)
            };

        // Resolve canonical working directory
        let mut canonical_workdir: Option<PathBuf> = None;

        if let Some(ref paths) = self.allowed_paths {
            if let Some(ref dir) = workdir {
                let workdir = dir.clone();
                let paths = paths.clone();
                let (allowed, canonical) = tokio::task::spawn_blocking(move || {
                    let canonical_dir = std::fs::canonicalize(&workdir).map_err(|_| {
                        ToolError::Permission(format!(
                            "working directory '{workdir}' could not be resolved"
                        ))
                    })?;

                    let mut allowed = false;
                    for path in &paths {
                        let canonical_path = std::fs::canonicalize(path).map_err(|_| {
                            ToolError::Permission(format!(
                                "allowed path '{path}' could not be resolved"
                            ))
                        })?;
                        if canonical_dir.starts_with(&canonical_path) {
                            allowed = true;
                            break;
                        }
                    }
                    Ok::<_, ToolError>((allowed, canonical_dir))
                })
                .await
                .map_err(|e| ToolError::Execution(format!("spawn_blocking failed: {}", e)))??;

                if allowed {
                    canonical_workdir = Some(canonical);
                } else {
                    return Err(ToolError::Permission(format!(
                        "working directory '{dir}' is not in allowed paths"
                    )));
                }
            } else if !paths.is_empty() {
                return Err(ToolError::Permission(
                    "workdir must be specified when allowed_paths is set".to_string(),
                ));
            }
        }

        if let Some(ref sandbox_config) = self.landlock_sandbox {
            if sandbox_config.enabled {
                let sandbox_config = sandbox_config.clone();
                tokio::task::spawn_blocking(move || -> Result<(), ToolError> {
                    sandbox_config.enforce()?;
                    Ok(())
                })
                .await
                .map_err(|e| ToolError::Execution(format!("sandbox enforce failed: {}", e)))??;
            }
        }

        tracing::info!("Running: {command}");
        let start = std::time::Instant::now();

        // Decide: active routing or raw shell
        let should_active_route = if let (Some(ref cic), Some(ref intent), Some(ref plan)) =
            (&self.command_intent_config, &intent, &plan)
        {
            let family = intent_kind_to_family(intent.kind);
            let active_for_family = family.map(|f| cic.is_active_for_family(f)).unwrap_or(false);
            let plan_valid = plan.validate_for_active_routing().is_ok();
            let kill_switch_active = family.map(|f| self.check_kill_switches(f)).unwrap_or(true);
            active_for_family && plan_valid && !kill_switch_active
        } else {
            false
        };

        let (
            mut result,
            output,
            execution_outcome,
            delegated_run_id,
        ) = if should_active_route {
            // ACTIVE ROUTING: dispatch to structured backend
            tracing::info!("Active routing dispatch for: {command}");
            let decision_ref = decision.as_ref().unwrap();
            let plan_ref = plan.as_ref().unwrap();
            let planned_backend = plan_to_planned_backend(Some(&plan_ref.backend));

            match self
                .dispatch_routing_decision(
                    decision_ref,
                    plan_ref,
                    canonical_workdir.as_deref(),
                    execution_timeout,
                )
                .await
            {
                Ok(outcome) => {
                    // A missing RunId means persistence was unavailable, not
                    // that execution failed. The delegated backend has already
                    // run the command; retrying through the shell here would
                    // execute tests/scripts twice and could repeat mutations.
                    let is_delegated_intent = matches!(
                        &outcome.executor,
                        ActualExecutor::TestRunner { .. } | ActualExecutor::PythonScript { .. }
                    );
                    if is_delegated_intent && outcome.delegated_run_id.is_none() {
                        tracing::warn!(
                            "Delegated dispatcher for {:?} returned no RunId; \
                             retaining the delegated result and using caller persistence when available",
                            outcome.executor
                        );
                        if let Some(ref intent) = intent {
                            self.record_routing_metric(RoutingMetric {
                                family: intent_kind_to_family(intent.kind)
                                    .unwrap_or(CommandIntentFamily::Tests),
                                decision: "active_routing_delegation_without_runid".to_string(),
                                fallback: self.run_store.is_some(),
                            });
                        }
                    }
                    let exec_outcome =
                        ExecutionOutcome::identity(planned_backend, outcome.executor.clone());
                    (outcome.result, outcome.output, exec_outcome, outcome.delegated_run_id)
                }
                Err(e) => {
                    // Fallback to raw shell on dispatch failure.
                    // Workstream A: record a FallbackRecord so persistence
                    // can show actual_backend=RawShell + planned_backend=X.
                    if let Some(ref intent) = intent {
                        self.record_routing_metric(RoutingMetric {
                            family: intent_kind_to_family(intent.kind)
                                .unwrap_or(CommandIntentFamily::Tests),
                            decision: "active_routing_fallback".to_string(),
                            fallback: true,
                        });
                    }
                    tracing::warn!(
                        "Active routing dispatch failed, falling back to raw shell: {}",
                        e
                    );
                    let (result, output) = self
                        .execute_via_raw_shell(
                            command,
                            canonical_workdir.as_deref(),
                            execution_timeout,
                        )
                        .await?;
                    let argv = vec![
                        "sh".to_string(),
                        "-c".to_string(),
                        command.to_string(),
                    ];
                    let actual = ActualExecutor::RawShell {
                        command: command.to_string(),
                        argv: argv.clone(),
                    };
                    let outcome = ExecutionOutcome::with_fallback(
                        planned_backend,
                        actual,
                        format!("dispatch failed: {e}"),
                    );
                    (result, output, outcome, None)
                }
            }
        } else {
            // OBSERVE MODE: run via raw shell (existing behavior)
            let argv = vec![
                "sh".to_string(),
                "-c".to_string(),
                command.to_string(),
            ];
            let (result, output) = self
                .execute_via_raw_shell(
                    command,
                    canonical_workdir.as_deref(),
                    execution_timeout,
                )
                .await?;
            let planned = plan_to_planned_backend(
                plan.as_ref().map(|p| &p.backend),
            );
            let actual = ActualExecutor::RawShell {
                command: command.to_string(),
                argv,
            };
            (
                result,
                output,
                ExecutionOutcome::identity(planned, actual),
                None,
            )
        };

        let elapsed = start.elapsed();
        tracing::info!("Completed in {elapsed:?}");

        // Determine RunKind and ownership from the ACTUAL execution outcome (not
        // the planned decision). Workstream B: persistence provenance must
        // reflect what ran, not what was planned.
        let delegated_executor = matches!(
            &execution_outcome.actual,
            ActualExecutor::TestRunner { .. } | ActualExecutor::PythonScript { .. }
        );
        let ownership = ownership_for_outcome(&execution_outcome);

        // Workstream E: persistence suppression requires PROOF of delegated
        // ownership — a delegated run_id. Without a run_id, the delegated
        // record may not exist (or begin_run failed) and we MUST persist
        // the caller-owned fallback record. This is the central invariant:
        // one logical execution always produces exactly one canonical record.
        let persist_run = match (ownership, delegated_run_id.as_ref()) {
            (codegg_core::run_store::RunOwnership::DelegatedBackend, Some(_)) => false,
            (codegg_core::run_store::RunOwnership::DelegatedBackend, None) => {
                // The backend already executed. If a shared store exists,
                // persist the result once from the caller; without a store
                // there is nowhere for BashTool to persist it.
                tracing::warn!(
                    "delegated ownership without run_id — using caller persistence when available"
                );
                self.run_store.is_some()
            }
            _ => true,
        };
        // Ownership is Copy (no, it's an enum without Copy). Recompute
        // when used below in the persistence draft — the value was
        // consumed by the match above. We rely on the fact that
        // ownership_for_outcome is a pure function.
        let ownership = if delegated_executor && delegated_run_id.is_none() {
            codegg_core::run_store::RunOwnership::Caller
        } else {
            ownership_for_outcome(&execution_outcome)
        };

        // Persist to run store if available and this is not a delegated backend.
        if persist_run {
            if let Some(ref store) = self.run_store {
                use chrono::Utc;
                use codegg_core::run_store::*;

                let cwd = canonical_workdir
                    .clone()
                    .or_else(|| workdir.as_ref().map(PathBuf::from))
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let workspace_root = cwd.clone();

                // Extract risk info from routing metadata if available
                let (risk_level, has_subprocess, has_git_mutation, has_destructive) =
                    if let Some(ref rm) = routing_metadata {
                        let caps = routing_metadata_risk_caps(command);
                        (rm.risk_level.clone(), caps.0, caps.1, caps.2)
                    } else {
                        ("low".to_string(), true, false, false)
                    };

                // Workstream B: RunKind is derived from the actual executor
                // and the intent, NOT the planned routing decision.
                let intent_kind = intent
                    .as_ref()
                    .map(|i| i.kind)
                    .unwrap_or(crate::command_intent::CommandIntentKind::RawShell);
                let run_kind_str = run_kind_for_outcome(&execution_outcome, intent_kind);
                let run_kind = match run_kind_str.as_str() {
                    "raw_shell" => RunKind::RawShell,
                    "managed_process" => RunKind::ManagedProcess,
                    "test" => RunKind::Test,
                    "git_read" => RunKind::GitRead,
                    "git_mutation" => RunKind::GitMutation,
                    "search" => RunKind::Search,
                    "python" => RunKind::Python,
                    "native_tool" => RunKind::NativeTool,
                    _ => RunKind::RawShell,
                };

                // Workstream E: argv reflects the ACTUAL execution, not
                // the planned decision. For raw shell: `[sh, -c, command]`.
                // For managed argv / native tool / managed process: the
                // actual argv that was used.
                let invocation_argv: Vec<String> = match &execution_outcome.actual {
                    ActualExecutor::RawShell { argv, .. } => argv.clone(),
                    ActualExecutor::ManagedArgv { argv, .. } => argv.clone(),
                    ActualExecutor::NativeTool { argv, .. } => argv.clone(),
                    ActualExecutor::TestRunner { argv, .. } => argv.clone(),
                    ActualExecutor::PythonScript { .. } => {
                        vec!["python3".to_string(), "<script>".to_string()]
                    }
                    ActualExecutor::Rejected { .. } => vec![],
                };
                let script_hash = match &execution_outcome.actual {
                    ActualExecutor::PythonScript { script_hash, .. } => script_hash.clone(),
                    _ => None,
                };

                // Workstream F: backend family/detail reflect the actual executor.
                let (backend_family, backend_detail) = match &execution_outcome.actual {
                    ActualExecutor::RawShell { .. } => (
                        "bash".to_string(),
                        Some("raw_shell".to_string()),
                    ),
                    ActualExecutor::ManagedArgv { .. } => (
                        "bash".to_string(),
                        Some("managed_argv".to_string()),
                    ),
                    ActualExecutor::NativeTool { tool_name, .. } => (
                        "native_tool".to_string(),
                        Some(tool_name.clone()),
                    ),
                    ActualExecutor::TestRunner { .. } => (
                        "test_runner".to_string(),
                        routing_metadata.as_ref().map(|m| m.intent_kind.clone()),
                    ),
                    ActualExecutor::PythonScript { mode, .. } => (
                        "python_script".to_string(),
                        Some(mode.clone()),
                    ),
                    ActualExecutor::Rejected { reason } => (
                        "bash".to_string(),
                        Some(format!("rejected:{reason}")),
                    ),
                };

                let draft = RunDraft {
                    kind: run_kind,
                    invocation: RunInvocation {
                        command: command.to_string(),
                        argv: Some(invocation_argv),
                        script_hash,
                    },
                    session_id: None,
                    parent_run_id: None,
                    workspace_root,
                    cwd,
                    backend: BackendRecord {
                        family: backend_family,
                        detail: backend_detail,
                    },
                    risk: RiskRecord {
                        level: risk_level,
                        has_subprocess,
                        has_git_mutation,
                        has_destructive_mutation: has_destructive,
                    },
                    planned_backend: Some(execution_outcome.planned.clone()),
                    actual_backend: Some(execution_outcome.actual.into_backend()),
                    ownership,
                };

                let exit_code = output.status.code().unwrap_or(-1);
                let status = if exit_code == 0 {
                    RunStatus::Complete
                } else {
                    RunStatus::Failed
                };

                // Workstream H: persistence failure (begin_run error) must
                // not change the actual execution outcome or result string.
                // The `if let Ok(handle)` already swallows persistence errors.
                if let Ok(handle) = store.begin_run(draft).await {
                    if !output.stdout.is_empty() {
                        let _ = store
                            .write_artifact(
                                &handle,
                                ArtifactInput {
                                    kind: ArtifactKind::Stdout,
                                    data: output.stdout.clone(),
                                    mime_type: "text/plain".to_string(),
                                    // Workstream G: raw stdout is NOT model-safe.
                                    safe_for_model: false,
                                },
                            )
                            .await;
                    }

                    if !output.stderr.is_empty() {
                        let _ = store
                            .write_artifact(
                                &handle,
                                ArtifactInput {
                                    kind: ArtifactKind::Stderr,
                                    data: output.stderr.clone(),
                                    mime_type: "text/plain".to_string(),
                                    // Workstream G: raw stderr is NOT model-safe.
                                    safe_for_model: false,
                                },
                            )
                            .await;
                    }

                    let _ = store
                        .complete_run(
                            handle,
                            RunCompletion {
                                status,
                                completed_at: Utc::now(),
                                permissions: vec![],
                                sandbox: None,
                                projection: None,
                                changes: vec![],
                                rerun: None,
                                actual_backend: Some(
                                    execution_outcome_clone_actual(&execution_outcome),
                                ),
                                fallback: execution_outcome.fallback_record(),
                            },
                        )
                        .await;
                }
            }
        }

        if let Some(warning) = preflight_warning {
            result = format!("{}\n\n{}", warning, result);
        }

        if let Some(meta) = routing_metadata {
            result = format!(
                "{}\n\n[intent: {} | backend: {} | projector: {} | confidence: {} | risk: {} | routing: {} | rtk: {} | route: {} | mode: {}]",
                result,
                meta.intent_kind,
                meta.backend_label,
                meta.projector_label,
                meta.confidence,
                meta.risk_level,
                if meta.routing_enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                if meta.rtk_eligible { "eligible" } else { "off" },
                meta.routing_decision,
                match meta.mode {
                    CommandIntentMode::Observe => "observe",
                    CommandIntentMode::Active => "active",
                    CommandIntentMode::Route => "route (fallback: observe)",
                },
            );
        }

        Ok(result)
    }
}

fn truncate_output(output: &str, max_lines: usize, max_bytes: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let truncated = if lines.len() > max_lines {
        let head = &lines[..max_lines / 2];
        let tail = &lines[lines.len() - max_lines / 2..];
        let mut result = head.join("\n");
        result.push_str(&format!(
            "\n\n... [{} lines truncated] ...\n\n",
            lines.len() - max_lines
        ));
        result.push_str(&tail.join("\n"));
        result
    } else {
        output.to_string()
    };

    if truncated.len() > max_bytes {
        let truncate_at = truncated
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max_bytes)
            .last()
            .unwrap_or(0);
        format!("{}... [output truncated]", &truncated[..truncate_at])
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_allowed(command: &str) {
        assert!(
            find_blocked_pattern(command).is_none(),
            "expected allowed but matched: {} (cmd={})",
            find_blocked_pattern(command).unwrap_or("?"),
            command
        );
    }

    fn assert_blocked(command: &str, expected_name_contains: &str) {
        let pat = find_blocked_pattern(command);
        assert!(pat.is_some(), "expected blocked but allowed: {}", command);
        let pat = pat.unwrap();
        assert!(
            pat.contains(expected_name_contains),
            "expected pattern containing '{}', got '{}' for cmd: {}",
            expected_name_contains,
            pat,
            command
        );
    }

    #[test]
    fn find_exec_is_allowed() {
        // `find -exec` is a benign find flag, not the shell `exec` builtin.
        assert_allowed("find . -name '*.rs' -exec grep -l 'fn ' {} +");
        assert_allowed("find /tmp -name '*.log' -exec rm {} \\;");
    }

    #[test]
    fn find_plain_is_allowed() {
        assert_allowed("find . -name '*.rs'");
        assert_allowed("find . -type f -name 'foo*'");
    }

    #[test]
    fn xargs_is_allowed() {
        assert_allowed("find . -name '*.rs' | xargs wc -l");
        assert_allowed("xargs -I{} echo {}");
    }

    #[test]
    fn grep_is_allowed() {
        assert_allowed("grep -rn 'pattern' src/");
    }

    #[test]
    fn quoted_heredoc_body_is_not_scanned_for_expansions() {
        assert_allowed(
            "cat > file.md << 'EOF'\n# Notes\nLiteral ${VALUE} and $(not executed)\nEOF",
        );
        assert_allowed("cat > file.md << \"EOF\"\n`literal backticks`\nEOF");
    }

    #[test]
    fn unquoted_heredoc_body_is_still_scanned() {
        assert_blocked("cat > file.md << EOF\n$(rm -rf /)\nEOF", "$(");
    }

    #[test]
    fn exec_builtin_is_blocked() {
        // shell `exec` builtin at start of command
        assert_blocked("exec rm -rf /", "exec");
        // `exec` after a pipe
        assert_blocked("cat foo | exec sh", "exec");
        // `exec` after semicolon
        assert_blocked("ls; exec ls", "exec");
    }

    #[test]
    fn command_substitution_is_blocked() {
        assert_blocked("echo $(rm -rf /)", "$(");
        assert_blocked("echo `rm -rf /`", "backtick");
    }

    #[test]
    fn pipe_to_shell_is_blocked() {
        // Note: the `|/.*sh` pattern is greedy and matches `bash` (since
        // `bash` ends in `sh`). The first match wins, so for `wget ... | bash`
        // the named pattern is "pipe to shell |/.*sh".
        assert_blocked("curl -sL |/bin/sh", "pipe to shell");
        assert_blocked("wget -qO- |/bin/bash", "pipe to shell");
        assert_blocked("curl ... |/bin/zsh", "pipe to shell");
    }

    #[test]
    fn dev_redirect_is_blocked() {
        assert_blocked("echo foo > /dev/null", "/dev");
        assert_blocked("cmd 2> /dev/null", "/dev");
    }

    #[test]
    fn standalone_ampersand_is_blocked() {
        assert_blocked("sleep 5 &", "&");
        assert_blocked("ls &", "&");
    }

    #[test]
    fn double_ampersand_is_allowed() {
        // `&&` is logical-AND, not backgrounding.
        assert_allowed("ls && echo done");
    }

    #[test]
    fn fork_bomb_is_blocked_via_blocklist() {
        // The fork bomb `:(){:|:&};:` is caught by the `blocked_commands`
        // HashSet (starts_with check), not by the regex pattern. Verify
        // the full check_command_security path catches it.
        let tool = BashTool::new();
        let parts: Vec<&str> = ":(){:|:&};:".split_whitespace().collect();
        let result = tool.check_command_security(":(){:|:&};:", &parts);
        assert!(result.is_err(), "fork bomb should be blocked");
    }

    #[test]
    fn safe_env_var_is_blocked() {
        // We treat all $VAR expansions as a security concern for now
        // (could be a leak of secrets, etc.)
        assert_blocked("ls $HOME", "$VAR");
    }

    // ── Phase 04 routing metadata tests ────────────────────────────────

    use crate::command_intent::IntentConfidence;
    use crate::command_intent::RiskLevel;
    use crate::command_planner::plan_execution;
    use crate::command_planner::ExecutionBackend;
    use crate::command_routing::resolve_routing;
    use crate::command_routing::RoutingDecision;
    use crate::config::schema::CommandIntentConfig;
    use crate::config::schema::CommandIntentFamily;
    use crate::config::schema::RouteLevel;

    #[test]
    fn classify_test_command() {
        let intent = classify_command("cargo test");
        assert_eq!(intent.kind, CommandIntentKind::Test);
        assert_eq!(intent.confidence, IntentConfidence::High);
        assert_eq!(intent.risk.level, RiskLevel::Low);
    }

    #[test]
    fn classify_git_readonly_command() {
        let intent = classify_command("git status");
        assert_eq!(intent.kind, CommandIntentKind::GitReadOnly);
        assert_eq!(intent.confidence, IntentConfidence::High);
    }

    #[test]
    fn classify_git_mutable_command() {
        let intent = classify_command("git commit -m 'foo'");
        assert_eq!(intent.kind, CommandIntentKind::GitMutating);
    }

    #[test]
    fn classify_search_command() {
        let intent = classify_command("grep -rn 'pattern' src/");
        assert_eq!(intent.kind, CommandIntentKind::SearchReadOnly);
    }

    #[test]
    fn classify_python_command() {
        let intent = classify_command("python3 script.py");
        assert!(matches!(
            intent.kind,
            CommandIntentKind::PythonAnalyze
                | CommandIntentKind::PythonTransform
                | CommandIntentKind::PythonVerify
        ));
    }

    #[test]
    fn classify_empty_is_rejected() {
        let intent = classify_command("");
        assert_eq!(intent.kind, CommandIntentKind::Rejected);
    }

    #[test]
    fn plan_test_routes_to_test_runner() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::TestRunner { .. }));
        assert_eq!(plan.projector.label(), "test-report");
    }

    #[test]
    fn plan_git_readonly_routes_to_native_tool() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::NativeTool { tool_name } if tool_name == "egggit"
        ));
    }

    #[test]
    fn plan_search_routes_to_managed_argv() {
        let intent = classify_command("grep -rn 'pattern' src/");
        let plan = plan_execution(&intent);
        assert!(matches!(plan.backend, ExecutionBackend::ManagedArgv { .. }));
        assert_eq!(plan.projector.label(), "file-search");
    }

    #[test]
    fn resolve_test_routing() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(
            decision,
            RoutingDecision::RouteToTestRunner { .. }
        ));
    }

    #[test]
    fn resolve_git_readonly_routing() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(
            decision,
            RoutingDecision::RouteToNativeTool { tool_name, .. } if tool_name == "egggit"
        ));
    }

    #[test]
    fn resolve_search_routing() {
        let intent = classify_command("grep -rn 'pattern' src/");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(
            decision,
            RoutingDecision::RouteToManagedProcess { .. }
        ));
    }

    #[test]
    fn config_is_enabled_requires_master_toggle() {
        let mut config = CommandIntentConfig::default();
        config.route_safe_commands = Some(false);
        config.route_tests = Some(RouteLevel::Observe);
        assert!(!config.is_enabled(CommandIntentFamily::Tests));

        config.route_safe_commands = Some(true);
        assert!(config.is_enabled(CommandIntentFamily::Tests));
    }

    #[test]
    fn config_is_enabled_per_family() {
        let mut config = CommandIntentConfig::default();
        config.route_safe_commands = Some(true);
        config.route_tests = Some(RouteLevel::Observe);
        config.route_git_read = Some(RouteLevel::Off);
        config.route_search = Some(RouteLevel::Off);

        assert!(config.is_enabled(CommandIntentFamily::Tests));
        assert!(!config.is_enabled(CommandIntentFamily::GitRead));
        assert!(!config.is_enabled(CommandIntentFamily::Search));
    }

    #[test]
    fn config_all_disabled_by_default() {
        let config = CommandIntentConfig::default();
        assert!(!config.is_enabled(CommandIntentFamily::Tests));
        assert!(!config.is_enabled(CommandIntentFamily::GitRead));
        assert!(!config.is_enabled(CommandIntentFamily::Search));
        assert!(!config.is_enabled(CommandIntentFamily::Python));
    }

    #[tokio::test]
    async fn bash_no_config_produces_no_routing_metadata() {
        let tool = BashTool::new();
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("hello"));
        assert!(!result.contains("[intent:"));
    }

    #[tokio::test]
    async fn bash_with_config_attaches_routing_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("[intent:"));
        assert!(result.contains("backend:"));
        assert!(result.contains("routing:"));
    }

    #[tokio::test]
    async fn bash_test_command_metadata_when_enabled() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"));
        assert!(result.contains("backend: test-runner"));
        assert!(result.contains("projector: test-report"));
        assert!(result.contains("routing: enabled"));
    }

    #[tokio::test]
    async fn bash_test_command_metadata_when_disabled() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"));
        assert!(result.contains("routing: disabled"));
    }

    #[tokio::test]
    async fn bash_git_readonly_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: git-readonly"));
        assert!(result.contains("backend: native-tool"));
        assert!(result.contains("routing: enabled"));
    }

    #[tokio::test]
    async fn bash_search_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_search = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "grep -rn 'pattern' src/"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: search-readonly"));
        assert!(result.contains("backend: managed-argv"));
        assert!(result.contains("projector: file-search"));
        assert!(result.contains("routing: enabled"));
    }

    #[tokio::test]
    async fn bash_python_metadata_when_disabled() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_python = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "python3 -c 'print(1)'"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("python"));
        assert!(result.contains("routing: disabled"));
    }

    #[tokio::test]
    async fn bash_raw_shell_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: raw-shell"));
        assert!(result.contains("backend: raw-shell"));
        assert!(result.contains("routing: disabled"));
    }

    // ── Workstream G: Observe-only mode tests ──────────────────────────

    #[tokio::test]
    async fn observe_mode_runs_raw_shell_for_test_command() {
        // Even when tests are "enabled", observe mode must execute via sh -c,
        // not route to TestRunner. The command must actually run.
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo observe-test-ok"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("observe-test-ok"),
            "command must execute via raw shell"
        );
        assert!(result.contains("mode: observe"));
        assert!(result.contains("intent: raw-shell"));
    }

    #[tokio::test]
    async fn observe_mode_appends_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("[intent:"), "metadata must be present");
        assert!(
            result.contains("mode: observe"),
            "mode must appear in metadata"
        );
    }

    #[tokio::test]
    async fn observe_mode_is_default_when_not_set() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo default-mode"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("mode: observe"),
            "default mode must be observe"
        );
    }

    #[tokio::test]
    async fn route_mode_falls_back_to_observe_and_warns() {
        // When mode = Route, the tool should fall back to observe behavior.
        // The command must still execute via raw shell.
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Route);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo route-fallback-ok"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("route-fallback-ok"),
            "command must execute even in route mode"
        );
        assert!(
            result.contains("mode: route (fallback: observe)"),
            "metadata must show route fallback"
        );
    }

    #[tokio::test]
    async fn route_mode_does_not_change_execution_path() {
        // Verify that even with route mode + all families enabled,
        // the command still executes via raw shell (not routed to any backend).
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        cic.route_git_read = Some(RouteLevel::Observe);
        cic.route_search = Some(RouteLevel::Observe);
        cic.route_python = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Route);

        let tool = BashTool::new().with_command_intent_config(cic);

        // Test command — would route to TestRunner if active routing existed
        let input = serde_json::json!({"command": "echo routing-inactive"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("routing-inactive"),
            "must execute via raw shell"
        );

        // Git command — would route to NativeTool if active routing existed
        let input = serde_json::json!({"command": "echo git-fallback"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("git-fallback"),
            "must execute via raw shell"
        );
    }

    #[tokio::test]
    async fn no_config_produces_no_metadata_no_mode() {
        let tool = BashTool::new();
        let input = serde_json::json!({"command": "echo clean"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("clean"));
        assert!(!result.contains("[intent:"), "no metadata without config");
        assert!(!result.contains("mode:"), "no mode without config");
    }

    #[tokio::test]
    async fn route_safe_commands_true_alone_does_not_enable_routing() {
        // Setting route_safe_commands = true does NOT mean active routing.
        // The mode must be explicitly Route for that (and even then it falls back).
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        // mode is default (Observe)

        let tool = BashTool::new().with_command_intent_config(cic);
        // Use a test command so family_enabled = true for metadata annotation.
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"), "must classify as test");
        assert!(
            result.contains("mode: observe"),
            "must remain in observe mode"
        );
        assert!(
            result.contains("routing: enabled"),
            "family can be enabled for metadata annotation"
        );
    }

    #[test]
    fn command_intent_mode_default_is_observe() {
        let mode = crate::config::schema::CommandIntentMode::default();
        assert_eq!(mode, crate::config::schema::CommandIntentMode::Observe);
    }

    #[test]
    fn command_intent_config_mode_helper() {
        let mut config = CommandIntentConfig::default();
        assert_eq!(
            config.mode(),
            crate::config::schema::CommandIntentMode::Observe
        );
        assert!(!config.is_route_mode());
        assert!(!config.is_active_mode());

        config.mode = Some(crate::config::schema::CommandIntentMode::Route);
        assert_eq!(
            config.mode(),
            crate::config::schema::CommandIntentMode::Route
        );
        assert!(config.is_route_mode());
        assert!(config.is_active_mode());
    }

    #[test]
    fn active_mode_is_active() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        assert!(config.is_active_mode());
        assert!(config.is_route_mode());
    }

    #[test]
    fn family_level_defaults_to_observe_when_mode_is_observe() {
        let config = CommandIntentConfig::default();
        assert_eq!(
            config.family_level(CommandIntentFamily::Tests),
            RouteLevel::Observe
        );
    }

    #[test]
    fn family_level_defaults_to_active_when_mode_is_active() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        assert_eq!(
            config.family_level(CommandIntentFamily::Tests),
            RouteLevel::Active
        );
    }

    #[test]
    fn family_level_uses_override_when_set() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        config.route_tests = Some(RouteLevel::Off);
        assert_eq!(
            config.family_level(CommandIntentFamily::Tests),
            RouteLevel::Off
        );
        // Other families still use global default
        assert_eq!(
            config.family_level(CommandIntentFamily::GitRead),
            RouteLevel::Active
        );
    }

    #[test]
    fn is_active_for_family_requires_active_mode() {
        let mut config = CommandIntentConfig::default();
        config.route_safe_commands = Some(true);
        config.route_tests = Some(RouteLevel::Active);
        // Mode is Observe (default), so active routing should be off
        assert!(!config.is_active_for_family(CommandIntentFamily::Tests));

        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        assert!(config.is_active_for_family(CommandIntentFamily::Tests));
    }

    #[test]
    fn is_active_for_family_requires_active_level() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        config.route_tests = Some(RouteLevel::Observe);
        // Level is Observe, not Active, so active routing should be off
        assert!(!config.is_active_for_family(CommandIntentFamily::Tests));

        config.route_tests = Some(RouteLevel::Active);
        assert!(config.is_active_for_family(CommandIntentFamily::Tests));
    }

    #[test]
    fn route_level_default_is_observe() {
        assert_eq!(RouteLevel::default(), RouteLevel::Observe);
    }

    #[test]
    fn config_all_new_families_default_to_off() {
        let config = CommandIntentConfig::default();
        assert!(!config.is_enabled(CommandIntentFamily::Build));
        assert!(!config.is_enabled(CommandIntentFamily::Lint));
        assert!(!config.is_enabled(CommandIntentFamily::Format));
    }

    // ── Workstream C/L/K: Active routing, kill switches, metrics ────

    #[tokio::test]
    async fn active_mode_routes_git_to_native_tool() {
        // Ensure env kill switch is clear (may be polluted by other tests)
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        // Git status should execute via native tool (Command::new("git"))
        // and produce output with [exit code: 0] or similar
        assert!(
            result.contains("[exit code:"),
            "command must produce exit code in output: {}",
            result
        );
        // Metadata should show active mode
        assert!(
            result.contains("mode: active"),
            "metadata must show active mode: {}",
            result
        );
    }

    #[tokio::test]
    async fn active_mode_test_command_routes_to_test_runner() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        // Test command should execute through the canonical TestRunner.
        assert!(
            result.starts_with("Test run "),
            "command must produce a structured test report"
        );
        assert!(
            result.contains("mode: active"),
            "metadata must show active mode"
        );
        assert!(
            result.contains("intent: test"),
            "metadata must classify as test"
        );
    }

    #[tokio::test]
    async fn observe_mode_still_runs_raw_shell_for_git() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        // Observe mode must execute via raw shell
        assert!(result.contains("mode: observe"));
        assert!(result.contains("[exit code:"));
    }

    #[tokio::test]
    async fn active_mode_off_level_kill_switch_prevents_routing() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Off);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        // Off level kills active routing, falls back to raw shell
        assert!(
            result.contains("mode: active"),
            "metadata shows active mode"
        );
        assert!(result.contains("routing: disabled"), "routing is disabled");
    }

    #[tokio::test]
    async fn env_kill_switch_disables_active_routing() {
        std::env::set_var("CODEGG_ROUTING_DISABLE", "1");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        // Env kill switch forces raw shell
        assert!(result.contains("[exit code:"));
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
    }

    #[tokio::test]
    async fn active_mode_build_command_routes_via_managed_process() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_build = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo check"});
        let result = tool.execute(input).await.unwrap();
        // Build command should execute and produce output
        assert!(
            result.contains("[exit code:"),
            "command must produce exit code in output"
        );
        assert!(
            result.contains("mode: active"),
            "metadata must show active mode"
        );
    }

    #[tokio::test]
    async fn active_mode_python_command_routes() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_python = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "python3 -c 'print(42)'"});
        let result = tool.execute(input).await.unwrap();
        // Python command should execute via python3 -c
        assert!(
            result.contains("42"),
            "python command must produce expected output: {}",
            result
        );
        assert!(
            result.contains("mode: active"),
            "metadata must show active mode"
        );
    }

    #[tokio::test]
    async fn active_mode_search_command_routes() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_search = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "rg --version"});
        let result = tool.execute(input).await.unwrap();
        // Search command should execute via direct Command::new
        assert!(
            result.contains("[exit code:"),
            "command must produce exit code in output"
        );
        assert!(
            result.contains("mode: active"),
            "metadata must show active mode"
        );
    }

    #[test]
    fn kill_switch_checks_env_var() {
        // Clear the env var first to ensure a clean state
        std::env::remove_var("CODEGG_ROUTING_DISABLE");

        let tool = BashTool::new();
        std::env::set_var("CODEGG_ROUTING_DISABLE", "1");
        assert!(tool.check_kill_switches(CommandIntentFamily::Tests));

        // Clean up immediately to avoid polluting other tests
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
    }

    #[test]
    fn kill_switch_checks_off_level() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        assert!(tool.check_kill_switches(CommandIntentFamily::Tests));
    }

    #[test]
    fn kill_switch_allows_active_level() {
        // Clear env var to ensure clean state
        std::env::remove_var("CODEGG_ROUTING_DISABLE");

        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        assert!(!tool.check_kill_switches(CommandIntentFamily::Tests));
    }

    #[test]
    fn intent_kind_to_family_mapping() {
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::Test),
            Some(CommandIntentFamily::Tests)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::GitReadOnly),
            Some(CommandIntentFamily::GitRead)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::SearchReadOnly),
            Some(CommandIntentFamily::Search)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::FileRead),
            Some(CommandIntentFamily::Search)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::PythonAnalyze),
            Some(CommandIntentFamily::Python)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::PythonTransform),
            Some(CommandIntentFamily::Python)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::PythonVerify),
            Some(CommandIntentFamily::Python)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::Build),
            Some(CommandIntentFamily::Build)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::Lint),
            Some(CommandIntentFamily::Lint)
        );
        assert_eq!(
            intent_kind_to_family(CommandIntentKind::Format),
            Some(CommandIntentFamily::Format)
        );
        assert_eq!(intent_kind_to_family(CommandIntentKind::RawShell), None);
        assert_eq!(intent_kind_to_family(CommandIntentKind::Rejected), None);
        assert_eq!(intent_kind_to_family(CommandIntentKind::FileWrite), None);
        assert_eq!(intent_kind_to_family(CommandIntentKind::FileEdit), None);
    }

    #[tokio::test]
    async fn route_mode_still_falls_back_to_observe() {
        // Route is a deprecated alias for Active — should still work for active routing
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Route);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        // Route mode should produce output (active routing since Route == Active)
        assert!(result.contains("[exit code:"));
        assert!(
            result.contains("mode: route (fallback: observe)"),
            "metadata must show route mode"
        );
    }

    #[tokio::test]
    async fn active_mode_raw_shell_falls_back_to_raw_shell() {
        // RawShell commands (e.g., echo) cannot be active-routed — should use raw shell
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo active-fallback"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("active-fallback"),
            "command must execute via raw shell fallback"
        );
    }
}
