use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::config::schema::CommandConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
    pub source: String,
}

pub fn find_command_files(base: &Path) -> Vec<Command> {
    let mut commands = Vec::new();

    for dir_name in ["command", "commands"] {
        let dir = base.join(dir_name);
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir).ok().into_iter().flatten() {
                let entry = entry
                    .ok()
                    .continue_if(|e| e.path().extension().is_none_or(|ext| ext == "md"));
                if let Some(entry) = entry {
                    if let Some(cmd) = load_command_from_file(&entry.path()) {
                        commands.push(cmd);
                    }
                }
            }
        }
    }

    commands
}

trait OptionExt<T> {
    fn continue_if<F: FnOnce(&T) -> bool>(self, f: F) -> Option<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn continue_if<F: FnOnce(&T) -> bool>(self, f: F) -> Option<T> {
        self.filter(f)
    }
}

pub fn load_command_from_file(path: &Path) -> Option<Command> {
    let content = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = parse_frontmatter(&content)?;

    let mut template = body.trim().to_string();
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut description = None;
    let mut agent = None;
    let mut model = None;
    let mut subtask = None;

    if let Ok(cfg) = serde_yaml::from_str::<CommandConfig>(&frontmatter) {
        template = cfg.template;
        description = cfg.description;
        agent = cfg.agent;
        model = cfg.model;
        subtask = cfg.subtask;
    } else if let Ok(cfg) = serde_yaml::from_str::<serde_yaml::Value>(&frontmatter) {
        if let Some(t) = cfg.get("template").and_then(|v| v.as_str()) {
            template = t.to_string();
        }
        description = cfg
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        agent = cfg.get("agent").and_then(|v| v.as_str()).map(String::from);
        model = cfg.get("model").and_then(|v| v.as_str()).map(String::from);
        subtask = cfg.get("subtask").and_then(|v| v.as_bool());
    }

    Some(Command {
        name,
        description,
        template,
        agent,
        model,
        subtask,
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
            subtask: cfg.subtask,
            source: "config".to_string(),
        })
        .collect()
}

pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in variables {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
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

    #[test]
    fn test_load_command_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\ndescription: A test command\nagent: build\ntemplate: \"Review the file: {file}\"\n---\nFallback body\n";
        std::fs::write(tmp.path().join("mycommand.md"), content).unwrap();
        let cmd = load_command_from_file(&tmp.path().join("mycommand.md")).unwrap();
        assert_eq!(cmd.name, "mycommand");
        assert_eq!(cmd.description, Some("A test command".to_string()));
        assert_eq!(cmd.agent, Some("build".to_string()));
        assert_eq!(cmd.template, "Review the file: {file}");
    }

    #[test]
    fn test_load_command_uses_filename() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("review.md"), "---\n---\nbody").unwrap();
        let cmd = load_command_from_file(&tmp.path().join("review.md")).unwrap();
        assert_eq!(cmd.name, "review");
    }
}
