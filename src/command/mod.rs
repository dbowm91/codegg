use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::config::schema::{CommandConfig, CommandRuntimeKind, CommandStdinMode, CommandStdoutMode};
use tracing::{debug, warn};

/// Process execution specification for a command with `runtime: process`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessCommandSpec {
    pub command: String,
    pub args: Vec<String>,
    pub stdin: CommandStdinMode,
    pub stdout: CommandStdoutMode,
    pub timeout_ms: u64,
    pub cwd: Option<String>,
    pub env: Vec<String>,
    pub output: Vec<String>,
}

impl Default for ProcessCommandSpec {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            stdin: CommandStdinMode::None,
            stdout: CommandStdoutMode::Auto,
            timeout_ms: 5000,
            cwd: None,
            env: Vec::new(),
            output: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    #[deprecated(since = "0.1.0", note = "subtask field is not yet implemented")]
    pub subtask: Option<bool>,
    pub source: String,
    /// Process execution spec (only when `runtime: process`).
    pub process: Option<ProcessCommandSpec>,
}

pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base)
        .into_iter()
        .filter_map(|r| r.ok())
        .collect()
}

#[allow(clippy::incompatible_msrv)]
pub fn find_command_files_sync(base: &Path) -> Vec<Result<Command, String>> {
    let mut commands = Vec::new();

    for dir_name in ["command", "commands"] {
        let dir = base.join(dir_name);
        if dir.is_dir() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) => {
                    warn!("Failed to read directory entry in {:?}: {}", dir, e);
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext == "md") {
                    match load_command_from_file_sync(&path) {
                        Ok(cmd) => {
                            if let Err(e) = validate_command_name(&cmd.name) {
                                warn!("Invalid command name {:?} in {:?}: {}", cmd.name, path, e);
                                continue;
                            }
                            debug!("Loaded command {:?} from {:?}", cmd.name, path);
                            commands.push(Ok(cmd));
                        }
                        Err(e) => {
                            warn!("Failed to load command from {:?}: {}", path, e);
                            commands.push(Err(e));
                        }
                    }
                }
            }
        }
    }

    commands
}

fn validate_command_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("empty name");
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err("name contains whitespace");
    }
    if name.starts_with('/') {
        return Err("name starts with /");
    }
    Ok(())
}

pub async fn load_command_from_file(path: &Path) -> Result<Command, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("failed to read file: {}", e))?;
    parse_command_content(path, &content)
}

pub fn load_command_from_file_sync(path: &Path) -> Result<Command, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read file: {}", e))?;
    parse_command_content(path, &content)
}

fn parse_command_content(path: &Path, content: &str) -> Result<Command, String> {
    let (frontmatter, body) =
        parse_frontmatter(content).ok_or_else(|| "missing frontmatter".to_string())?;

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if let Ok(cfg) = serde_yaml::from_str::<CommandConfig>(&frontmatter) {
        let is_process = cfg.runtime == Some(CommandRuntimeKind::Process);
        let process = if is_process {
            let command = cfg.command.ok_or_else(|| {
                format!("command '{name}' has runtime 'process' but no 'command' field")
            })?;
            Some(ProcessCommandSpec {
                command,
                args: cfg.args.unwrap_or_default(),
                stdin: cfg.stdin.unwrap_or_default(),
                stdout: cfg.stdout.unwrap_or_default(),
                timeout_ms: cfg.timeout_ms.unwrap_or(5000),
                cwd: cfg.cwd,
                env: cfg.env.unwrap_or_default(),
                output: cfg.output.unwrap_or_default(),
            })
        } else {
            None
        };

        let template = if cfg.template.is_empty() {
            if is_process {
                String::new()
            } else {
                body.trim().to_string()
            }
        } else {
            cfg.template
        };

        #[allow(deprecated)]
        return Ok(Command {
            name,
            description: cfg.description,
            template,
            agent: cfg.agent,
            model: cfg.model,
            subtask: cfg.subtask,
            source: path.to_string_lossy().to_string(),
            process,
        });
    }

    if let Ok(cfg) = serde_yaml::from_str::<serde_yaml::Value>(&frontmatter) {
        let runtime = cfg
            .get("runtime")
            .and_then(|v| v.as_str())
            .unwrap_or("template");
        let is_process = runtime == "process";

        let process = if is_process {
            let command = cfg
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    format!("command '{name}' has runtime 'process' but no 'command' field")
                })?
                .to_string();
            Some(ProcessCommandSpec {
                command,
                args: cfg
                    .get("args")
                    .and_then(|v| v.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                stdin: cfg
                    .get("stdin")
                    .and_then(|v| v.as_str())
                    .map(|s| match s {
                        "json" => CommandStdinMode::Json,
                        _ => CommandStdinMode::None,
                    })
                    .unwrap_or_default(),
                stdout: cfg
                    .get("stdout")
                    .and_then(|v| v.as_str())
                    .and_then(|s| match s {
                        "text" => Some(CommandStdoutMode::Text),
                        "json" => Some(CommandStdoutMode::Json),
                        "auto" => Some(CommandStdoutMode::Auto),
                        _ => None,
                    })
                    .unwrap_or_default(),
                timeout_ms: cfg
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5000),
                cwd: cfg.get("cwd").and_then(|v| v.as_str()).map(String::from),
                env: cfg
                    .get("env")
                    .and_then(|v| v.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                output: cfg
                    .get("output")
                    .and_then(|v| v.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        } else {
            None
        };

        let template = cfg
            .get("template")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| {
                if is_process {
                    String::new()
                } else {
                    body.trim().to_string()
                }
            });

        #[allow(deprecated)]
        return Ok(Command {
            name,
            description: cfg
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            template,
            agent: cfg.get("agent").and_then(|v| v.as_str()).map(String::from),
            model: cfg.get("model").and_then(|v| v.as_str()).map(String::from),
            subtask: cfg.get("subtask").and_then(|v| v.as_bool()),
            source: path.to_string_lossy().to_string(),
            process,
        });
    }

    Err("failed to parse frontmatter".to_string())
}

pub fn resolve_commands_from_config(
    config_commands: &HashMap<String, CommandConfig>,
) -> Vec<Command> {
    config_commands
        .iter()
        .map(|(name, cfg)| {
            let is_process = cfg.runtime == Some(CommandRuntimeKind::Process);
            let process = if is_process {
                cfg.command.as_ref().map(|cmd| ProcessCommandSpec {
                    command: cmd.clone(),
                    args: cfg.args.clone().unwrap_or_default(),
                    stdin: cfg.stdin.unwrap_or_default(),
                    stdout: cfg.stdout.unwrap_or_default(),
                    timeout_ms: cfg.timeout_ms.unwrap_or(5000),
                    cwd: cfg.cwd.clone(),
                    env: cfg.env.clone().unwrap_or_default(),
                    output: cfg.output.clone().unwrap_or_default(),
                })
            } else {
                None
            };
            #[allow(deprecated)]
            Command {
                name: name.clone(),
                description: cfg.description.clone(),
                template: cfg.template.clone(),
                agent: cfg.agent.clone(),
                model: cfg.model.clone(),
                subtask: cfg.subtask,
                source: "config".to_string(),
                process,
            }
        })
        .collect()
}

pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    let mut sorted_keys: Vec<_> = variables.keys().collect();
    sorted_keys.sort();
    for key in sorted_keys {
        let value = variables.get(key).unwrap();
        result = result.replace(&format!("{{{{{key}}}}}",), value);
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}

fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = rest[..end].trim().to_string();
    let body = rest[end + 3..].trim().to_string();

    Some((frontmatter, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_command_template_simple() {
        let template = "Hello {name}!";
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());
        assert_eq!(execute_command_template(template, &vars), "Hello World!");
    }

    #[test]
    fn test_execute_command_template_double_braces() {
        let template = "Hello {{name}}!";
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());
        assert_eq!(execute_command_template(template, &vars), "Hello World!");
    }

    #[test]
    fn test_execute_command_template_missing_var() {
        let template = "Hello {name}!";
        let vars = HashMap::new();
        assert_eq!(execute_command_template(template, &vars), "Hello {name}!");
    }

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = "---\nname: test\n---\nbody content";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm, "name: test");
        assert_eq!(body, "body content");
    }

    #[test]
    fn test_parse_frontmatter_missing() {
        assert!(parse_frontmatter("no frontmatter").is_none());
    }

    #[tokio::test]
    async fn test_load_command_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: A test command\nagent: build\ntemplate: \"Review the file: {file}\"\n---\nFallback body\n";
        tokio::fs::write(tmp.path().join("mycommand.md"), content)
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("mycommand.md"))
            .await
            .unwrap();
        assert_eq!(cmd.name, "mycommand");
        assert_eq!(cmd.description, Some("A test command".to_string()));
        assert_eq!(cmd.agent, Some("build".to_string()));
        assert_eq!(cmd.template, "Review the file: {file}");
        assert!(cmd.process.is_none());
    }

    #[tokio::test]
    async fn test_load_command_uses_filename() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("review.md"), "---\n---\nbody")
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("review.md"))
            .await
            .unwrap();
        assert_eq!(cmd.name, "review");
    }

    #[tokio::test]
    async fn test_load_command_fallback_to_body() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(
            tmp.path().join("testcmd.md"),
            "---\ndescription: just desc\n---\nBody template here",
        )
        .await
        .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("testcmd.md"))
            .await
            .unwrap();
        assert_eq!(cmd.template, "Body template here");
    }

    #[test]
    fn test_validate_command_name() {
        assert!(validate_command_name("valid").is_ok());
        assert!(validate_command_name("").is_err());
        assert!(validate_command_name("bad name").is_err());
        assert!(validate_command_name("/leading").is_err());
    }

    #[tokio::test]
    async fn test_load_command_missing_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("nocfm.md"), "no frontmatter")
            .await
            .unwrap();
        assert!(load_command_from_file(&tmp.path().join("nocfm.md"))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_process_command_yaml_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: Show quota\nruntime: process\ncommand: python3\nargs: [\"scripts/quota.py\"]\nstdout: text\ntimeout_ms: 5000\n---\n";
        tokio::fs::write(tmp.path().join("quota.md"), content)
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("quota.md"))
            .await
            .unwrap();
        assert_eq!(cmd.name, "quota");
        assert_eq!(cmd.description, Some("Show quota".to_string()));
        assert!(cmd.template.is_empty());
        let proc = cmd.process.expect("should have process spec");
        assert_eq!(proc.command, "python3");
        assert_eq!(proc.args, vec!["scripts/quota.py"]);
        assert_eq!(proc.stdout, CommandStdoutMode::Text);
        assert_eq!(proc.timeout_ms, 5000);
    }

    #[tokio::test]
    async fn test_process_command_json_output() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: Show quota as dialog\nruntime: process\ncommand: python3\nargs: [\"scripts/quota.py\", \"--json\"]\nstdin: json\nstdout: json\ntimeout_ms: 5000\noutput: [\"chat\", \"dialog\"]\n---\n";
        tokio::fs::write(tmp.path().join("quota_json.md"), content)
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("quota_json.md"))
            .await
            .unwrap();
        let proc = cmd.process.expect("should have process spec");
        assert_eq!(proc.stdin, CommandStdinMode::Json);
        assert_eq!(proc.stdout, CommandStdoutMode::Json);
        assert_eq!(proc.output, vec!["chat", "dialog"]);
    }

    #[tokio::test]
    async fn test_process_command_auto_stdout_default() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: Auto detect\nruntime: process\ncommand: echo\n---\n";
        tokio::fs::write(tmp.path().join("auto.md"), content)
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("auto.md"))
            .await
            .unwrap();
        let proc = cmd.process.expect("should have process spec");
        assert_eq!(proc.stdout, CommandStdoutMode::Auto);
        assert_eq!(proc.timeout_ms, 5000); // default
        assert_eq!(proc.stdin, CommandStdinMode::None); // default
    }

    #[tokio::test]
    async fn test_process_command_without_command_field_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: Bad command\nruntime: process\n---\n";
        tokio::fs::write(tmp.path().join("bad.md"), content)
            .await
            .unwrap();
        let result = load_command_from_file(&tmp.path().join("bad.md")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no 'command' field"));
    }

    #[tokio::test]
    async fn test_process_command_env_and_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: With env\ncwd: /tmp\nenv: [\"FOO=bar\", \"BAZ=qux\"]\nruntime: process\ncommand: env\n---\n";
        tokio::fs::write(tmp.path().join("envcmd.md"), content)
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("envcmd.md"))
            .await
            .unwrap();
        let proc = cmd.process.expect("should have process spec");
        assert_eq!(proc.cwd, Some("/tmp".to_string()));
        assert_eq!(proc.env, vec!["FOO=bar", "BAZ=qux"]);
    }

    #[tokio::test]
    async fn test_template_command_still_works() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: A template\ntemplate: \"Review {args}\"\n---\n";
        tokio::fs::write(tmp.path().join("review.md"), content)
            .await
            .unwrap();
        let cmd = load_command_from_file(&tmp.path().join("review.md"))
            .await
            .unwrap();
        assert!(cmd.process.is_none());
        assert_eq!(cmd.template, "Review {args}");
    }

    #[test]
    fn test_resolve_commands_process_from_config() {
        use crate::config::schema::CommandConfig;
        let mut commands = HashMap::new();
        commands.insert(
            "quota".to_string(),
            CommandConfig {
                template: String::new(),
                description: Some("Show quota".to_string()),
                runtime: Some(CommandRuntimeKind::Process),
                command: Some("python3".to_string()),
                args: Some(vec!["scripts/quota.py".to_string()]),
                stdout: Some(CommandStdoutMode::Text),
                timeout_ms: Some(3000),
                ..Default::default()
            },
        );
        let resolved = resolve_commands_from_config(&commands);
        assert_eq!(resolved.len(), 1);
        let cmd = &resolved[0];
        assert_eq!(cmd.name, "quota");
        let proc = cmd.process.as_ref().expect("should have process spec");
        assert_eq!(proc.command, "python3");
        assert_eq!(proc.timeout_ms, 3000);
    }
}
