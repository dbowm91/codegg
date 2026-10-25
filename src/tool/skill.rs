use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize)]
struct SkillInput {
    name: String,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Default)]
pub struct SkillTool {
    snapshot: Option<Arc<crate::agent::asset_snapshot::ProjectAssetSnapshot>>,
    asset_pin: Option<Arc<Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
    /// Root that local skill discovery is bounded to.
    ///
    /// `None` means "no session root was resolved", which is a refusal, not a
    /// cue to read the daemon's process directory: skill lookup walks
    /// `<workspace>/.opencode/skills`, so an inherited process directory would
    /// silently load another tree's skills.
    workspace_root: Option<std::path::PathBuf>,
}

impl SkillTool {
    pub fn with_snapshot(
        snapshot: Option<Arc<crate::agent::asset_snapshot::ProjectAssetSnapshot>>,
        asset_pin: Option<Arc<Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
    ) -> Self {
        Self {
            snapshot,
            asset_pin,
            workspace_root: None,
        }
    }

    /// Bind local skill discovery to the session's workspace root.
    pub fn with_workspace_root(mut self, root: Option<std::path::PathBuf>) -> Self {
        self.workspace_root = root;
        self
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Load a skill (SKILL.md) by name into context. Returns the skill content and list of resource files. \
         Skills are discovered by exact name from the pinned asset snapshot or the session workspace; \
         an unknown name returns did-you-mean suggestions (top matches with one-line descriptions). \
         Find the full first-party list in the AGENTS.md Skills index or .opencode/skills/."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the skill to load"
                },
                "query": {
                    "type": "string",
                    "description": "Optional search text used only to rank did-you-mean suggestions when 'name' is not found"
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
                let record = pin
                    .lock()
                    .map_err(|_| ToolError::Execution("asset pin lock poisoned".to_string()))?
                    .record_skill_activation(&parsed.name)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                if let Err(msg) = record {
                    if msg.contains("not present in the pinned snapshot") {
                        return Err(skill_not_found(
                            &parsed.name,
                            parsed.query.as_deref(),
                            &snapshot.skills,
                        ));
                    }
                    return Err(ToolError::Execution(msg));
                }
            }
            let skill = snapshot.skills.get(&parsed.name).ok_or_else(|| {
                skill_not_found(&parsed.name, parsed.query.as_deref(), &snapshot.skills)
            })?;
            return render_skill(skill);
        }

        // Build an explicit context from propagated workspace state only.
        let cwd = self
            .workspace_root
            .clone()
            .ok_or_else(|| ToolError::MissingWorkspaceRoot {
                context: "skill tool was constructed without a workspace root".to_string(),
            })?;
        let ctx = crate::agent::asset_context::AssetContextBuilder::new()
            .with_synthetic_project_id(crate::agent::asset_context::ProjectId::new())
            .with_workspace_root(cwd)
            .build()
            .map_err(|e| ToolError::Execution(format!("invalid skill context: {e}")))?;

        let app_config = crate::config::schema::Config::load_or_default();
        let asset_config = crate::agent::asset_context::asset_discovery_config_from(&app_config);
        let global_roots: Vec<std::path::PathBuf> = ctx
            .global_roots()
            .iter()
            .chain(crate::agent::asset_context::default_global_discovery_root().as_ref())
            .cloned()
            .collect();
        let registry =
            crate::skills::AssetRegistry::build(&asset_config, ctx.workspace_root(), &global_roots);

        let skill = registry
            .get(&parsed.name)
            .ok_or_else(|| skill_not_found(&parsed.name, parsed.query.as_deref(), &registry))?;

        render_skill(skill)
    }
}

const MAX_SKILL_SUGGESTIONS: usize = 5;

fn skill_not_found(
    name: &str,
    query: Option<&str>,
    registry: &crate::skills::AssetRegistry,
) -> ToolError {
    let hint = query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(name);
    let mut matches = registry.find_matching(hint);
    matches.truncate(MAX_SKILL_SUGGESTIONS);
    let suggestions: Vec<&crate::skills::EffectiveSkill> = if matches.is_empty() {
        registry.list().iter().take(MAX_SKILL_SUGGESTIONS).collect()
    } else {
        matches
    };
    if suggestions.is_empty() {
        return ToolError::Execution(format!(
            "skill '{name}' not found (no skills registered; see the AGENTS.md Skills index for first-party skills)"
        ));
    }
    let mut msg = format!("skill '{name}' not found. Did you mean:");
    for skill in suggestions {
        let description = skill.description.replace('\n', " ");
        msg.push_str(&format!("\n- {}: {}", skill.name, description));
    }
    msg.push_str("\nSee the AGENTS.md Skills index for first-party skills.");
    ToolError::Execution(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use std::fs;

    fn fixture_registry(
        dir: &tempfile::TempDir,
        skills: &[(&str, &str)],
    ) -> crate::skills::AssetRegistry {
        for (name, description) in skills {
            let skill_dir = dir.path().join(".codegg").join("skills").join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\nBody"),
            )
            .unwrap();
        }
        let config = crate::skills::AssetDiscoveryConfig::default();
        crate::skills::AssetRegistry::build_with_home(&config, dir.path(), &[], None)
    }

    #[test]
    fn unknown_skill_error_suggests_similar_names() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = fixture_registry(
            &dir,
            &[
                ("testing-ci", "Run the test suite"),
                ("tool-execution", "Run tools"),
            ],
        );
        let err = skill_not_found("testing", None, &registry);
        let msg = err.to_string();
        assert!(msg.contains("did you mean") || msg.contains("Did you mean"));
        assert!(msg.contains("testing-ci"));
        assert!(msg.contains("AGENTS.md Skills index"));
    }

    #[test]
    fn query_param_re_ranks_suggestions() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = fixture_registry(
            &dir,
            &[
                ("testing-ci", "Run the test suite"),
                ("tool-execution", "Run tools"),
            ],
        );
        let err = skill_not_found("typo-name", Some("tool"), &registry);
        let msg = err.to_string();
        assert!(msg.contains("tool-execution"));
        assert!(msg.contains("AGENTS.md Skills index"));
    }

    #[test]
    fn empty_registry_reports_no_skills_registered() {
        let dir = tempfile::TempDir::new().unwrap();
        let config = crate::skills::AssetDiscoveryConfig::default();
        let registry =
            crate::skills::AssetRegistry::build_with_home(&config, dir.path(), &[], None);
        assert!(registry.list().is_empty());
        let err = skill_not_found("anything", None, &registry);
        assert!(err.to_string().contains("no skills registered"));
    }

    #[test]
    fn skill_description_and_params_document_suggestions() {
        let tool = SkillTool::default();
        assert!(!tool.description().contains("no skill-listing operation"));
        assert!(tool.description().contains("did-you-mean"));
        let params = tool.parameters();
        let required = params.get("required").and_then(|v| v.as_array()).unwrap();
        assert!(required.iter().any(|v| v == "name"));
        assert!(params["properties"].get("query").is_some());
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
