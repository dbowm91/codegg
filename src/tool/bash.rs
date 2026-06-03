use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;

use std::time::Duration;
use tokio::process::Command;

use crate::error::ToolError;
use crate::security::sandbox::{get_default_allowed_paths, get_sensitive_paths, SandboxConfig};
use crate::tool::Tool;

const MAX_COMMAND_LENGTH: usize = 100_000;

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
    ("fork bomb with rm -rf", r"&[\s\n\r]*&[\s\n\r]*rm[\s\n\r]+-rf"),
    ("|| rm -rf", r"\|\|[\s\n\r]*rm[\s\n\r]+-rf"),
    ("printf injection %{...}|&", r"%\{[^}]*\|\s*&"),
    ("eval(", r"eval\s*\("),
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
        .map(|(name, pat)| (*name, Regex::new(pat).expect("invalid blocked pattern regex")))
        .collect()
});

/// Returns the name of the first matching blocked pattern, or None.
fn find_blocked_pattern(command: &str) -> Option<&'static str> {
    for (name, re) in BLOCKED_PATTERN_REGEXES.iter() {
        if re.is_match(command) {
            return Some(*name);
        }
    }
    None
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

                let full_match = allowlist.iter().any(|allowed| normalized.starts_with(allowed));
                if !full_match {
                    return Err(ToolError::Permission(format!(
                        "command '{}' not in allowlist",
                        normalized
                    )));
                }
                return Ok(());
            }

            if !allowlist.contains(&cmd) {
                let full_match = allowlist.iter().any(|allowed| normalized.starts_with(allowed));
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
        "Execute a shell command and return its output"
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

        let timeout_secs = input["timeout"].as_u64().unwrap_or(120);
        let timeout = Duration::from_secs(timeout_secs);

        let workdir = input["workdir"].as_str().map(|s| s.to_string());
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
                tokio::task::spawn_blocking(move || {
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
                if var.ends_with('*') {
                    // Handle wildcard prefix matching (e.g., "CARGO_PROFILE_*")
                    let prefix = &var[..var.len() - 1];
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
        assert!(
            pat.is_some(),
            "expected blocked but allowed: {}",
            command
        );
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
}
