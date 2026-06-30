use async_trait::async_trait;

use crate::config::schema::{CommandStdinMode, CommandStdoutMode};
use crate::protocol::plugin::{PluginInvocation, PluginResponse};

use super::{PluginRuntime, RuntimeError, RuntimeLimits};

/// Configuration for a process-backed plugin runtime.
#[derive(Debug, Clone)]
pub struct ProcessRuntimeSpec {
    pub command: String,
    pub args: Vec<String>,
    pub stdin: CommandStdinMode,
    pub stdout: CommandStdoutMode,
    pub timeout_ms: Option<u64>,
    pub cwd: Option<String>,
    pub env: Vec<String>,
}

/// A plugin runtime that executes commands as child processes.
pub struct ProcessRuntime {
    spec: ProcessRuntimeSpec,
    limits: RuntimeLimits,
}

impl ProcessRuntime {
    pub fn new(spec: ProcessRuntimeSpec, limits: RuntimeLimits) -> Self {
        Self { spec, limits }
    }

    pub fn with_defaults(spec: ProcessRuntimeSpec) -> Self {
        Self::new(spec, RuntimeLimits::default())
    }
}

impl From<crate::command::ProcessCommandSpec> for ProcessRuntimeSpec {
    fn from(spec: crate::command::ProcessCommandSpec) -> Self {
        Self {
            command: spec.command,
            args: spec.args,
            stdin: spec.stdin,
            stdout: spec.stdout,
            timeout_ms: Some(spec.timeout_ms),
            cwd: spec.cwd,
            env: spec.env,
        }
    }
}

impl From<&crate::plugin::manifest::PluginRuntimeSpec> for Option<ProcessRuntimeSpec> {
    fn from(runtime: &crate::plugin::manifest::PluginRuntimeSpec) -> Self {
        match runtime {
            crate::plugin::manifest::PluginRuntimeSpec::Process {
                command,
                args,
                timeout_ms,
            } => Some(ProcessRuntimeSpec {
                command: command.clone(),
                args: args.clone(),
                stdin: CommandStdinMode::Json,
                stdout: CommandStdoutMode::Auto,
                timeout_ms: *timeout_ms,
                cwd: None,
                env: Vec::new(),
            }),
            _ => None,
        }
    }
}

#[async_trait]
impl PluginRuntime for ProcessRuntime {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError> {
        let timeout_ms = self.spec.timeout_ms.unwrap_or(self.limits.timeout_ms);

        let mut cmd = tokio::process::Command::new(&self.spec.command);
        cmd.args(&self.spec.args)
            .args(&invocation.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(ref cwd) = self.spec.cwd {
            cmd.current_dir(cwd);
        }

        for env_var in &self.spec.env {
            if let Some((key, value)) = env_var.split_once('=') {
                cmd.env(key, value);
            }
        }

        let mut child = cmd.spawn().map_err(|e| {
            RuntimeError::Spawn(format!("failed to spawn '{}': {}", self.spec.command, e))
        })?;

        // Write stdin if needed
        if self.spec.stdin == CommandStdinMode::Json {
            if let Some(ref mut stdin) = child.stdin {
                let json = serde_json::to_string(&invocation).map_err(|e| {
                    RuntimeError::InvalidJson(format!("failed to serialize invocation: {e}"))
                })?;
                use tokio::io::AsyncWriteExt;
                stdin
                    .write_all(json.as_bytes())
                    .await
                    .map_err(|e| RuntimeError::Io(format!("failed to write stdin: {e}")))?;
            }
        }
        // Drop stdin to signal EOF
        drop(child.stdin.take());

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| RuntimeError::Timeout { timeout_ms })?
        .map_err(|e| {
            RuntimeError::Io(format!("failed to wait for '{}': {}", self.spec.command, e))
        })?;

        let stdout_raw = truncate_bytes(&output.stdout, self.limits.max_stdout_bytes);
        let stderr_raw = truncate_bytes(&output.stderr, self.limits.max_stderr_bytes);

        let exit_code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            let stdout_str = String::from_utf8_lossy(&stdout_raw).to_string();
            let stderr_str = String::from_utf8_lossy(&stderr_raw).to_string();
            return Err(RuntimeError::NonZeroExit {
                code: exit_code,
                stdout: stdout_str,
                stderr: stderr_str,
            });
        }

        let stdout_str = String::from_utf8_lossy(&stdout_raw).to_string();
        let stderr_str = String::from_utf8_lossy(&stderr_raw).to_string();

        parse_process_output(&stdout_str, &stderr_str, &self.spec.stdout)
    }
}

/// Parse process stdout/stderr into a PluginResponse based on the output mode.
fn parse_process_output(
    stdout: &str,
    stderr: &str,
    mode: &CommandStdoutMode,
) -> Result<PluginResponse, RuntimeError> {
    match mode {
        CommandStdoutMode::Text => Ok(text_to_response(stdout, stderr)),
        CommandStdoutMode::Json => {
            let response: PluginResponse = serde_json::from_str(stdout).map_err(|e| {
                RuntimeError::InvalidJson(format!("invalid PluginResponse JSON: {e}"))
            })?;
            Ok(response)
        }
        CommandStdoutMode::Auto => {
            // Try JSON first, fall back to text
            if let Ok(response) = serde_json::from_str::<PluginResponse>(stdout) {
                Ok(response)
            } else {
                Ok(text_to_response(stdout, stderr))
            }
        }
    }
}

/// Convert plain text output into a PluginResponse with an EmitChat effect.
fn text_to_response(stdout: &str, stderr: &str) -> PluginResponse {
    use crate::protocol::ui::{ChatBlock, ChatFormat, UiEffect};

    let mut effects = Vec::new();
    if !stdout.is_empty() {
        effects.push(UiEffect::EmitChat {
            block: ChatBlock {
                format: ChatFormat::Markdown,
                content: stdout.to_string(),
            },
        });
    }

    let mut diagnostics = Vec::new();
    if !stderr.is_empty() {
        diagnostics.push(crate::protocol::plugin::PluginDiagnostic {
            level: crate::protocol::plugin::PluginDiagnosticLevel::Warning,
            message: stderr.to_string(),
        });
    }

    PluginResponse {
        ok: true,
        effects,
        data: serde_json::Value::Null,
        diagnostics,
    }
}

fn truncate_bytes(bytes: &[u8], max: usize) -> Vec<u8> {
    if bytes.len() <= max {
        bytes.to_vec()
    } else {
        bytes[..max].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_to_response_creates_chat_effect() {
        let resp = text_to_response("hello world", "");
        assert!(resp.ok);
        assert_eq!(resp.effects.len(), 1);
        assert!(resp.diagnostics.is_empty());
    }

    #[test]
    fn text_to_response_with_stderr_adds_diagnostic() {
        let resp = text_to_response("output", "warning");
        assert!(resp.ok);
        assert_eq!(resp.effects.len(), 1);
        assert_eq!(resp.diagnostics.len(), 1);
        assert_eq!(
            resp.diagnostics[0].level,
            crate::protocol::plugin::PluginDiagnosticLevel::Warning
        );
    }

    #[test]
    fn text_to_response_empty_is_valid() {
        let resp = text_to_response("", "");
        assert!(resp.ok);
        assert!(resp.effects.is_empty());
        assert!(resp.diagnostics.is_empty());
    }

    #[test]
    fn parse_json_output() {
        let json = r#"{"ok": true, "effects": [], "data": {"key": "value"}, "diagnostics": []}"#;
        let resp = parse_process_output(json, "", &CommandStdoutMode::Json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn parse_json_output_invalid() {
        let result = parse_process_output("not json", "", &CommandStdoutMode::Json);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RuntimeError::InvalidJson(_)));
    }

    #[test]
    fn auto_mode_falls_back_to_text() {
        let resp = parse_process_output("plain text", "", &CommandStdoutMode::Auto).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.effects.len(), 1);
    }

    #[test]
    fn auto_mode_parses_json() {
        let json = r#"{"ok": true, "effects": [], "data": {}, "diagnostics": []}"#;
        let resp = parse_process_output(json, "", &CommandStdoutMode::Auto).unwrap();
        assert!(resp.ok);
        assert!(resp.effects.is_empty());
    }

    #[test]
    fn truncate_bytes_works() {
        assert_eq!(truncate_bytes(&[1, 2, 3], 5), vec![1, 2, 3]);
        assert_eq!(truncate_bytes(&[1, 2, 3, 4, 5], 3), vec![1, 2, 3]);
        assert_eq!(truncate_bytes(&[], 5), Vec::<u8>::new());
    }

    #[test]
    fn process_spec_from_command_spec() {
        let cmd_spec = crate::command::ProcessCommandSpec {
            command: "my-cmd".to_string(),
            args: vec!["--flag".to_string()],
            stdin: CommandStdinMode::Json,
            stdout: CommandStdoutMode::Auto,
            timeout_ms: 10000,
            cwd: Some("/tmp".to_string()),
            env: vec!["FOO=bar".to_string()],
            output: Vec::new(),
        };
        let runtime_spec: ProcessRuntimeSpec = cmd_spec.into();
        assert_eq!(runtime_spec.command, "my-cmd");
        assert_eq!(runtime_spec.args, vec!["--flag"]);
        assert_eq!(runtime_spec.timeout_ms, Some(10000));
        assert_eq!(runtime_spec.cwd.as_deref(), Some("/tmp"));
        assert_eq!(runtime_spec.env, vec!["FOO=bar"]);
    }

    #[test]
    fn process_spec_from_manifest_runtime() {
        let manifest_runtime = crate::plugin::manifest::PluginRuntimeSpec::Process {
            command: "plugin-cmd".to_string(),
            args: vec!["arg1".to_string()],
            timeout_ms: Some(3000),
        };
        let opt: Option<ProcessRuntimeSpec> = (&manifest_runtime).into();
        assert!(opt.is_some());
        let spec = opt.unwrap();
        assert_eq!(spec.command, "plugin-cmd");
        assert_eq!(spec.stdin, CommandStdinMode::Json);
        assert_eq!(spec.stdout, CommandStdoutMode::Auto);
    }

    #[test]
    fn manifest_builtin_runtime_returns_none() {
        let manifest_runtime = crate::plugin::manifest::PluginRuntimeSpec::Builtin {
            handler: "test".to_string(),
        };
        let opt: Option<ProcessRuntimeSpec> = (&manifest_runtime).into();
        assert!(opt.is_none());
    }

    #[test]
    fn manifest_wasm_runtime_returns_none() {
        let manifest_runtime = crate::plugin::manifest::PluginRuntimeSpec::Wasm {
            module: "test.wasm".to_string(),
            timeout_ms: None,
            memory_max_mb: None,
            fuel_per_call: None,
        };
        let opt: Option<ProcessRuntimeSpec> = (&manifest_runtime).into();
        assert!(opt.is_none());
    }
}
