//! Bash-owned command policy and classification glue.
//!
//! This module owns only the decision-making helpers that are local to the
//! `bash` tool: blocked-pattern scanning, `blocked_commands` / `allowlist`
//! enforcement, isolated child-workspace validation, kill-switch evaluation,
//! and thin adapters over the canonical command-intent owners.
//!
//! Canonical behavior lives elsewhere and is invoked here, never copied:
//!
//! - destructive classification: `crate::tool::destructive`
//! - command classification/planning: `crate::command_intent`
//! - sandbox/path policy: `crate::security::sandbox`
//! - scheduler admission: `crate::scheduler::JobSubmissionService`
//! - output projection: shell/projector helpers via the dispatch path
//!
//! The pre-spawn authorization order enforced by `BashTool::execute` is:
//!
//! ```text
//! command length
//!   -> check_command_security (blocked list, allowlist, injection patterns)
//!   -> preflight service decision (block is terminal)
//!   -> validate_child_workspace_command (child worktree ceiling)
//!   -> allowed_paths workdir resolution (fail-closed)
//!   -> command-intent classify/plan (observe vs active route)
//!   -> spawn (raw shell) or scheduler submission (active route)
//! ```
//!
//! No function in this module spawns a process or submits a scheduler job.

use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use super::BashTool;
use crate::command_intent::plan::{CommandPlan, ExecutionBackend};
#[cfg(test)]
use crate::command_intent::CommandIntentKind;
use crate::config::schema::CommandIntentFamily;
use crate::error::ToolError;

pub(crate) const MAX_COMMAND_LENGTH: usize = 100_000;

/// Metrics for routing decisions — recorded per command execution.
#[derive(Debug, Clone)]
pub(crate) struct RoutingMetric {
    pub(crate) family: CommandIntentFamily,
    pub(crate) decision: String,
    pub(crate) fallback: bool,
}

/// Compatibility helper for older tests/callers. The canonical family
/// mapping is owned by `command_intent::plan`.
#[cfg(test)]
pub(crate) fn intent_kind_to_family(kind: CommandIntentKind) -> Option<CommandIntentFamily> {
    crate::command_intent::plan::command_intent_family_for_kind(kind)
}

/// Compatibility helper delegating to the plan-owned family mapping.
pub(crate) fn plan_family(plan: &CommandPlan) -> Option<CommandIntentFamily> {
    plan.command_family()
}

/// Map an `ExecutionBackend` to a `PlannedBackend` for persistence provenance.
pub(crate) fn plan_to_planned_backend(
    plan_backend: Option<&ExecutionBackend>,
) -> codegg_core::run_store::PlannedBackend {
    use codegg_core::run_store::PlannedBackend;
    match plan_backend {
        None => PlannedBackend::Unrouted,
        Some(ExecutionBackend::RawShell { .. }) => PlannedBackend::RawShell,
        Some(ExecutionBackend::TestRunner { .. }) => PlannedBackend::TestRunner,
        Some(ExecutionBackend::PythonScript { .. }) => PlannedBackend::PythonScript,
        Some(ExecutionBackend::NativeTool { .. }) => PlannedBackend::NativeTool,
        Some(ExecutionBackend::ManagedArgv { .. }) => PlannedBackend::ManagedArgv,
        Some(ExecutionBackend::Git { .. }) => PlannedBackend::Git,
        Some(ExecutionBackend::Reject { .. }) => PlannedBackend::Unrouted,
    }
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
    (
        "variable expansion $VAR or special parameter",
        r"\$([A-Za-z_][A-Za-z0-9_]*|[0-9!@#?*$-])",
    ),
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
pub(crate) fn find_blocked_pattern(command: &str) -> Option<&'static str> {
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
pub(crate) fn routing_metadata_risk_caps(command: &str) -> (bool, bool, bool) {
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

pub(crate) fn validate_child_workspace_command(
    command: &str,
    args: &[String],
    root: &Path,
) -> Result<(), ToolError> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        ToolError::Permission(format!(
            "isolated child shell root {} cannot be canonicalized: {error}",
            root.display()
        ))
    })?;
    let mut tokens = vec![command.to_string()];
    tokens.extend(args.iter().cloned());
    for token in tokens
        .iter()
        .flat_map(|value| value.split(|ch: char| ch.is_whitespace() || ";|&<>()".contains(ch)))
    {
        if matches!(token, ".." | "cd" | "pushd" | "popd") {
            return Err(ToolError::Permission(
                "isolated child shell cannot change or escape its worktree".into(),
            ));
        }
        if token.starts_with('/') && !Path::new(token).starts_with(&root) {
            return Err(ToolError::Permission(format!(
                "isolated child shell path is outside worktree: {token}"
            )));
        }
    }
    Ok(())
}

impl BashTool {
    /// Check if active routing is disabled by any kill switch.
    pub(crate) fn check_kill_switches(&self, family: CommandIntentFamily) -> bool {
        // 1. Check env var emergency disable (or test override)
        let env_disabled = self
            .routing_disabled_override
            .unwrap_or_else(|| std::env::var("CODEGG_ROUTING_DISABLE").unwrap_or_default() == "1");
        if env_disabled {
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
    pub(crate) fn record_routing_metric(&self, metric: RoutingMetric) {
        tracing::debug!(
            family = ?metric.family,
            decision = %metric.decision,
            fallback = metric.fallback,
            "routing metric"
        );
    }

    pub(crate) fn check_command_security(
        &self,
        command: &str,
        parts: &[&str],
    ) -> Result<(), ToolError> {
        if parts.is_empty() {
            return Ok(());
        }

        let normalized = parts.join(" ");
        let mut command_start = 0;
        while command_start < parts.len()
            && ["env", "nohup", "time", "nice", "setuid", "sudo"].contains(&parts[command_start])
        {
            command_start += 1;
        }
        let normalized_without_prefix = parts[command_start..].join(" ");

        // Check blocked commands first (entire command string)
        let blocked = &self.blocked_commands;
        if !blocked.is_empty() {
            for blocked_cmd in blocked {
                if normalized.starts_with(blocked_cmd)
                    || normalized_without_prefix.starts_with(blocked_cmd)
                {
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

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::command_intent::classify_command;
    use crate::command_intent::plan::plan_execution;
    use crate::command_intent::IntentConfidence;
    use crate::command_intent::RiskLevel;
    use crate::command_routing::resolve_routing;
    use crate::command_routing::RoutingDecision;
    use crate::config::schema::CommandIntentConfig;
    use crate::config::schema::RouteLevel;

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
    fn isolated_child_shell_rejects_parent_paths_and_directory_changes() {
        let dir = tempfile::tempdir().expect("isolated child shell test root");
        let root = dir.path();
        assert!(validate_child_workspace_command("echo ok", &[], root).is_ok());
        assert!(validate_child_workspace_command("cd ..", &[], root).is_err());
        let canonical_root = std::fs::canonicalize(root).expect("test root must canonicalize");
        let outside = canonical_root
            .parent()
            .expect("test root must have a parent")
            .join("codegg-isolated-child-parent.txt");
        assert!(validate_child_workspace_command(
            &format!("echo ok > {}", outside.display()),
            &[],
            root
        )
        .is_err());
        let inside = canonical_root.join("child.txt");
        assert!(validate_child_workspace_command(
            &format!("echo ok > {}", inside.display()),
            &[],
            root
        )
        .is_ok());
    }

    #[test]
    fn isolated_child_shell_fails_closed_when_root_canonicalize_fails() {
        let root = std::env::temp_dir().join("codegg-isolated-child-missing-root");
        assert!(validate_child_workspace_command("echo ok", &[], &root).is_err());
    }

    #[test]
    fn find_exec_is_allowed() {
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
        assert_blocked("exec rm -rf /", "exec");
        assert_blocked("cat foo | exec sh", "exec");
        assert_blocked("ls; exec ls", "exec");
    }

    #[test]
    fn command_substitution_is_blocked() {
        assert_blocked("echo $(rm -rf /)", "$(");
        assert_blocked("echo `rm -rf /`", "backtick");
    }

    #[test]
    fn pipe_to_shell_is_blocked() {
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
        assert_allowed("ls && echo done");
    }

    #[test]
    fn fork_bomb_is_blocked_via_blocklist() {
        let tool = BashTool::new();
        let parts: Vec<&str> = ":(){:|:&};:".split_whitespace().collect();
        let result = tool.check_command_security(":(){:|:&};:", &parts);
        assert!(result.is_err(), "fork bomb should be blocked");
    }

    #[test]
    fn safe_env_var_is_blocked() {
        assert_blocked("ls $HOME", "$VAR");
    }

    #[test]
    fn special_shell_parameters_are_blocked() {
        for parameter in ["$@", "$*", "$#", "$?", "$-", "$!", "$0", "$9"] {
            assert_blocked(&format!("echo {parameter}"), "variable expansion");
        }
    }

    #[test]
    fn blocked_commands_strip_execution_prefixes() {
        let tool = BashTool::new();
        for command in ["env rm -rf /", "nohup env rm -rf /", "sudo rm -rf /"] {
            let parts: Vec<&str> = command.split_whitespace().collect();
            assert!(
                tool.check_command_security(command, &parts).is_err(),
                "blocked command prefix should be rejected: {command}"
            );
        }
    }

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
        assert!(matches!(
            plan.backend,
            crate::command_intent::plan::ExecutionBackend::TestRunner { .. }
        ));
        assert_eq!(plan.projector.label(), "test-report");
    }

    #[test]
    fn plan_git_readonly_routes_to_git_backend() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            crate::command_intent::plan::ExecutionBackend::Git { .. }
        ));
    }

    #[test]
    fn plan_search_routes_to_managed_argv() {
        let intent = classify_command("grep -rn 'pattern' src/");
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            crate::command_intent::plan::ExecutionBackend::ManagedArgv { .. }
        ));
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
        assert!(matches!(decision, RoutingDecision::RouteToGit { .. }));
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

    #[test]
    fn kill_switch_checks_env_var() {
        let tool = BashTool::new().with_routing_disabled_env(true);
        assert!(tool.check_kill_switches(CommandIntentFamily::Tests));
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
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Active);

        let tool = BashTool::new()
            .with_routing_disabled_env(false)
            .with_command_intent_config(cic);
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
            intent_kind_to_family(CommandIntentKind::GitMutating),
            Some(CommandIntentFamily::GitLocalMutation)
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

    #[test]
    fn plan_family_resolves_typed_git_operation_families() {
        let cases = [
            ("git add src/main.rs", CommandIntentFamily::GitLocalMutation),
            ("git commit -m fix", CommandIntentFamily::GitLocalMutation),
            (
                "git stash push -m wip",
                CommandIntentFamily::GitLocalMutation,
            ),
            ("git fetch origin", CommandIntentFamily::GitNetwork),
            ("git push origin main", CommandIntentFamily::GitNetwork),
            (
                "git reset --hard HEAD~1",
                CommandIntentFamily::GitDestructive,
            ),
            ("git clean -f", CommandIntentFamily::GitDestructive),
            (
                "git push --force origin main",
                CommandIntentFamily::GitDestructive,
            ),
        ];
        for (cmd, expected) in cases {
            let intent = classify_command(cmd);
            let plan = plan_execution(&intent);
            assert_eq!(
                plan_family(&plan),
                Some(expected),
                "plan_family({:?}) = {:?}, expected {:?}",
                cmd,
                plan_family(&plan),
                expected
            );
        }
    }

    #[test]
    fn plan_family_non_git_intents_delegate_to_intent_kind_to_family() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert_eq!(plan_family(&plan), Some(CommandIntentFamily::Tests));

        let intent = classify_command("rg pattern src/");
        let plan = plan_execution(&intent);
        assert_eq!(plan_family(&plan), Some(CommandIntentFamily::Search));

        let intent = classify_command("echo hi");
        let plan = plan_execution(&intent);
        assert_eq!(plan_family(&plan), None);
    }
}
