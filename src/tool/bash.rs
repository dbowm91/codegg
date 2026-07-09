use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use std::time::Duration;
use tokio::process::Command;

use crate::command_intent::{classify_command, CommandIntentKind};
use crate::command_planner::plan_execution;
use crate::command_routing::resolve_routing;
use crate::config::schema::CommandIntentConfig;
use crate::config::schema::CommandIntentMode;
use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::security::sandbox::{get_default_allowed_paths, get_sensitive_paths, SandboxConfig};
use crate::tool::{Tool, ToolCategory};

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
        let timeout = Duration::from_secs(timeout_secs);

        let workdir = input["workdir"].as_str().map(|s| s.to_string());

        // Phase 04: Classify command intent and attach routing metadata.
        // All commands still execute via raw shell in this phase; metadata
        // is for visibility and prepares for future structured routing.
        //
        // Workstream G: The `mode` field controls whether this is observe-only
        // (default) or active routing. Route mode is not yet implemented — if
        // configured, we log a warning and fall back to observe behavior.
        let routing_metadata = if let Some(ref cic) = self.command_intent_config {
            let mode = cic.mode();

            if mode == CommandIntentMode::Route {
                tracing::warn!(
                    "command_intent.mode = \"route\" is not yet implemented; \
                     falling back to observe mode. Active routing will be enabled \
                     in a future phase."
                );
            }

            let intent = classify_command(command);
            let plan = plan_execution(&intent);
            let decision = resolve_routing(&plan);

            let family_enabled = match intent.kind {
                CommandIntentKind::Test => {
                    cic.is_enabled(crate::config::schema::CommandIntentFamily::Tests)
                }
                CommandIntentKind::GitReadOnly => {
                    cic.is_enabled(crate::config::schema::CommandIntentFamily::GitRead)
                }
                CommandIntentKind::SearchReadOnly | CommandIntentKind::FileRead => {
                    cic.is_enabled(crate::config::schema::CommandIntentFamily::Search)
                }
                CommandIntentKind::PythonAnalyze
                | CommandIntentKind::PythonTransform
                | CommandIntentKind::PythonVerify => {
                    cic.is_enabled(crate::config::schema::CommandIntentFamily::Python)
                }
                _ => false,
            };

            Some(RoutingMetadata {
                intent_kind: intent.kind.label().to_string(),
                backend_label: plan.backend.label().to_string(),
                projector_label: plan.projector.label().to_string(),
                rtk_eligible: plan.rtk_policy.is_rtk_eligible(),
                confidence: format!("{:?}", intent.confidence).to_lowercase(),
                risk_level: format!("{:?}", intent.risk.level).to_lowercase(),
                routing_enabled: family_enabled,
                routing_decision: format!("{:?}", decision),
                mode,
            })
        } else {
            None
        };
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

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = Command::new("sh");
            cmd.env_clear();
            // Restore essential environment variables for development tools
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
                    // Handle wildcard prefix matching (e.g., "CARGO_PROFILE_*")
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
            if let Some(ref dir) = canonical_workdir {
                cmd.current_dir(dir);
            }
            cmd.kill_on_drop(true);
            cmd.output().await
        })
        .await
        .map_err(|_| ToolError::Timeout(command.to_string()))?
        .map_err(|e| ToolError::Execution(e.to_string()))?;

        let elapsed = start.elapsed();
        tracing::info!("Completed in {elapsed:?}");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

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

    #[test]
    fn classify_test_command() {
        let intent = classify_command("cargo test");
        assert_eq!(intent.kind, CommandIntentKind::Test);
        assert_eq!(intent.confidence, IntentConfidence::High);
        assert_eq!(intent.risk.level, RiskLevel::Safe);
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
        config.route_tests = Some(true);
        assert!(!config.is_enabled(CommandIntentFamily::Tests));

        config.route_safe_commands = Some(true);
        assert!(config.is_enabled(CommandIntentFamily::Tests));
    }

    #[test]
    fn config_is_enabled_per_family() {
        let mut config = CommandIntentConfig::default();
        config.route_safe_commands = Some(true);
        config.route_tests = Some(true);
        config.route_git_read = Some(false);
        config.route_search = Some(false);

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
        cic.route_tests = Some(true);

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
        cic.route_tests = Some(true);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --no-run"});
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
        cic.route_tests = Some(false);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --no-run"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"));
        assert!(result.contains("routing: disabled"));
    }

    #[tokio::test]
    async fn bash_git_readonly_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(true);

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
        cic.route_search = Some(true);

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
        cic.route_python = Some(false);

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
        cic.route_tests = Some(false);

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
        cic.route_tests = Some(true);
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
        cic.route_tests = Some(true);

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
        cic.route_tests = Some(true);
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
        cic.route_tests = Some(true);
        cic.route_git_read = Some(true);
        cic.route_search = Some(true);
        cic.route_python = Some(true);
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
        cic.route_tests = Some(true);
        // mode is default (Observe)

        let tool = BashTool::new().with_command_intent_config(cic);
        // Use a test command so family_enabled = true for metadata annotation.
        let input = serde_json::json!({"command": "cargo test --no-run"});
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

        config.mode = Some(crate::config::schema::CommandIntentMode::Route);
        assert_eq!(
            config.mode(),
            crate::config::schema::CommandIntentMode::Route
        );
        assert!(config.is_route_mode());
    }
}
