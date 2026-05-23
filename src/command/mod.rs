use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use tracing::{debug, warn};
use crate::config::schema::CommandConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    #[deprecated(since = "2026-05-22", note = "subtask field is not yet implemented")]
    pub subtask: Option<bool>,
    pub source: String,
}

pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base).into_iter().map(|r| r.unwrap_or_else(|e| {
        warn!("Failed to load command: {}", e);
        panic!("expected")
    })).collect()
}

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
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read file: {}", e))?;
    parse_command_content(path, &content)
}

fn parse_command_content(path: &Path, content: &str) -> Result<Command, String> {
    let (frontmatter, body) = parse_frontmatter(content)
        .ok_or_else(|| "missing frontmatter".to_string())?;

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let (description, template, agent, model, _subtask) =
        if let Ok(cfg) = serde_yaml::from_str::<CommandConfig>(&frontmatter) {
            (
                cfg.description,
                if cfg.template.is_empty() {
                    None
                } else {
                    Some(cfg.template)
                },
                cfg.agent,
                cfg.model,
                cfg.subtask,
            )
        } else if let Ok(cfg) = serde_yaml::from_str::<serde_yaml::Value>(&frontmatter) {
            (
                cfg.get("description").and_then(|v| v.as_str()).map(String::from),
                cfg.get("template")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                cfg.get("agent").and_then(|v| v.as_str()).map(String::from),
                cfg.get("model").and_then(|v| v.as_str()).map(String::from),
                cfg.get("subtask").and_then(|v| v.as_bool()),
            )
        } else {
            return Err("failed to parse frontmatter".to_string());
        };

    let template = template.unwrap_or_else(|| body.trim().to_string());

    #[allow(deprecated)]
    Ok(Command {
        name,
        description,
        template,
        agent,
        model,
        subtask: _subtask,
        source: path.to_string_lossy().to_string(),
    })
}

pub fn resolve_commands_from_config(
    config_commands: &HashMap<String, CommandConfig>,
) -> Vec<Command> {
    config_commands
        .iter()
        .map(|(name, cfg)| Command {
            name: name.clone(),
            description: cfg.description.clone(),
            template: cfg.template.clone(),
            agent: cfg.agent.clone(),
            model: cfg.model.clone(),
            #[allow(deprecated)]
            subtask: cfg.subtask,
            source: "config".to_string(),
        })
        .collect()
}

pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    let mut sorted_keys: Vec<_> = variables.keys().collect();
    sorted_keys.sort();
    for key in sorted_keys {
        let value = variables.get(key).unwrap();
        result = result.replace(&format!("{{{{{key}}}}}", ), value);
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
        tokio::fs::write(tmp.path().join("mycommand.md"), content).await.unwrap();
        let cmd = load_command_from_file(&tmp.path().join("mycommand.md")).await.unwrap();
        assert_eq!(cmd.name, "mycommand");
        assert_eq!(cmd.description, Some("A test command".to_string()));
        assert_eq!(cmd.agent, Some("build".to_string()));
        assert_eq!(cmd.template, "Review the file: {file}");
    }

    #[tokio::test]
    async fn test_load_command_uses_filename() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("review.md"), "---\n---\nbody").await.unwrap();
        let cmd = load_command_from_file(&tmp.path().join("review.md")).await.unwrap();
        assert_eq!(cmd.name, "review");
    }

    #[tokio::test]
    async fn test_load_command_fallback_to_body() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("testcmd.md"), "---\ndescription: just desc\n---\nBody template here").await.unwrap();
        let cmd = load_command_from_file(&tmp.path().join("testcmd.md")).await.unwrap();
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
        tokio::fs::write(tmp.path().join("nocfm.md"), "no frontmatter").await.unwrap();
        assert!(load_command_from_file(&tmp.path().join("nocfm.md")).await.is_err());
    }
}
