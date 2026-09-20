//! Bounded semantic verification facade.
//!
//! `VerifyTool` owns no process execution. It resolves a small allow-listed
//! Rust verification vocabulary and delegates the generated command to the
//! already configured `BashTool`, preserving command-intent, scheduler,
//! sandbox, audit, and output ownership.

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::bash::BashTool;
use crate::tool::{Tool, ToolCategory};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_TIMEOUT_SECS: u64 = 900;
const DEFAULT_REPORT_BYTES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifyAction {
    Auto,
    Check,
    Build,
    Lint,
    Typecheck,
    FormatCheck,
}

impl VerifyAction {
    fn parse(value: Option<&str>) -> Result<Self, ToolError> {
        match value.unwrap_or("auto") {
            "auto" => Ok(Self::Auto),
            "check" => Ok(Self::Check),
            "build" => Ok(Self::Build),
            "lint" => Ok(Self::Lint),
            "typecheck" => Ok(Self::Typecheck),
            "format_check" => Ok(Self::FormatCheck),
            other => Err(ToolError::Execution(format!(
                "unknown verification action '{other}'; valid actions: auto, check, build, lint, typecheck, format_check"
            ))),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Check => "check",
            Self::Build => "build",
            Self::Lint => "lint",
            Self::Typecheck => "typecheck",
            Self::FormatCheck => "format_check",
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedVerification {
    requested: VerifyAction,
    effective: VerifyAction,
    argv: Vec<String>,
    cwd: PathBuf,
    timeout_secs: u64,
    max_report_bytes: usize,
}

impl ResolvedVerification {
    fn resolve(
        input: &serde_json::Value,
        configured_root: Option<&Path>,
        context_root: Option<&Path>,
    ) -> Result<Self, ToolError> {
        let requested = VerifyAction::parse(input.get("action").and_then(|v| v.as_str()))?;
        let scope = input
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("workspace");
        if scope != "workspace" {
            return Err(ToolError::Execution(
                "verify currently supports only workspace scope; use bash for an explicitly scoped unsupported project command"
                    .into(),
            ));
        }
        if input.get("command").is_some()
            || input.get("package").is_some()
            || input.get("path").is_some()
        {
            return Err(ToolError::Execution(
                "verify does not accept arbitrary commands, packages, or paths".into(),
            ));
        }

        let cwd = configured_root
            .or(context_root)
            .ok_or_else(|| {
                ToolError::Execution("verify requires an authoritative workspace root".into())
            })?
            .to_path_buf();
        if !cwd.join("Cargo.toml").is_file() {
            return Err(ToolError::Execution(
                "verify has no deterministic resolver for this project; use bash or the project-specific tool instead"
                    .into(),
            ));
        }

        let effective = if requested == VerifyAction::Auto {
            VerifyAction::Check
        } else {
            requested
        };
        let argv = match effective {
            VerifyAction::Check | VerifyAction::Typecheck => {
                vec!["cargo".into(), "check".into(), "--offline".into()]
            }
            VerifyAction::Build => vec!["cargo".into(), "build".into(), "--offline".into()],
            VerifyAction::Lint => vec![
                "cargo".into(),
                "clippy".into(),
                "--offline".into(),
                "--all-targets".into(),
                "--all-features".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ],
            VerifyAction::FormatCheck => vec![
                "cargo".into(),
                "fmt".into(),
                "--all".into(),
                "--".into(),
                "--check".into(),
            ],
            VerifyAction::Auto => unreachable!("auto is resolved above"),
        };

        let timeout_secs = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);
        let max_report_bytes = input
            .get("max_report_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_REPORT_BYTES as u64)
            .try_into()
            .map_err(|_| ToolError::Execution("max_report_bytes is too large".into()))?;
        if max_report_bytes == 0 {
            return Err(ToolError::Execution(
                "max_report_bytes must be positive".into(),
            ));
        }

        Ok(Self {
            requested,
            effective,
            argv,
            cwd,
            timeout_secs,
            max_report_bytes,
        })
    }

    fn command(&self) -> String {
        self.argv
            .iter()
            .map(|arg| {
                if arg.chars().any(char::is_whitespace) {
                    format!("'{}'", arg.replace('\'', "'\\''"))
                } else {
                    arg.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub struct VerifyTool {
    bash: BashTool,
}

impl VerifyTool {
    pub fn new(bash: BashTool) -> Self {
        Self { bash }
    }
}

#[async_trait]
impl Tool for VerifyTool {
    fn name(&self) -> &str {
        "verify"
    }

    fn description(&self) -> &str {
        "Run bounded, offline workspace verification for supported projects (check, build, lint, typecheck, or format_check). Returns structured status; it does not accept arbitrary commands or auto-fix files."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["auto", "check", "build", "lint", "typecheck", "format_check"]},
                "scope": {"type": "string", "enum": ["workspace"], "description": "The authoritative workspace root."},
                "timeout": {"type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_SECS, "description": "Wall-clock timeout in seconds."},
                "max_report_bytes": {"type": "integer", "minimum": 1, "description": "Maximum diagnostic summary bytes."}
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    fn defer_loading(&self) -> bool {
        false
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        Ok(self.execute_structured(input, None).await?.output)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let started = Instant::now();
        let resolved = ResolvedVerification::resolve(
            &input,
            self.bash.workspace_root.as_deref(),
            ctx.as_ref().map(|context| context.cwd.as_path()),
        )?;
        let command = resolved.command();
        let child = self
            .bash
            .execute_structured(
                json!({
                    "command": command,
                    "timeout": resolved.timeout_secs,
                    "workdir": resolved.cwd,
                }),
                ctx,
            )
            .await?;
        let status = parse_exit_status(&child.output);
        let passed = status == Some(0);
        let summary = truncate_report(&child.output, resolved.max_report_bytes);
        let value = json!({
            "requested_action": resolved.requested.label(),
            "effective_action": resolved.effective.label(),
            "project": "rust/cargo",
            "argv": resolved.argv,
            "cwd": resolved.cwd,
            "status": match status { Some(0) => "passed", Some(_) => "failed", None => "unknown" },
            "exit_code": status,
            "summary": summary,
            "truncated": child.output.len() > resolved.max_report_bytes,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "next_action": if passed { "continue" } else { "inspect diagnostics and revise" }
        });
        let output = serde_json::to_string(&value).map_err(|error| {
            ToolError::Execution(format!("verify result encoding failed: {error}"))
        })?;
        let provenance = child.provenance.or_else(|| {
            Some(ToolProvenance {
                backend: "native".into(),
                implementation: "codegg/verify".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                truncated: child.output.len() > resolved.max_report_bytes,
                trust: ToolTrust::LocalUntrusted,
            })
        });
        Ok(StructuredToolResult::with_value(
            output.clone(),
            value,
            passed,
            provenance,
        ))
    }
}

fn parse_exit_status(output: &str) -> Option<i32> {
    let marker = "[exit code: ";
    let start = output.rfind(marker)? + marker.len();
    let end = output[start..].find(']')? + start;
    output[start..end].trim().parse().ok()
}

fn truncate_report(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[verification report truncated]", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolver_is_allowlisted_and_offline() {
        let root = tempdir().expect("tempdir");
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\n",
        )
        .expect("manifest");
        let input = json!({"action": "lint"});
        let resolved =
            ResolvedVerification::resolve(&input, Some(root.path()), None).expect("resolve");
        assert_eq!(resolved.argv[0], "cargo");
        assert!(resolved.argv.contains(&"--offline".to_string()));
        assert!(!resolved.argv.iter().any(|arg| arg.contains(';')));
    }

    #[test]
    fn resolver_rejects_arbitrary_commands_and_unknown_projects() {
        let root = tempdir().expect("tempdir");
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\n",
        )
        .expect("manifest");
        let arbitrary = ResolvedVerification::resolve(
            &json!({"action": "check", "command": "cargo check; touch pwned"}),
            Some(root.path()),
            None,
        );
        assert!(arbitrary.is_err());

        let other = tempdir().expect("other tempdir");
        let unsupported =
            ResolvedVerification::resolve(&json!({"action": "check"}), Some(other.path()), None);
        assert!(unsupported.is_err());
    }

    #[test]
    fn exit_status_and_report_are_bounded() {
        assert_eq!(parse_exit_status("output\n[exit code: 101]"), Some(101));
        assert_eq!(parse_exit_status("output"), None);
        assert!(truncate_report("ééé", 2).contains("truncated"));
    }
}
