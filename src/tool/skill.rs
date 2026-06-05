use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SkillInput {
    name: String,
}

pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Load a skill (SKILL.md) by name into context. Returns the skill content and list of resource files."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the skill to load"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: SkillInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid skill input: {e}")))?;

        let project_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut loaded = crate::skills::SkillIndex::new();
        if let Err(e) = loaded.load(&project_dir).await {
            return Err(ToolError::Execution(format!("failed to load skills: {e}")));
        }

        let skill = loaded
            .get(&parsed.name)
            .ok_or_else(|| ToolError::Execution(format!("skill '{}' not found", parsed.name)))?;

        let resources = list_skill_resources(&skill.source).await;

        let result = serde_json::json!({
            "name": skill.name,
            "description": skill.description,
            "body": skill.body,
            "resources": resources,
        });

        serde_json::to_string_pretty(&result)
            .map_err(|e| ToolError::Execution(format!("failed to serialize: {e}")))
    }
}

async fn list_skill_resources(skill_path: &std::path::Path) -> Vec<String> {
    let dir = if skill_path.is_dir() {
        skill_path.to_path_buf()
    } else {
        skill_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
    };

    if !tokio::fs::metadata(&dir)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Vec::new();
    }

    let mut resources = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.file_name() != Some(std::ffi::OsStr::new("SKILL.md")) {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    resources.push(name.to_string());
                }
            }
        }
    }

    resources
}
