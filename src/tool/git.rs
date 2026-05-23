use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

use crate::error::ToolError;
use crate::tool::Tool;

pub struct GitTool {
    timeout: Duration,
    workdir: PathBuf,
}

impl GitTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for GitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GitTool {
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "Execute git commands and operations"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "Git subcommand (e.g., status, log, diff, branch, add, commit, checkout)"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Arguments to pass to the git subcommand"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for git command (default: current directory)"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 30)"
                }
            },
            "required": ["subcommand"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let subcommand = input["subcommand"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'subcommand' parameter".to_string()))?;

        let args: Vec<String> = input["args"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let workdir = input["workdir"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workdir.clone());

        let timeout_secs = input["timeout"].as_u64().unwrap_or(30);
        let timeout = Duration::from_secs(timeout_secs);

        if !workdir.exists() {
            return Err(ToolError::Execution(format!(
                "working directory does not exist: {}",
                workdir.display()
            )));
        }

        let full_args = {
            let mut full = vec![subcommand.to_string()];
            full.extend(args);
            full
        };

        tracing::info!(
            "Running git command in {}: git {:?}",
            workdir.display(),
            full_args
        );

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = Command::new("git");
            cmd.env_clear();
            if let Some(path) = std::env::var_os("PATH") {
                cmd.env("PATH", path);
            } else {
                cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
            }
            cmd.args(&full_args)
                .current_dir(&workdir)
                .output()
                .await
        })
        .await
        .map_err(|_| ToolError::Timeout(format!("git command timed out after {}s", timeout_secs)))?
        .map_err(|e| ToolError::Execution(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
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

        Ok(result)
    }
}
