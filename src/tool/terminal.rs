use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};

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

/// One-shot non-interactive shell execution (M003 disposition).
///
/// The historic `terminal` name is retained so stored runs, session
/// imports, permission modes, and agent deny-lists keep resolving, but
/// this tool is NOT an interactive terminal: it runs one command to
/// completion through the canonical finite-execution owner
/// (`ManagedProcessService`, the same owner behind `bash`) and returns
/// captured output. `bash` is the canonical model shell. Human
/// interactive workspace terminals run in the TUI over the daemon
/// interactive-process service (`InteractiveProcessService` + the M002
/// attach/resume protocol), never through this tool.
pub struct TerminalTool {
    timeout: Duration,
    max_output_lines: usize,
    max_output_bytes: usize,
    workdir: Option<PathBuf>,
    blocked_commands: HashSet<&'static str>,
    allowlist: Option<HashSet<&'static str>>,
    allowed_root: Option<PathBuf>,
}

impl TerminalTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            max_output_lines: 2000,
            max_output_bytes: 50_000,
            workdir: None,
            blocked_commands: crate::tool::bash::policy::default_blocked_commands(),
            allowlist: None,
            allowed_root: None,
        }
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = Some(dir);
        self
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = Some(root);
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

    fn effective_script(command: &str, args: &[String]) -> String {
        if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        }
    }

    fn check_command_security(&self, effective_script: &str) -> Result<(), ToolError> {
        // `terminal` is a compatibility/input adapter. The effective string
        // is exactly the payload passed to `sh -c`, and Bash owns its shell
        // safety decision so the two model-facing surfaces cannot drift.
        let parts: Vec<&str> = effective_script.split_whitespace().collect();
        crate::tool::bash::policy::check_shell_security(
            effective_script,
            &parts,
            &self.blocked_commands,
            self.allowlist.as_ref(),
        )
    }

    #[cfg(test)]
    fn check_command_security_for_args(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<(), ToolError> {
        self.check_command_security(&Self::effective_script(command, args))
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
        "Run one shell command to completion and return captured output (one-shot, not an interactive PTY). Bash is the canonical model shell; human interactive terminals run in the TUI over the daemon interactive-process service"
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

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    /// M002: interactive terminal overlaps `bash`; deferred for ordinary turns.
    fn defer_loading(&self) -> bool {
        crate::tool::disclosure::is_deferred_by_default(self.name())
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

        let full_command = Self::effective_script(command, &args);
        self.check_command_security(&full_command)?;
        if let Some(root) = self.allowed_root.as_ref() {
            crate::tool::bash::validate_child_workspace_command(command, &args, root)?;
        }

        tracing::info!("Running terminal command: {} {:?}", command, args);

        let cwd = self
            .workdir
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| ToolError::Execution("terminal cwd could not be resolved".into()))?;
        let mut environment_policy = crate::managed_process::EnvironmentPolicy::sanitized();
        for (key, value) in env_vars {
            environment_policy = environment_policy.with_var(key, value);
        }
        let mut request = crate::managed_process::ManagedProcessRequest::new(
            vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from(full_command),
            ],
            cwd,
            crate::managed_process::ProcessProvenance::default(),
        );
        request.environment_policy = environment_policy;
        request.timeout = Some(timeout);
        request.output_policy = crate::managed_process::OutputPolicy::new(self.max_output_bytes);
        let output = crate::managed_process::ManagedProcessService::run(request)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        if matches!(
            output.termination,
            crate::managed_process::TerminationReason::TimedOut
        ) {
            return Err(ToolError::Timeout(format!(
                "command timed out after {}s",
                timeout_secs
            )));
        }

        let stdout = output.stdout.to_string_lossy();
        let stderr = output.stderr.to_string_lossy();

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
            output.exit_status.code().unwrap_or(-1)
        ));

        Ok(result)
    }
}

fn truncate_output(output: &str, max_lines: usize, max_bytes: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let truncated = if lines.len() > max_lines {
        let head = &lines[..max_lines.div_ceil(2)];
        let tail_count = max_lines / 2;
        let tail = &lines[lines.len() - tail_count..];
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
    use crate::tool::bash::BashTool;

    #[test]
    fn effective_script_matches_the_shell_payload() {
        let args = vec!["--label".to_string(), "café".to_string()];
        assert_eq!(
            TerminalTool::effective_script("printf", &args),
            "printf --label café"
        );
        assert_eq!(TerminalTool::effective_script("true", &[]), "true");
    }

    #[test]
    fn terminal_and_bash_share_shell_safety_decisions() {
        let bash = BashTool::new();
        let terminal = TerminalTool::new();
        let cases = [
            ("printf", vec!["%s".to_string(), "hello".to_string()], false),
            (
                "find",
                vec![".".to_string(), "-name".to_string(), "*.rs".to_string()],
                false,
            ),
            (
                "find",
                vec![".".to_string(), "-exec".to_string(), "rm".to_string()],
                false,
            ),
            ("echo", vec!["${HOME}".to_string()], true),
            ("printf", vec!["$(touch marker)".to_string()], true),
            ("cat", vec![">".to_string(), "/dev/null".to_string()], true),
            (
                "python",
                vec!["-c".to_string(), "print(1)".to_string()],
                true,
            ),
            ("sleep", vec!["5".to_string(), "&".to_string()], true),
            ("bash", vec!["-c".to_string(), "echo ok".to_string()], true),
            (
                "env",
                vec!["rm".to_string(), "-rf".to_string(), "/".to_string()],
                true,
            ),
        ];

        for (command, args, should_block) in cases {
            let script = TerminalTool::effective_script(command, &args);
            let parts: Vec<&str> = script.split_whitespace().collect();
            let bash_result = bash.check_command_security(&script, &parts);
            let terminal_result = terminal.check_command_security_for_args(command, &args);
            assert_eq!(
                bash_result.is_err(),
                should_block,
                "unexpected Bash decision for {script:?}: {bash_result:?}"
            );
            assert_eq!(
                terminal_result.is_err(),
                should_block,
                "unexpected terminal decision for {script:?}: {terminal_result:?}"
            );
            assert_eq!(
                bash_result.is_err(),
                terminal_result.is_err(),
                "Bash and terminal diverged for {script:?}"
            );
        }
    }

    #[test]
    fn terminal_custom_restrictions_only_narrow_canonical_policy() {
        let terminal = TerminalTool::new().with_blocked_commands(vec!["printf"]);
        assert!(terminal
            .check_command_security_for_args("printf", &["ok".to_string()])
            .is_err());

        let allowlisted = TerminalTool::new().with_allowlist(vec!["printf"]);
        assert!(allowlisted
            .check_command_security_for_args("printf", &["ok".to_string()])
            .is_ok());
        assert!(allowlisted
            .check_command_security_for_args("echo", &["ok".to_string()])
            .is_err());
        assert!(allowlisted
            .check_command_security_for_args("printf", &["$(touch marker)".to_string()])
            .is_err());

        let shell_allowlisted = TerminalTool::new().with_allowlist(vec!["sh"]);
        assert!(shell_allowlisted
            .check_command_security_for_args(
                "sh",
                &["-c".to_string(), "$(touch marker)".to_string()]
            )
            .is_err());
    }

    #[tokio::test]
    async fn blocked_effective_script_does_not_spawn_a_side_effect() {
        let workspace = tempfile::tempdir().expect("temporary terminal workspace");
        let marker = workspace.path().join("marker");
        let tool = TerminalTool::new().with_workdir(workspace.path().to_path_buf());
        let result = tool
            .execute(json!({
                "command": "printf",
                "args": ["$(touch marker)"]
            }))
            .await;

        assert!(result.is_err(), "command substitution must be rejected");
        assert!(
            !marker.exists(),
            "blocked command must not spawn its side effect"
        );
    }
}
