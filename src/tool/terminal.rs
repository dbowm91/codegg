use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

use crate::error::ToolError;
use crate::tool::Tool;

const DANGEROUS_ENV_VARS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
];

fn is_safe_env_var_name(name: &str) -> bool {
    if name.is_empty() || name.contains('=') || name.contains('\0') {
        return false;
    }
    if name.starts_with('_') && name.contains("ENV") {
        return false;
    }
    !DANGEROUS_ENV_VARS.contains(&name)
}

static BLOCKED_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?x)
        \$\(            # command substitution
        |`             # backtick substitution
        |\|/.*sh       # pipe to shell
        |\|/.*bash     # pipe to bash
        |> /dev/       # redirect to dev
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
        |(?:^|\s)&(?:[\s]|$)               # standalone &
    ").unwrap()
});

pub struct TerminalTool {
    timeout: Duration,
    max_output_lines: usize,
    max_output_bytes: usize,
    workdir: Option<PathBuf>,
    blocked_commands: HashSet<&'static str>,
    allowlist: Option<HashSet<&'static str>>,
}

impl TerminalTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            max_output_lines: 2000,
            max_output_bytes: 50_000,
            workdir: None,
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
            allowlist: None,
        }
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = Some(dir);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_blocked_commands(mut self, commands: Vec<&'static str>) -> Self {
        self.blocked_commands = commands.into_iter().collect();
        self
    }

    pub fn with_allowlist(mut self, commands: Vec<&'static str>) -> Self {
        self.allowlist = Some(commands.into_iter().collect());
        self
    }

    fn check_command_security(&self, command: &str, args: &[String]) -> Result<(), ToolError> {
        let full_command = format!("{} {}", command, args.join(" "));

        if BLOCKED_PATTERN.is_match(&full_command) {
            return Err(ToolError::Permission(
                "command matches blocked pattern".to_string(),
            ));
        }

        let normalized = full_command.as_str();

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

        if let Some(ref allowlist) = self.allowlist {
            let mut cmd_parts = full_command.split_whitespace();
            let mut cmd = cmd_parts.next().unwrap_or("");

            while ["env", "nohup", "time", "nice", "setuid", "sudo"].contains(&cmd) {
                cmd = cmd_parts.next().unwrap_or("");
            }

            if (cmd == "bash" || cmd == "sh" || cmd == "dash")
                && full_command.contains(" -c ")
            {
                if !allowlist.contains(&cmd) {
                    return Err(ToolError::Permission(format!(
                        "command '{}' not in allowlist",
                        cmd
                    )));
                }
                return Ok(());
            }

            if !allowlist.contains(&cmd) {
                return Err(ToolError::Permission(format!(
                    "command '{}' not in allowlist",
                    cmd
                )));
            }
        }

        Ok(())
    }
}

impl Default for TerminalTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn description(&self) -> &str {
        "Run commands in an interactive terminal session"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute in the terminal"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Arguments to pass to the command"
                },
                "env": {
                    "type": "object",
                    "description": "Environment variables to set (key-value pairs)"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 60)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'command' parameter".to_string()))?;

        let args: Vec<String> = input["args"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let timeout_secs = input["timeout"].as_u64().unwrap_or(60);
        let timeout = Duration::from_secs(timeout_secs);

        let env_vars: Vec<(String, String)> = input["env"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .filter(|(k, _)| is_safe_env_var_name(k))
                    .collect()
            })
            .unwrap_or_default();

        self.check_command_security(command, &args)?;

        tracing::info!("Running terminal command: {} {:?}", command, args);

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = Command::new(command);
            cmd.env_clear();
            if let Some(path) = std::env::var_os("PATH") {
                cmd.env("PATH", path);
            } else {
                cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
            }
            cmd.args(&args);

            for (key, value) in env_vars {
                cmd.env(&key, &value);
            }

            if let Some(ref dir) = self.workdir {
                cmd.current_dir(dir);
            }

            cmd.output().await
        })
        .await
        .map_err(|_| ToolError::Timeout(format!("command timed out after {}s", timeout_secs)))?
        .map_err(|e| ToolError::Execution(e.to_string()))?;

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
