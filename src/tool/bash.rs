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

static BLOCKED_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?x)
        \$\(            # command substitution
        |\$\{           # braced command substitution ${{...}}
        |`             # backtick substitution
        |\$[A-Za-z_][A-Za-z0-9_]*  # variable expansion $VAR
        |\|/.*sh       # pipe to shell
        |\|/.*bash     # pipe to bash
        |> /dev/       # redirect to dev
        |< /dev/       # input redirect from dev
        |2> /dev/      # redirect stderr to dev
        |&[\s\n\r]*&[\s\n\r]*rm[\s\n\r]+-rf  # fork bomb with rm
        |\|\|[\s\n\r]*rm[\s\n\r]+-rf       # || rm -rf
        |%\{[^}]*\|\s*&                   # printf injection
        |eval\s*\(                        # eval
        |exec\s+                          # exec
        |source\s+.*\.sh                  # source shell script
        |\.\s+.*\.sh                      # dot source shell script
        |base64\s+-d                       # base64 decode
        |xxd\s+-r                          # hex reverse
        |perl\s+-e                         # perl -e
        |python\s+-c                       # python -c
        |ruby\s+-e                         # ruby -e
        |node\s+-e                         # node -e
        |nohup\s+.*&\s*$                   # nohup background
        |nohup\s+.*\s+&                   # nohup with &
        |disown\s+-a                       # disown all
        |kill\s+-9\s+-1                    # kill all
        |killall\s+-9                      # killall -9
        |pkill\s+-9                        # pkill -9
        |chmod\s+[0-7]{4}\s+/etc           # chmod to /etc
        |chmod\s+[0-7]{4}\s+/home          # chmod to /home
        |chmod\s+[0-7]{4}\s+/root          # chmod to /root
        |chmod\s+[0-7]{4}\s+/var           # chmod to /var
        |chmod\s+[0-7]{4}\s+/ssh           # chmod to /ssh
        |chmod\s+[0-7]{4}\s+/proc          # chmod to /proc
        |chmod\s+[0-7]{4}\s+/sys           # chmod to /sys
        |chmod\s+777\s+/                   # chmod 777 to root
        |chown\s+.*\s+/etc                 # chown to /etc
        |chown\s+.*\s+/home                # chown to /home
        |chown\s+.*\s+/root                # chown to /root
        |chown\s+.*\s+/var                # chown to /var
        |chown\s+.*\s+/ssh                # chown to /ssh
        |chown\s+.*\s+/proc               # chown to /proc
        |chown\s+.*\s+/sys                # chown to /sys
        |wget\s+.*-O\s+/                   # wget to root
        |curl\s+.*-o\s+/                  # curl to root
        |:\(\)\s*:\s*\|                   # fork bomb
        |&&                                # &&
        |\|\|                              # || pipe
        |(?:^|\s)&(?:[\s]|$)               # standalone &
        |;\s*                              # semicolon
    ").unwrap()
});

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
            let normalized = parts.join(" ");

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
        if BLOCKED_PATTERN.is_match(command) {
            return Err(ToolError::Permission(
                "command matches blocked pattern".to_string(),
            ));
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
            let (program, args) = match parts.split_first() {
                Some((p, a)) => (p, a),
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "empty command",
                    ))
                }
            };
            let mut cmd = Command::new(program);
            cmd.env_clear();
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
            cmd.args(args);
            if let Some(ref dir) = canonical_workdir {
                cmd.current_dir(dir);
            }
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
        format!("{}... [output truncated]", &truncated[..max_bytes])
    } else {
        truncated
    }
}
