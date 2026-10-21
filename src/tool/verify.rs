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
        if scope != "workspace" && scope != "package" {
            return Err(ToolError::Execution(
                "verify supports workspace or package scope; use bash for an explicitly scoped unsupported project command"
                    .into(),
            ));
        }
        if input.get("command").is_some() || input.get("path").is_some() {
            return Err(ToolError::Execution(
                "verify does not accept arbitrary commands or paths".into(),
            ));
        }
        let package = input.get("package").and_then(|value| value.as_str());
        if input.get("package").is_some() && package.is_none() {
            return Err(ToolError::Execution(
                "package must be one workspace package name".into(),
            ));
        }
        if scope == "package" && package.is_none() {
            return Err(ToolError::Execution(
                "scope=package requires a package name".into(),
            ));
        }
        if scope == "workspace" && package.is_some() {
            return Err(ToolError::Execution(
                "a package name requires scope=package".into(),
            ));
        }
        if let Some(package) = package {
            if package.is_empty()
                || !package
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(ToolError::Execution(
                    "package must be a workspace package name (letters, digits, '-' or '_')".into(),
                ));
            }
        }
        let requested_features: &[serde_json::Value] = input
            .get("features")
            .map(|value| {
                value.as_array().ok_or_else(|| {
                    ToolError::Execution("features must be an array of feature names".into())
                })
            })
            .transpose()?
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !requested_features.is_empty() && package.is_none() {
            return Err(ToolError::Execution(
                "explicit features require a package name".into(),
            ));
        }
        let compile_action = matches!(
            requested,
            VerifyAction::Auto
                | VerifyAction::Check
                | VerifyAction::Build
                | VerifyAction::Lint
                | VerifyAction::Typecheck
        );
        if (!requested_features.is_empty()
            || input.get("all_features").is_some_and(|value| value == true))
            && !compile_action
        {
            return Err(ToolError::Execution(
                "Cargo features can be selected only for check, build, lint, or typecheck".into(),
            ));
        }
        let mut features = Vec::with_capacity(requested_features.len());
        for feature in requested_features {
            let feature = feature.as_str().ok_or_else(|| {
                ToolError::Execution("features must contain only feature names".into())
            })?;
            if feature.is_empty()
                || !feature
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(ToolError::Execution(format!(
                    "invalid feature name '{feature}'; use letters, digits, '-' or '_'"
                )));
            }
            features.push(feature.to_owned());
        }
        let all_features = input
            .get("all_features")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if input.get("all_features").is_some()
            && input
                .get("all_features")
                .and_then(serde_json::Value::as_bool)
                .is_none()
        {
            return Err(ToolError::Execution(
                "all_features must be a boolean".into(),
            ));
        }
        if all_features && package.is_none() {
            return Err(ToolError::Execution(
                "all_features requires a package name; workspace-wide all-features verification is not supported".into(),
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
        let package_manifest = if let Some(package) = package {
            if let Some(manifest) = Self::workspace_package_manifest(&cwd, package) {
                Some(manifest)
            } else {
                return Err(ToolError::Execution(format!(
                    "package '{package}' is not a member of this Cargo workspace"
                )));
            }
        } else {
            None
        };
        if !features.is_empty() {
            let manifest_path = package_manifest.as_ref().expect("features require package");
            let manifest = std::fs::read_to_string(manifest_path).map_err(|_| {
                ToolError::Execution("cannot read selected package manifest".into())
            })?;
            let document = manifest.parse::<toml::Value>().map_err(|_| {
                ToolError::Execution("selected package manifest is invalid TOML".into())
            })?;
            let mut supported = document
                .get("features")
                .and_then(toml::Value::as_table)
                .map(|table| {
                    table
                        .keys()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default();
            for dependency_kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(dependencies) = document
                    .get(dependency_kind)
                    .and_then(toml::Value::as_table)
                {
                    for (name, dependency) in dependencies {
                        if dependency.get("optional").and_then(toml::Value::as_bool) == Some(true) {
                            supported.insert(name.clone());
                        }
                    }
                }
            }
            for feature in &features {
                if !supported.contains(feature) {
                    return Err(ToolError::Execution(format!(
                        "feature '{feature}' is not declared by package '{}'",
                        package.unwrap_or_default()
                    )));
                }
            }
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
            ],
            VerifyAction::FormatCheck => vec!["cargo".into(), "fmt".into()],
            VerifyAction::Auto => unreachable!("auto is resolved above"),
        };
        let mut argv = argv;
        if effective == VerifyAction::FormatCheck {
            if let Some(package) = package {
                argv.extend(["-p".into(), package.to_owned()]);
            } else {
                argv.push("--all".into());
            }
            argv.extend(["--".into(), "--check".into()]);
        } else if let Some(package) = package {
            argv.extend(["-p".into(), package.to_owned()]);
        }
        for feature in features {
            argv.extend(["--features".into(), feature]);
        }
        if all_features {
            argv.push("--all-features".into());
        }
        if effective == VerifyAction::Lint {
            argv.extend(["--".into(), "-D".into(), "warnings".into()]);
        }

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

    fn workspace_package_manifest(workspace_root: &Path, package: &str) -> Option<PathBuf> {
        let Ok(manifest) = std::fs::read_to_string(workspace_root.join("Cargo.toml")) else {
            return None;
        };
        let Ok(document) = manifest.parse::<toml::Value>() else {
            return None;
        };
        let members = document
            .get("workspace")
            .and_then(|value| value.get("members"))
            .and_then(toml::Value::as_array);
        let Some(members) = members else {
            return (document
                .get("package")
                .and_then(|value| value.get("name"))
                .and_then(toml::Value::as_str)
                == Some(package))
            .then(|| workspace_root.join("Cargo.toml"));
        };
        members
            .iter()
            .filter_map(toml::Value::as_str)
            .find_map(|member| {
                let member_manifest = workspace_root.join(member).join("Cargo.toml");
                let Ok(content) = std::fs::read_to_string(member_manifest) else {
                    return None;
                };
                let Ok(document) = content.parse::<toml::Value>() else {
                    return None;
                };
                (document
                    .get("package")
                    .and_then(|value| value.get("name"))
                    .and_then(toml::Value::as_str)
                    == Some(package))
                .then(|| workspace_root.join(member).join("Cargo.toml"))
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

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("verify")
    }

    fn description(&self) -> &str {
        "Run bounded, offline workspace verification for supported projects (check, build, lint, typecheck, or format_check). Returns structured status; it does not accept arbitrary commands or auto-fix files."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["auto", "check", "build", "lint", "typecheck", "format_check"]},
                "scope": {"type": "string", "enum": ["workspace", "package"], "description": "Workspace-wide or one named workspace package."},
                "package": {"type": "string", "description": "Workspace package name; required when scope is package or features are selected."},
                "features": {"type": "array", "items": {"type": "string"}, "description": "Explicit Cargo feature names. Requires package."},
                "all_features": {"type": "boolean", "description": "Explicitly enable all features for one selected package. May compile optional and real-server dependencies."},
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
        // M005 C005: refuse a process launch under the restricted shared
        // profile, before the tool runs and before any process exists.
        crate::tool::process_admission::admit_tool_process("verify", ctx.as_ref()).await?;
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
            "warnings": if resolved.argv.iter().any(|arg| arg == "--all-features") { vec!["all_features explicitly enables every package feature, including optional and real-server test support" ] } else { Vec::<&str>::new() },
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
        assert!(!resolved.argv.contains(&"--all-features".to_string()));
        assert!(resolved
            .argv
            .windows(2)
            .any(|pair| pair == ["-D", "warnings"]));
        assert!(!resolved.argv.iter().any(|arg| arg.contains(';')));
    }

    #[test]
    fn resolver_supports_bounded_package_and_feature_scope() {
        let root = tempdir().expect("tempdir");
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers=['crates/one', 'crates/two']\n",
        )
        .expect("workspace manifest");
        for (directory, name) in [("one", "one-crate"), ("two", "two-crate")] {
            let package = root.path().join("crates").join(directory);
            std::fs::create_dir_all(&package).expect("package dir");
            std::fs::write(
                package.join("Cargo.toml"),
                format!("[package]\nname='{name}'\n[features]\nfast=[]\n"),
            )
            .expect("package manifest");
        }
        let resolved = ResolvedVerification::resolve(
            &json!({"action":"lint", "scope":"package", "package":"one-crate", "features":["fast"]}),
            Some(root.path()),
            None,
        )
        .expect("package resolve");
        assert!(resolved
            .argv
            .windows(2)
            .any(|pair| pair == ["-p", "one-crate"]));
        assert!(resolved
            .argv
            .windows(2)
            .any(|pair| pair == ["--features", "fast"]));
        assert!(!resolved.argv.contains(&"--all-features".to_string()));
        let expansive = ResolvedVerification::resolve(
            &json!({"action":"check", "scope":"package", "package":"one-crate", "all_features":true}),
            Some(root.path()),
            None,
        )
        .expect("explicit package all-features opt-in");
        assert!(expansive.argv.contains(&"--all-features".to_string()));
        let format = ResolvedVerification::resolve(
            &json!({"action":"format_check", "scope":"package", "package":"one-crate"}),
            Some(root.path()),
            None,
        )
        .expect("package format resolve");
        assert_eq!(
            format.argv,
            ["cargo", "fmt", "-p", "one-crate", "--", "--check"]
        );
        assert!(ResolvedVerification::resolve(
            &json!({"action":"check", "scope":"workspace", "package":"one-crate"}),
            Some(root.path()),
            None
        )
        .is_err());
    }

    #[test]
    fn resolver_rejects_unknown_package_and_injected_features() {
        let root = tempdir().expect("tempdir");
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\n[features]\nfast=[]\n",
        )
        .expect("manifest");
        assert!(ResolvedVerification::resolve(
            &json!({"action":"check", "package":"missing"}),
            Some(root.path()),
            None
        )
        .is_err());
        assert!(ResolvedVerification::resolve(
            &json!({"action":"check", "package":"fixture", "features":["fast; touch pwned"]}),
            Some(root.path()),
            None
        )
        .is_err());
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
    fn resolver_uses_bound_workspace_root_instead_of_an_unrelated_context_cwd() {
        let bound_root = tempdir().expect("bound root");
        std::fs::write(
            bound_root.path().join("Cargo.toml"),
            "[package]\nname='bound'\n",
        )
        .expect("bound manifest");
        let context_root = tempdir().expect("context root");
        std::fs::write(
            context_root.path().join("Cargo.toml"),
            "[package]\nname='context'\n",
        )
        .expect("context manifest");

        let resolved = ResolvedVerification::resolve(
            &json!({"action":"check"}),
            Some(bound_root.path()),
            Some(context_root.path()),
        )
        .expect("bound workspace resolve");
        assert_eq!(resolved.cwd, bound_root.path());

        let context_only = ResolvedVerification::resolve(
            &json!({"action":"check"}),
            None,
            Some(context_root.path()),
        )
        .expect("context-root fallback resolve");
        assert_eq!(context_only.cwd, context_root.path());
        assert!(ResolvedVerification::resolve(&json!({"action":"check"}), None, None).is_err());
    }

    #[test]
    fn exit_status_and_report_are_bounded() {
        assert_eq!(parse_exit_status("output\n[exit code: 101]"), Some(101));
        assert_eq!(parse_exit_status("output"), None);
        assert!(truncate_report("ééé", 2).contains("truncated"));
    }
}
