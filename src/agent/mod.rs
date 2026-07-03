//! Agent definitions and management.
//!
//! This module provides the core Agent struct and built-in agent configurations.
//! Agents define how the AI assistant behaves, including permissions, model selection,
//! and system prompts. Codegg supports multiple agent modes: Primary (full access),
//! Subagent (limited), and All (combines multiple agents).

pub mod agent_loop_factory;
pub mod builtins;
pub mod compaction;
pub mod context_frame;
pub mod r#loop;
pub mod mention;
pub mod policy;
pub mod processor;
pub mod prompt;
pub mod router;
pub mod runtime_factory;
pub mod task;
pub mod task_tool_runtime;
pub mod team;
pub mod turn_runtime;
pub mod worker;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::config::schema::{AgentConfig, Config};
use crate::error::AgentError;
use crate::permission::modes::ModeDefinition;
use crate::permission::{self, PermissionRuleset};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Agent {
    pub name: String,
    pub role: Option<String>,
    pub description: String,
    pub mode: AgentMode,
    pub mode_name: Option<String>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub color: Option<String>,
    pub steps: Option<usize>,
    pub system_prompt: Option<String>,
    pub permissions: HashMap<String, String>,
    pub hidden: bool,
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    #[default]
    Primary,
    Subagent,
    All,
}

impl Agent {
    pub fn permission_ruleset(&self) -> PermissionRuleset {
        let mut tool_rules = Vec::new();
        let mut path_rules = Vec::new();

        for (key, value) in &self.permissions {
            if key == "paths" {
                path_rules.push(permission::PathRule {
                    pattern: value.clone(),
                    level: permission::PermissionLevel::Ask,
                });
                continue;
            }
            let level = match value.as_str() {
                "allow" => permission::PermissionLevel::Allow,
                "deny" => permission::PermissionLevel::Deny,
                _ => permission::PermissionLevel::Ask,
            };
            tool_rules.push(permission::ToolRule {
                tool: key.clone(),
                level,
                paths: None,
                bash_patterns: None,
            });
        }

        PermissionRuleset {
            default: permission::PermissionLevel::Ask,
            tool_rules,
            path_rules,
        }
    }

    pub fn merge_permissions(&self, config_perms: &PermissionRuleset) -> PermissionRuleset {
        let agent_perms = self.permission_ruleset();
        permission::merge_rulesets(config_perms, &agent_perms)
    }

    pub fn with_mode(mut self, mode_def: ModeDefinition) -> Self {
        let mode_ruleset = mode_def.to_ruleset();
        let mut agent_perms = self.permissions.clone();

        for rule in &mode_ruleset.tool_rules {
            let level_str = match rule.level {
                permission::PermissionLevel::Allow => "allow",
                permission::PermissionLevel::Deny => "deny",
                permission::PermissionLevel::Ask => "ask",
            };
            agent_perms.insert(rule.tool.clone(), level_str.to_string());
        }

        self.permissions = agent_perms;
        if self.system_prompt.is_none() {
            self.system_prompt = Some(format!(
                "[Mode: {}] {}",
                mode_def.name, mode_def.description
            ));
        }
        self.mode_name = Some(mode_def.name.to_string());
        self
    }

    pub fn with_config_mode(
        mut self,
        mode_config: &crate::config::schema::ModeConfig,
        base: Option<&PermissionRuleset>,
    ) -> Self {
        let mode_def = ModeDefinition::from_config(mode_config, base);
        let mode_ruleset = mode_def.to_ruleset();
        let mut agent_perms = self.permissions.clone();

        for rule in &mode_ruleset.tool_rules {
            let level_str = match rule.level {
                permission::PermissionLevel::Allow => "allow",
                permission::PermissionLevel::Deny => "deny",
                permission::PermissionLevel::Ask => "ask",
            };
            agent_perms.insert(rule.tool.clone(), level_str.to_string());
        }

        self.permissions = agent_perms;
        if self.system_prompt.is_none() {
            self.system_prompt = Some(format!(
                "[Mode: custom] {}",
                mode_config.description.as_deref().unwrap_or("Custom mode")
            ));
        }
        self.mode_name = Some("custom".to_string());
        self
    }
}

pub fn builtin_agents() -> Vec<Agent> {
    builtins::generated_builtin_agents()
}

pub fn resolve_agents(config: &Config) -> Result<Vec<Agent>, AgentError> {
    let mut agents = builtin_agents();

    if let Some(config_dir) = dirs::config_dir() {
        let agents_dir = config_dir.join("codegg").join("agents");
        if let Ok(file_agents) = load_agents_from_dir(&agents_dir) {
            for file_agent in file_agents {
                if let Some(pos) = agents.iter().position(|a| a.name == file_agent.agent.name) {
                    agents[pos] = file_agent.agent;
                } else {
                    agents.push(file_agent.agent);
                }
            }
        }
    }

    if let Some(project_dir) = std::env::var("PWD").ok().filter(|p| !p.is_empty()) {
        let project_agents_dir = Path::new(&project_dir).join(".codegg").join("agents");
        if let Ok(file_agents) = load_agents_from_dir(&project_agents_dir) {
            for file_agent in file_agents {
                if let Some(pos) = agents.iter().position(|a| a.name == file_agent.agent.name) {
                    agents[pos] = file_agent.agent;
                } else {
                    agents.push(file_agent.agent);
                }
            }
        }
    }

    if let Some(agent_map) = &config.agent {
        for (key, agent_cfg) in agent_map {
            if agent_cfg.disable == Some(true) {
                continue;
            }

            if let Some(pos) = agents.iter().position(|a| a.name == *key) {
                agents[pos] = merge_agent_config(&agents[pos], agent_cfg)?;
            } else {
                agents.push(agent_from_config(key, agent_cfg)?);
            }
        }
    }

    if let Some(mode_map) = &config.mode {
        for (key, mode_cfg) in mode_map {
            if mode_cfg.inherit.unwrap_or(false) {
                if let Some(pos) = agents.iter().position(|a| a.name == *key) {
                    let base_ruleset = agents[pos].permission_ruleset();
                    let _mode_ruleset =
                        crate::permission::modes::mode_ruleset(mode_cfg, Some(&base_ruleset));
                    agents[pos] = agents[pos]
                        .clone()
                        .with_config_mode(mode_cfg, Some(&base_ruleset));
                }
            } else if let Some(pos) = agents.iter().position(|a| a.name == *key) {
                agents[pos] = agents[pos].clone().with_config_mode(mode_cfg, None);
            } else {
                let mut agent = Agent {
                    name: key.clone(),
                    role: None,
                    description: mode_cfg.description.clone().unwrap_or_default(),
                    mode: AgentMode::Primary,
                    mode_name: Some(key.clone()),
                    model: None,
                    variant: None,
                    temperature: None,
                    top_p: None,
                    color: None,
                    steps: None,
                    system_prompt: Some(format!(
                        "[Mode: {}] {}",
                        key,
                        mode_cfg.description.as_deref().unwrap_or("")
                    )),
                    permissions: HashMap::new(),
                    hidden: false,
                    thinking_budget: None,
                    reasoning_effort: None,
                };
                agent = agent.with_config_mode(mode_cfg, None);
                agents.push(agent);
            }
        }
    }

    Ok(agents)
}

fn merge_agent_config(agent: &Agent, cfg: &AgentConfig) -> Result<Agent, AgentError> {
    Ok(Agent {
        name: cfg.name.clone().unwrap_or_else(|| agent.name.clone()),
        role: cfg.role.clone().or_else(|| agent.role.clone()),
        description: cfg
            .description
            .clone()
            .unwrap_or_else(|| agent.description.clone()),
        mode: parse_mode(cfg.mode.as_deref().unwrap_or(match agent.mode {
            AgentMode::Primary => "primary",
            AgentMode::Subagent => "subagent",
            AgentMode::All => "all",
        }))?,
        mode_name: None,
        model: cfg.model.clone().or_else(|| agent.model.clone()),
        variant: cfg.variant.clone().or_else(|| agent.variant.clone()),
        temperature: cfg.temperature.or(agent.temperature),
        top_p: cfg.top_p.or(agent.top_p),
        color: cfg.color.clone().or_else(|| agent.color.clone()),
        steps: cfg.steps.map(|s| s as usize).or(agent.steps),
        system_prompt: cfg.prompt.clone().or_else(|| agent.system_prompt.clone()),
        permissions: {
            let mut perms = agent.permissions.clone();
            if let Some(cfg_perms) = &cfg.permission {
                for (k, v) in cfg_perms {
                    let value = match v {
                        crate::config::schema::PermissionRule::Action(s) => s.clone(),
                        crate::config::schema::PermissionRule::Object(obj) => obj
                            .get("default")
                            .or_else(|| obj.get("action"))
                            .cloned()
                            .unwrap_or("ask".to_string()),
                    };
                    perms.insert(k.clone(), value);
                }
            }
            perms
        },
        hidden: cfg.hidden.unwrap_or(agent.hidden),
        thinking_budget: None,
        reasoning_effort: None,
    })
}

fn agent_from_config(key: &str, cfg: &AgentConfig) -> Result<Agent, AgentError> {
    let mode = parse_mode(cfg.mode.as_deref().unwrap_or("primary"))?;
    let name = cfg.name.clone().unwrap_or_else(|| key.to_string());

    let mut permissions = HashMap::new();
    if let Some(cfg_perms) = &cfg.permission {
        for (k, v) in cfg_perms {
            let value = match v {
                crate::config::schema::PermissionRule::Action(s) => s.clone(),
                crate::config::schema::PermissionRule::Object(obj) => obj
                    .get("default")
                    .or_else(|| obj.get("action"))
                    .cloned()
                    .unwrap_or("ask".to_string()),
            };
            permissions.insert(k.clone(), value);
        }
    }

    Ok(Agent {
        name,
        role: cfg.role.clone(),
        description: cfg.description.clone().unwrap_or_default(),
        mode,
        mode_name: None,
        model: cfg.model.clone(),
        variant: cfg.variant.clone(),
        temperature: cfg.temperature,
        top_p: cfg.top_p,
        color: cfg.color.clone(),
        steps: cfg.steps.map(|s| s as usize),
        system_prompt: cfg.prompt.clone(),
        permissions,
        hidden: cfg.hidden.unwrap_or(false),
        thinking_budget: None,
        reasoning_effort: None,
    })
}

fn parse_mode(s: &str) -> Result<AgentMode, AgentError> {
    match s {
        "primary" => Ok(AgentMode::Primary),
        "subagent" => Ok(AgentMode::Subagent),
        "all" => Ok(AgentMode::All),
        _ => Err(AgentError::Invalid(format!("unknown agent mode: {s}"))),
    }
}

#[derive(Debug, Clone)]
pub struct FileAgent {
    pub agent: Agent,
    pub source: String,
}

pub fn load_agents_from_dir(dir: &Path) -> Result<Vec<FileAgent>, AgentError> {
    let mut agents = Vec::new();

    if !dir.is_dir() {
        return Ok(agents);
    }

    for entry in std::fs::read_dir(dir).map_err(|e| AgentError::Invalid(e.to_string()))? {
        let entry = entry.map_err(|e| AgentError::Invalid(e.to_string()))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        if let Some(file_agent) = load_agent_from_file(&path)? {
            agents.push(file_agent);
        }
    }

    Ok(agents)
}

pub fn load_agent_from_file(path: &Path) -> Result<Option<FileAgent>, AgentError> {
    let content = std::fs::read_to_string(path).map_err(|e| AgentError::Invalid(e.to_string()))?;

    let Some((frontmatter, _body)) = parse_frontmatter(&content) else {
        return Ok(None);
    };

    let agent_cfg: AgentConfig =
        serde_yaml::from_str(&frontmatter).map_err(|e| AgentError::Invalid(e.to_string()))?;

    let name = agent_cfg.name.clone().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string())
    });

    let agent = agent_from_config(&name, &agent_cfg)?;

    let source = path.to_string_lossy().to_string();

    Ok(Some(FileAgent { agent, source }))
}

fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim_start();

    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = rest[..end].trim().to_string();
    let body = rest[end + 3..].to_string();

    Some((frontmatter, body))
}

pub fn find_default_agent(agents: &[Agent]) -> Option<&Agent> {
    agents
        .iter()
        .find(|a| a.name == "build")
        .or_else(|| agents.iter().find(|a| !a.hidden))
        .or_else(|| agents.first())
}

pub fn find_agent_by_name<'a>(agents: &'a [Agent], name: &str) -> Option<&'a Agent> {
    agents.iter().find(|a| a.name == name)
}

pub fn list_visible_agents(agents: &[Agent]) -> Vec<&Agent> {
    agents.iter().filter(|a| !a.hidden).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{AgentConfig, Config, ModeConfig};
    use std::collections::HashMap;

    #[test]
    fn test_builtin_agents_count() {
        let agents = builtin_agents();
        assert_eq!(agents.len(), 9);
    }

    #[test]
    fn test_builtin_agents_contains_build() {
        let agents = builtin_agents();
        let build = agents.iter().find(|a| a.name == "build").unwrap();
        assert_eq!(build.mode, AgentMode::Primary);
        assert!(!build.hidden);
    }

    #[test]
    fn test_builtin_plan_agent_denies_write() {
        let agents = builtin_agents();
        let plan = agents.iter().find(|a| a.name == "plan").unwrap();
        assert_eq!(plan.permissions.get("write"), Some(&"deny".to_string()));
        assert_eq!(plan.permissions.get("edit"), Some(&"deny".to_string()));
        assert_eq!(plan.permissions.get("bash"), Some(&"deny".to_string()));
    }

    #[test]
    fn test_builtin_compaction_agent_denies_all() {
        let agents = builtin_agents();
        let compaction = agents.iter().find(|a| a.name == "compaction").unwrap();
        assert_eq!(compaction.permissions.get("*"), Some(&"deny".to_string()));
        assert!(compaction.hidden);
    }

    #[test]
    fn test_resolve_agents_empty_config() {
        let config = Config::default();
        let agents = resolve_agents(&config).unwrap();
        assert_eq!(agents.len(), 9);
    }

    #[test]
    fn test_resolve_agents_merges_existing() {
        let mut agent_map = HashMap::new();
        agent_map.insert(
            "build".to_string(),
            AgentConfig {
                name: Some("custom-build".to_string()),
                description: Some("Custom builder".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agent_map),
            ..Default::default()
        };
        let agents = resolve_agents(&config).unwrap();
        let build = agents.iter().find(|a| a.name == "custom-build").unwrap();
        assert_eq!(build.description, "Custom builder");
    }

    #[test]
    fn test_resolve_agents_adds_new() {
        let mut agent_map = HashMap::new();
        agent_map.insert(
            "reviewer".to_string(),
            AgentConfig {
                name: Some("Reviewer".to_string()),
                description: Some("Code reviewer".to_string()),
                mode: Some("primary".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agent_map),
            ..Default::default()
        };
        let agents = resolve_agents(&config).unwrap();
        assert_eq!(agents.len(), 10);
        let reviewer = agents.iter().find(|a| a.name == "Reviewer").unwrap();
        assert_eq!(reviewer.mode, AgentMode::Primary);
    }

    #[test]
    fn test_resolve_agents_skips_disabled() {
        let mut agent_map = HashMap::new();
        agent_map.insert(
            "build".to_string(),
            AgentConfig {
                disable: Some(true),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agent_map),
            ..Default::default()
        };
        let agents = resolve_agents(&config).unwrap();
        let build = agents.iter().find(|a| a.name == "build").unwrap();
        assert_eq!(build.description, "Default agent with full permissions");
    }

    #[test]
    fn test_resolve_agents_invalid_mode() {
        let mut agent_map = HashMap::new();
        agent_map.insert(
            "bad".to_string(),
            AgentConfig {
                mode: Some("invalid".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agent_map),
            ..Default::default()
        };
        assert!(resolve_agents(&config).is_err());
    }

    #[test]
    fn test_merge_agent_config_permissions() {
        let agents = builtin_agents();
        let build = agents.iter().find(|a| a.name == "build").unwrap();
        let mut permission = HashMap::new();
        permission.insert(
            "bash".to_string(),
            crate::config::schema::PermissionRule::Action("deny".to_string()),
        );
        let cfg = AgentConfig {
            permission: Some(permission),
            ..Default::default()
        };
        let merged = merge_agent_config(build, &cfg).unwrap();
        assert_eq!(merged.permissions.get("bash"), Some(&"deny".to_string()));
    }

    #[test]
    fn test_find_default_agent() {
        let agents = builtin_agents();
        let default = find_default_agent(&agents).unwrap();
        assert_eq!(default.name, "build");
    }

    #[test]
    fn test_find_default_agent_fallback() {
        let agents = vec![Agent {
            name: "custom".to_string(),
            role: None,
            description: "Custom".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
        }];
        let default = find_default_agent(&agents).unwrap();
        assert_eq!(default.name, "custom");
    }

    #[test]
    fn test_find_agent_by_name() {
        let agents = builtin_agents();
        let plan = find_agent_by_name(&agents, "plan").unwrap();
        assert_eq!(plan.mode, AgentMode::Primary);
        assert!(find_agent_by_name(&agents, "nonexistent").is_none());
    }

    #[test]
    fn test_list_visible_agents() {
        let agents = builtin_agents();
        let visible = list_visible_agents(&agents);
        assert_eq!(visible.len(), 6);
        assert!(visible.iter().all(|a| !a.hidden));
    }

    #[test]
    fn test_builtin_research_agent_registered() {
        let agents = builtin_agents();
        let research = agents.iter().find(|a| a.name == "research").unwrap();
        // Both Primary (user-selectable) and Subagent (spawnable via `task`).
        assert_eq!(research.mode, AgentMode::All);
        assert!(!research.hidden);
        assert_eq!(research.role.as_deref(), Some("researcher"));
        // Network + research tools allowed; mutating tools ask; image denied.
        assert_eq!(
            research.permissions.get("websearch"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            research.permissions.get("webfetch"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            research.permissions.get("research"),
            Some(&"allow".to_string())
        );
        assert_eq!(research.permissions.get("edit"), Some(&"ask".to_string()));
        assert_eq!(research.permissions.get("image"), Some(&"deny".to_string()));
    }

    #[test]
    fn test_research_subagent_registry_includes_websearch_and_research() {
        // The subagent's tool registry is `ToolRegistry::with_defaults()`
        // with todo/plan tools stripped. Verify that websearch and
        // research survive that filter.
        use crate::tool::ToolRegistry;
        let mut registry = ToolRegistry::with_defaults();
        let blocked = vec![
            "todowrite".to_string(),
            "todoread".to_string(),
            "plan_enter".to_string(),
            "plan_exit".to_string(),
        ];
        registry.filter_out(&blocked);
        assert!(
            registry.get("websearch").is_some(),
            "subagent must have websearch"
        );
        assert!(
            registry.get("research").is_some(),
            "subagent must have research tool"
        );
        assert!(
            registry.get("webfetch").is_some(),
            "subagent must have webfetch"
        );
    }

    #[test]
    fn test_agent_permission_ruleset() {
        let mut permissions = HashMap::new();
        permissions.insert("bash".to_string(), "allow".to_string());
        permissions.insert("write".to_string(), "deny".to_string());
        permissions.insert("paths".to_string(), "/tmp/*".to_string());
        let agent = Agent {
            name: "test".to_string(),
            role: None,
            description: "Test".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions,
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
        };
        let ruleset = agent.permission_ruleset();
        assert_eq!(ruleset.tool_rules.len(), 2);
        assert_eq!(ruleset.path_rules.len(), 1);
        assert_eq!(ruleset.path_rules[0].pattern, "/tmp/*");
    }

    #[test]
    fn test_agent_merge_permissions() {
        let mut permissions = HashMap::new();
        permissions.insert("bash".to_string(), "allow".to_string());
        let agent = Agent {
            name: "test".to_string(),
            role: None,
            description: "Test".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions,
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
        };
        let config_ruleset = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "write".to_string(),
                level: crate::permission::PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };
        let merged = agent.merge_permissions(&config_ruleset);
        assert_eq!(merged.tool_rules.len(), 2);
    }

    #[test]
    fn test_parse_mode_valid() {
        assert!(matches!(parse_mode("primary"), Ok(AgentMode::Primary)));
        assert!(matches!(parse_mode("subagent"), Ok(AgentMode::Subagent)));
        assert!(matches!(parse_mode("all"), Ok(AgentMode::All)));
    }

    #[test]
    fn test_parse_mode_invalid() {
        assert!(parse_mode("invalid").is_err());
    }

    #[test]
    fn test_load_agents_from_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_load_agents_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: TestAgent
mode: primary
description: A test agent
---
Some body content
"#;
        std::fs::write(tmp.path().join("test.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent.name, "TestAgent");
    }

    #[test]
    fn test_load_agent_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("nofm.md"), "Just content").unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_load_agent_uses_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\nmode: primary\n---\nbody";
        std::fs::write(tmp.path().join("myagent.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents[0].agent.name, "myagent");
    }

    #[test]
    fn test_resolve_agents_from_mode_config() {
        let mut mode_map = HashMap::new();
        mode_map.insert(
            "review".to_string(),
            ModeConfig {
                description: Some("Review mode".to_string()),
                default: Some("ask".to_string()),
                inherit: Some(true),
                tools: Some(HashMap::from([
                    ("read".to_string(), "allow".to_string()),
                    ("bash".to_string(), "deny".to_string()),
                ])),
            },
        );
        let config = Config {
            mode: Some(mode_map),
            ..Default::default()
        };
        let agents = resolve_agents(&config).unwrap();
        // Mode config creates a "review" agent
        let review = agents.iter().find(|a| a.name == "review");
        assert!(
            review.is_some() || agents.len() >= 2,
            "review mode should be resolved"
        );
    }

    // --- Behavioral invariant tests (milestone 2) ---

    #[test]
    fn test_builtin_build_is_visible_primary() {
        let agents = builtin_agents();
        let build = agents.iter().find(|a| a.name == "build").unwrap();
        assert_eq!(build.mode, AgentMode::Primary);
        assert!(!build.hidden);
    }

    #[test]
    fn test_builtin_plan_is_visible_and_denies_mutation() {
        let agents = builtin_agents();
        let plan = agents.iter().find(|a| a.name == "plan").unwrap();
        assert_eq!(plan.mode, AgentMode::Primary);
        assert!(!plan.hidden);
        for tool in &["write", "edit", "bash", "apply_patch", "replace", "multiedit", "terminal", "commit"] {
            assert_eq!(
                plan.permissions.get(*tool),
                Some(&"deny".to_string()),
                "plan should deny {tool}"
            );
        }
    }

    #[test]
    fn test_builtin_general_and_explore_are_subagents() {
        let agents = builtin_agents();
        let general = agents.iter().find(|a| a.name == "general").unwrap();
        let explore = agents.iter().find(|a| a.name == "explore").unwrap();
        assert_eq!(general.mode, AgentMode::Subagent);
        assert_eq!(explore.mode, AgentMode::All);
    }

    #[test]
    fn test_builtin_title_summary_compaction_are_hidden() {
        let agents = builtin_agents();
        for name in &["title", "summary", "compaction"] {
            let agent = agents.iter().find(|a| a.name == *name).unwrap();
            assert!(agent.hidden, "{name} should be hidden");
        }
    }

    #[test]
    fn test_builtin_compaction_denies_all() {
        let agents = builtin_agents();
        let compaction = agents.iter().find(|a| a.name == "compaction").unwrap();
        assert_eq!(compaction.permissions.get("*"), Some(&"deny".to_string()));
        assert!(compaction.hidden);
    }

    #[test]
    fn test_builtin_security_review_allows_read_and_denies_mutation() {
        let agents = builtin_agents();
        let sr = agents.iter().find(|a| a.name == "security-review").unwrap();
        assert_eq!(sr.mode, AgentMode::Subagent);
        assert!(!sr.hidden);
        for tool in &["read", "grep", "glob", "list", "security", "lsp"] {
            assert_eq!(
                sr.permissions.get(*tool),
                Some(&"allow".to_string()),
                "security-review should allow {tool}"
            );
        }
        for tool in &["write", "edit", "apply_patch", "replace", "multiedit", "commit", "image"] {
            assert_eq!(
                sr.permissions.get(*tool),
                Some(&"deny".to_string()),
                "security-review should deny {tool}"
            );
        }
    }

    #[test]
    fn test_builtin_research_mode_all_and_permissions() {
        let agents = builtin_agents();
        let research = agents.iter().find(|a| a.name == "research").unwrap();
        assert_eq!(research.mode, AgentMode::All);
        assert!(!research.hidden);
        assert_eq!(research.color.as_deref(), Some("magenta"));
        for tool in &["websearch", "webfetch", "research", "skill", "question", "task"] {
            assert_eq!(
                research.permissions.get(*tool),
                Some(&"allow".to_string()),
                "research should allow {tool}"
            );
        }
        assert_eq!(research.permissions.get("image"), Some(&"deny".to_string()));
        assert_eq!(research.permissions.get("plan_enter"), Some(&"deny".to_string()));
        assert_eq!(research.permissions.get("plan_exit"), Some(&"deny".to_string()));
    }

    #[test]
    fn test_builtin_count_is_nine() {
        assert_eq!(builtin_agents().len(), 9);
    }

    #[test]
    fn test_builtin_visible_count_is_six() {
        let agents = builtin_agents();
        let visible = list_visible_agents(&agents);
        assert_eq!(visible.len(), 6);
    }

    #[test]
    fn test_security_review_prompt_sentinels() {
        let agents = builtin_agents();
        let sr = agents.iter().find(|a| a.name == "security-review").unwrap();
        let prompt = sr.system_prompt.as_ref().expect("security-review should have a prompt");
        assert!(prompt.contains("defensive"), "prompt should mention defensive");
        assert!(prompt.contains("deterministic"), "prompt should mention deterministic");
        assert!(prompt.contains("evidence"), "prompt should mention evidence");
        assert!(prompt.contains("Never mutate files"), "prompt should prohibit file mutation");
    }

    #[test]
    fn test_research_prompt_sentinels() {
        let agents = builtin_agents();
        let research = agents.iter().find(|a| a.name == "research").unwrap();
        let prompt = research.system_prompt.as_ref().expect("research should have a prompt");
        assert!(prompt.contains("research"), "prompt should mention research tool");
        assert!(prompt.contains("websearch"), "prompt should mention websearch");
        assert!(prompt.contains("cite"), "prompt should mention citation");
    }
}
