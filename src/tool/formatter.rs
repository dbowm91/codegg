use std::collections::HashMap;
use std::path::Path;

use crate::config::schema::{FormatterConfig, FormatterRule};
use crate::error::{AppError, ToolError};

pub struct Formatter {
    rules: HashMap<String, FormatterRule>,
}

impl Formatter {
    pub fn new(config: Option<&FormatterConfig>) -> Self {
        let rules = match config {
            Some(FormatterConfig::Rules(rules)) => rules.clone(),
            _ => HashMap::new(),
        };
        Self { rules }
    }

    pub fn format_file(&self, path: &Path) -> Result<(), AppError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| AppError::Tool(ToolError::Format("no file extension".to_string())))?;

        let rule = self.rules.get(ext).ok_or_else(|| {
            AppError::Tool(ToolError::Format(format!(
                "no formatter rule for extension: {ext}"
            )))
        })?;

        if rule.disabled == Some(true) {
            return Ok(());
        }

        let command = rule
            .command
            .as_ref()
            .ok_or_else(|| AppError::Tool(ToolError::Format("no formatter command".to_string())))?;

        if command.is_empty() {
            return Err(AppError::Tool(ToolError::Format(
                "formatter command is empty".to_string(),
            )));
        }

        let mut cmd = std::process::Command::new(&command[0]);
        cmd.args(&command[1..])
            .arg(path)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");

        if let Some(env) = &rule.environment {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        let output = cmd.output().map_err(|e| {
            AppError::Tool(ToolError::Format(format!("failed to run formatter: {e}")))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Tool(ToolError::Format(format!(
                "formatter failed: {stderr}"
            ))));
        }

        Ok(())
    }

    pub fn has_rule(&self, ext: &str) -> bool {
        self.rules
            .get(ext)
            .map(|r| r.disabled != Some(true))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formatter_new_empty_config() {
        let formatter = Formatter::new(None);
        assert!(!formatter.has_rule("rs"));
    }

    #[test]
    fn test_formatter_has_rule() {
        let mut rules = HashMap::new();
        rules.insert(
            "rs".to_string(),
            FormatterRule {
                disabled: Some(false),
                command: Some(vec!["rustfmt".to_string()]),
                environment: None,
                extensions: Some(vec!["rs".to_string()]),
            },
        );
        let formatter = Formatter::new(Some(&FormatterConfig::Rules(rules)));
        assert!(formatter.has_rule("rs"));
        assert!(!formatter.has_rule("js"));
    }

    #[test]
    fn test_formatter_disabled_rule() {
        let mut rules = HashMap::new();
        rules.insert(
            "rs".to_string(),
            FormatterRule {
                disabled: Some(true),
                command: Some(vec!["rustfmt".to_string()]),
                environment: None,
                extensions: Some(vec!["rs".to_string()]),
            },
        );
        let formatter = Formatter::new(Some(&FormatterConfig::Rules(rules)));
        assert!(!formatter.has_rule("rs"));
    }

    #[test]
    fn test_formatter_format_file_no_rule() {
        let formatter = Formatter::new(None);
        let result = formatter.format_file(Path::new("test.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn test_formatter_format_file_disabled() {
        let mut rules = HashMap::new();
        rules.insert(
            "rs".to_string(),
            FormatterRule {
                disabled: Some(true),
                command: Some(vec!["rustfmt".to_string()]),
                environment: None,
                extensions: Some(vec!["rs".to_string()]),
            },
        );
        let formatter = Formatter::new(Some(&FormatterConfig::Rules(rules)));
        let result = formatter.format_file(Path::new("test.rs"));
        assert!(result.is_ok());
    }
}
