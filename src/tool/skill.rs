use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize)]
struct SkillInput {
    name: String,
}

#[derive(Default)]
pub struct SkillTool {
    snapshot: Option<Arc<crate::agent::asset_snapshot::ProjectAssetSnapshot>>,
    asset_pin: Option<Arc<Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
}

impl SkillTool {
    pub fn with_snapshot(
        snapshot: Option<Arc<crate::agent::asset_snapshot::ProjectAssetSnapshot>>,
        asset_pin: Option<Arc<Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
    ) -> Self {
        Self {
            snapshot,
            asset_pin,
        }
    }
}

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

        if let Some(snapshot) = self.snapshot.as_ref() {
            if let Some(pin) = self.asset_pin.as_ref() {
                pin.lock()
                    .map_err(|_| ToolError::Execution("asset pin lock poisoned".to_string()))?
                    .record_skill_activation(&parsed.name)
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
            }
            let skill = snapshot.skills.get(&parsed.name).ok_or_else(|| {
                ToolError::Execution(format!("skill '{}' not found", parsed.name))
            })?;
            return render_skill(skill);
        }

        // Build an explicit context. CLI bootstrap reads cwd exactly once
        // at this boundary; the registry no longer reads process-global
        // state.
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let ctx = crate::agent::asset_context::AssetContextBuilder::new()
            .with_synthetic_project_id(crate::agent::asset_context::ProjectId::new())
            .with_workspace_root(cwd)
            .build()
            .map_err(|e| ToolError::Execution(format!("invalid skill context: {e}")))?;

        let asset_config = crate::skills::AssetDiscoveryConfig::default();
        let global_roots: Vec<std::path::PathBuf> = ctx
            .global_roots()
            .iter()
            .chain(crate::agent::asset_context::default_global_skills_root().as_ref())
            .cloned()
            .collect();
        let registry =
            crate::skills::AssetRegistry::build(&asset_config, ctx.workspace_root(), &global_roots);

        let skill = registry
            .get(&parsed.name)
            .ok_or_else(|| ToolError::Execution(format!("skill '{}' not found", parsed.name)))?;

        render_skill(skill)
    }
}

fn render_skill(skill: &crate::skills::EffectiveSkill) -> Result<String, ToolError> {
    // Discovery already inventories bounded resource metadata. Reuse it
    // instead of scanning the package again; resource bodies are loaded
    // only through an explicit ResourceHandle read.
    let resources: Vec<&str> = skill
        .resources
        .iter()
        .map(|resource| resource.name.as_str())
        .collect();

    let result = serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "body": skill.body,
        "resources": resources,
    });

    serde_json::to_string_pretty(&result)
        .map_err(|e| ToolError::Execution(format!("failed to serialize: {e}")))
}
