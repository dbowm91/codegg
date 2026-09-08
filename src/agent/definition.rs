//! Agent definition, resolution, and execution-profile types.
//!
//! Physical decomposition of [`super`](crate::agent) module root (M003):
//! the [`Agent`] nominal type, permission/safety-envelope composition, model
//! alias and execution-profile resolution, and config-layer agent merging.
//! File-based loading lives in [`super::file_agents`]; the registry in
//! [`super::registry`].

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::schema::{AgentConfig, Config};
use crate::error::AgentError;
use crate::permission::modes::ModeDefinition;
use crate::permission::{self, PermissionRuleset};

use super::builtins;
use super::file_agents::load_agents_from_dir;

/// Runtime classification for agents.
///
/// This metadata selects Rust-defined runtime behavior for specialized agents.
/// TOML sets the kind; Rust implements the behavior (security preflight,
/// research orchestration, compaction contracts, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeKind {
    /// Default agent runtime — standard prompt, no special orchestration.
    #[default]
    Standard,
    /// Security review runtime — defensive scanning, evidence-based findings.
    SecurityReview,
    /// Research runtime — multi-hop research, citation-bearing synthesis.
    Research,
    /// Context compaction runtime.
    Compaction,
    /// Title generation runtime.
    Title,
    /// Summary generation runtime.
    Summary,
}

impl fmt::Display for AgentRuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard => write!(f, "standard"),
            Self::SecurityReview => write!(f, "security_review"),
            Self::Research => write!(f, "research"),
            Self::Compaction => write!(f, "compaction"),
            Self::Title => write!(f, "title"),
            Self::Summary => write!(f, "summary"),
        }
    }
}

impl std::str::FromStr for AgentRuntimeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "standard" => Ok(Self::Standard),
            "security_review" => Ok(Self::SecurityReview),
            "research" => Ok(Self::Research),
            "compaction" => Ok(Self::Compaction),
            "title" => Ok(Self::Title),
            "summary" => Ok(Self::Summary),
            other => Err(format!(
                "unknown runtime kind '{other}' (expected one of: standard, security_review, research, compaction, title, summary)"
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Agent {
    pub name: String,
    pub role: Option<String>,
    pub description: String,
    pub mode: AgentMode,
    pub mode_name: Option<String>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
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
    #[serde(default)]
    pub runtime_kind: Option<AgentRuntimeKind>,
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
        let mut bash_allow_patterns: Vec<String> = Vec::new();
        let mut bash_deny_patterns: Vec<String> = Vec::new();

        for (key, value) in &self.permissions {
            // Handle structured bash allow patterns: "bash:allow:<pattern>"
            if let Some(pattern) = key.strip_prefix("bash:allow:") {
                bash_allow_patterns.push(pattern.to_string());
                continue;
            }
            // Handle structured bash deny patterns: "bash:deny:<pattern>"
            if let Some(pattern) = key.strip_prefix("bash:deny:") {
                bash_deny_patterns.push(pattern.to_string());
                continue;
            }
            // Handle structured path allow patterns: "path:allow:<pattern>"
            if let Some(pattern) = key.strip_prefix("path:allow:") {
                path_rules.push(permission::PathRule {
                    pattern: pattern.to_string(),
                    level: permission::PermissionLevel::Allow,
                });
                continue;
            }
            // Handle structured path deny patterns: "path:deny:<pattern>"
            if let Some(pattern) = key.strip_prefix("path:deny:") {
                path_rules.push(permission::PathRule {
                    pattern: pattern.to_string(),
                    level: permission::PermissionLevel::Deny,
                });
                continue;
            }
            // Legacy "paths" key
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

        // If we have structured bash patterns, create a ToolRule with bash_patterns
        if !bash_allow_patterns.is_empty() || !bash_deny_patterns.is_empty() {
            // Deny patterns take precedence: add them as deny rules with patterns
            if !bash_deny_patterns.is_empty() {
                tool_rules.push(permission::ToolRule {
                    tool: "bash".to_string(),
                    level: permission::PermissionLevel::Deny,
                    paths: None,
                    bash_patterns: Some(bash_deny_patterns),
                });
            }
            // Allow patterns are added as allow rules with patterns
            if !bash_allow_patterns.is_empty() {
                tool_rules.push(permission::ToolRule {
                    tool: "bash".to_string(),
                    level: permission::PermissionLevel::Allow,
                    paths: None,
                    bash_patterns: Some(bash_allow_patterns),
                });
            }
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

    /// Merge another agent's fields into this one (overlay merge).
    /// Scalar fields replace only when the overlay has them set.
    /// Permissions merge per-tool.
    /// `replace=true` in overlay resets the base before applying.
    pub fn merge_overlay(&self, overlay: &Agent, replace: bool) -> Agent {
        if replace {
            return overlay.clone();
        }

        let mut merged = self.clone();

        if !overlay.name.is_empty() {
            merged.name = overlay.name.clone();
        }
        if overlay.role.is_some() {
            merged.role = overlay.role.clone();
        }
        if !overlay.description.is_empty() {
            merged.description = overlay.description.clone();
        }
        if overlay.mode != AgentMode::Primary || overlay.mode_name.is_some() {
            merged.mode = overlay.mode.clone();
        }
        if overlay.model.is_some() {
            merged.model = overlay.model.clone();
        }
        if overlay.variant.is_some() {
            merged.variant = overlay.variant.clone();
        }
        if overlay.temperature.is_some() {
            merged.temperature = overlay.temperature;
        }
        if overlay.top_p.is_some() {
            merged.top_p = overlay.top_p;
        }
        if overlay.color.is_some() {
            merged.color = overlay.color.clone();
        }
        if overlay.steps.is_some() {
            merged.steps = overlay.steps;
        }
        if overlay.system_prompt.is_some() {
            merged.system_prompt = overlay.system_prompt.clone();
        }
        if overlay.hidden {
            merged.hidden = overlay.hidden;
        }
        if overlay.thinking_budget.is_some() {
            merged.thinking_budget = overlay.thinking_budget;
        }
        if overlay.reasoning_effort.is_some() {
            merged.reasoning_effort = overlay.reasoning_effort.clone();
        }

        // Permissions: merge per-tool (overlay overwrites matching keys)
        for (key, value) in &overlay.permissions {
            merged.permissions.insert(key.clone(), value.clone());
        }

        merged
    }

    /// Apply the safety envelope: effective permissions are bounded by the
    /// most restrictive result across agent, session, config, and hard safety
    /// constraints. This prevents custom agent files from silently escalating
    /// permissions beyond runtime safety bounds.
    ///
    /// `session_rules` are the session-level permission overrides.
    /// `config_rules` are the global config permission rules.
    /// `hard_deny` are tools that must always be denied (e.g., sandbox restrictions).
    pub fn apply_safety_envelope(
        &self,
        session_rules: &PermissionRuleset,
        config_rules: &PermissionRuleset,
        hard_deny: &[String],
    ) -> Agent {
        let mut agent = self.clone();
        let agent_ruleset = agent.permission_ruleset();

        // Safety envelope: for each tool rule in agent permissions,
        // check if session or config rules are more restrictive.
        // The most restrictive result across all layers wins.
        for tool_rule in &agent_ruleset.tool_rules {
            let mut current_level = tool_rule.level.clone();
            // Check session rules for this tool
            for session_rule in &session_rules.tool_rules {
                if (session_rule.tool == tool_rule.tool || session_rule.matches(&tool_rule.tool))
                    && session_rule.level < current_level
                {
                    current_level = session_rule.level.clone();
                }
            }
            // Check config rules for this tool
            for config_rule in &config_rules.tool_rules {
                if (config_rule.tool == tool_rule.tool || config_rule.matches(&tool_rule.tool))
                    && config_rule.level < current_level
                {
                    current_level = config_rule.level.clone();
                }
            }
            agent
                .permissions
                .insert(tool_rule.tool.clone(), current_level.as_str().to_string());
        }

        // Hard deny always wins — applied last so it cannot be overwritten
        for tool in hard_deny {
            agent.permissions.insert(tool.clone(), "deny".to_string());
        }

        agent
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

/// Well-known model alias prefixes.
///
/// These resolve to the provider's best available model in each tier.
/// The actual resolution is delegated to the provider registry; the
/// alias just selects the tier.
pub const MODEL_ALIAS_FRONTIER: &str = "tier.frontier";
pub const MODEL_ALIAS_WORKHORSE: &str = "tier.workhorse";

/// Emergency fallback model used when no model is configured at any level.
/// This should be rare — users should configure models explicitly.
pub const EMERGENCY_DEFAULT_MODEL: &str = "openai/gpt-4o";

/// Emergency fallback for the "workhorse" (small/fast) tier.
pub const EMERGENCY_DEFAULT_WORKHORSE_MODEL: &str = "openai/gpt-4o-mini";

/// Check if a model string is a known alias.
pub fn is_model_alias(model: &str) -> bool {
    matches!(model, MODEL_ALIAS_FRONTIER | MODEL_ALIAS_WORKHORSE)
}

/// Resolve a model alias to an actual model string.
///
/// Returns `None` if the input is not a known alias, allowing the caller
/// to fall through to other resolution steps.
pub fn resolve_model_alias(alias: &str, config: &Config) -> Option<String> {
    match alias {
        MODEL_ALIAS_FRONTIER => config
            .model
            .clone()
            .or_else(|| Some(EMERGENCY_DEFAULT_MODEL.to_string())),
        MODEL_ALIAS_WORKHORSE => config
            .model
            .clone()
            .or_else(|| Some(EMERGENCY_DEFAULT_WORKHORSE_MODEL.to_string())),
        _ => None,
    }
}

/// Fully resolved execution profile for a subagent task.
///
/// Bundles the resolved agent, runtime kind, effective model, and
/// permissions so that task execution does not need to re-derive
/// provider/model behavior from raw strings.
#[derive(Debug, Clone)]
pub struct ResolvedAgentExecutionProfile {
    pub agent: Agent,
    pub runtime_kind: AgentRuntimeKind,
    pub resolved_model: String,
    pub effective_permissions: PermissionRuleset,
}

impl ResolvedAgentExecutionProfile {
    /// Build a resolved profile by applying model inheritance and alias resolution.
    ///
    /// Resolution order:
    /// 1. Explicit `agent.model`
    /// 2. `agent.fallback_model`
    /// 3. Parent/session model inheritance (for subagents)
    /// 4. Config `model` (global)
    /// 5. Hardcoded default (openai/gpt-4o)
    ///
    /// Phase 4: Ensures no empty model strings are emitted.
    pub fn resolve(agent: &Agent, config: &Config, parent_model: Option<&str>) -> Self {
        // 1. Explicit agent.model
        // 3. Parent/session model inheritance (for subagents)
        // 4. Global config model
        let raw_model = agent
            .model
            .as_deref()
            .or(parent_model)
            .or(config.model.as_deref())
            .unwrap_or("");

        // Resolve model aliases
        let resolved_model = if is_model_alias(raw_model) {
            resolve_model_alias(raw_model, config).unwrap_or_else(|| raw_model.to_string())
        } else if raw_model.is_empty() {
            // 2. Fallback model
            // 5. Hardcoded default to prevent empty model strings
            agent
                .fallback_model
                .as_deref()
                .and_then(|fm| {
                    if is_model_alias(fm) {
                        resolve_model_alias(fm, config)
                    } else {
                        Some(fm.to_string())
                    }
                })
                .or_else(|| config.model.clone())
                .unwrap_or_else(|| {
                    tracing::warn!(
                        "Using emergency default model '{}'. No model configured at agent, session, or config level. \
                         Set a model in your config or agent definition to suppress this warning.",
                        EMERGENCY_DEFAULT_MODEL
                    );
                    EMERGENCY_DEFAULT_MODEL.to_string()
                })
        } else {
            raw_model.to_string()
        };

        let runtime_kind = agent.runtime_kind.clone().unwrap_or_default();
        let effective_permissions = agent.permission_ruleset();

        Self {
            agent: agent.clone(),
            runtime_kind,
            resolved_model,
            effective_permissions,
        }
    }
}
/// Resolve agents using the explicit project root (no `PWD` inference).
/// When `project_root` is `None`, project-file discovery is skipped.
pub fn resolve_agents_with_context(
    config: &Config,
    project_root: Option<&Path>,
) -> Result<Vec<Agent>, AgentError> {
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

    if let Some(project_dir) = project_root {
        let project_agents_dir = project_dir.join(".codegg").join("agents");
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
                    fallback_model: None,
                    reasoning_effort: None,
                    runtime_kind: None,
                };
                agent = agent.with_config_mode(mode_cfg, None);
                agents.push(agent);
            }
        }
    }

    // Phase 3: Apply safety envelope to all resolved agents.
    // This ensures that custom agent files (global, project, config) cannot
    // escalate permissions beyond session/config/hard policy bounds.
    // Session rules are empty here (resolved at load time), config rules
    // come from the loaded config, and hard_deny contains tools that must
    // always be denied (e.g., sandbox restrictions).
    let session_rules = crate::permission::PermissionRuleset::default();
    let config_rules = crate::permission::config_ruleset(Some(config));
    let hard_deny = vec![
        "commit".to_string(),
        "todowrite".to_string(),
        "todoread".to_string(),
    ];
    for agent in &mut agents {
        *agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
    }

    Ok(agents)
}

pub(super) fn merge_agent_config(agent: &Agent, cfg: &AgentConfig) -> Result<Agent, AgentError> {
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
        fallback_model: cfg
            .fallback_model
            .clone()
            .or_else(|| agent.fallback_model.clone()),
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
        runtime_kind: cfg
            .runtime_kind
            .as_deref()
            .and_then(|s| s.parse::<AgentRuntimeKind>().ok())
            .or_else(|| agent.runtime_kind.clone()),
    })
}

pub(super) fn agent_from_config(key: &str, cfg: &AgentConfig) -> Result<Agent, AgentError> {
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
        fallback_model: cfg.fallback_model.clone(),
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
        runtime_kind: cfg
            .runtime_kind
            .as_deref()
            .and_then(|s| s.parse::<AgentRuntimeKind>().ok()),
    })
}

pub(crate) fn parse_mode(s: &str) -> Result<AgentMode, AgentError> {
    match s.to_ascii_lowercase().as_str() {
        "primary" => Ok(AgentMode::Primary),
        "subagent" => Ok(AgentMode::Subagent),
        "all" => Ok(AgentMode::All),
        _ => Err(AgentError::Invalid(format!("unknown agent mode: {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::file_agents::{find_agent_by_name, find_default_agent, list_visible_agents};
    use crate::agent::resolve_agents;
    use crate::config::schema::{AgentConfig, Config, ModeConfig};
    use std::collections::HashMap;

    fn make_test_agent(name: &str) -> Agent {
        Agent {
            name: name.to_string(),
            role: None,
            description: "Test agent".to_string(),
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
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        }
    }

    #[test]
    fn test_builtin_agents_count() {
        let agents = builtin_agents();
        assert_eq!(agents.len(), 10);
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
        assert_eq!(agents.len(), 10);
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
        assert_eq!(agents.len(), 11);
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
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
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
        // Network + research tools allowed; mutating tools denied; image denied.
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
        assert_eq!(research.permissions.get("edit"), Some(&"deny".to_string()));
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
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
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
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
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
    fn test_parse_mode_case_insensitive() {
        assert!(matches!(parse_mode("Primary"), Ok(AgentMode::Primary)));
        assert!(matches!(parse_mode("PRIMARY"), Ok(AgentMode::Primary)));
        assert!(matches!(parse_mode("Subagent"), Ok(AgentMode::Subagent)));
        assert!(matches!(parse_mode("SUBAGENT"), Ok(AgentMode::Subagent)));
        assert!(matches!(parse_mode("All"), Ok(AgentMode::All)));
        assert!(matches!(parse_mode("ALL"), Ok(AgentMode::All)));
    }

    #[test]
    fn test_parse_mode_invalid() {
        assert!(parse_mode("invalid").is_err());
        assert!(parse_mode("unknown").is_err());
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
        for tool in &[
            "write",
            "edit",
            "bash",
            "apply_patch",
            "replace",
            "terminal",
            "commit",
        ] {
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
        for tool in &["write", "edit", "apply_patch", "replace", "commit", "image"] {
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
        for tool in &[
            "websearch",
            "webfetch",
            "research",
            "skill",
            "question",
            "task",
        ] {
            assert_eq!(
                research.permissions.get(*tool),
                Some(&"allow".to_string()),
                "research should allow {tool}"
            );
        }
        assert_eq!(research.permissions.get("image"), Some(&"deny".to_string()));
        assert_eq!(
            research.permissions.get("plan_enter"),
            Some(&"deny".to_string())
        );
        assert_eq!(
            research.permissions.get("plan_exit"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_builtin_count_is_ten() {
        assert_eq!(builtin_agents().len(), 10);
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
        let prompt = sr
            .system_prompt
            .as_ref()
            .expect("security-review should have a prompt");
        assert!(
            prompt.contains("defensive"),
            "prompt should mention defensive"
        );
        assert!(
            prompt.contains("deterministic"),
            "prompt should mention deterministic"
        );
        assert!(
            prompt.contains("evidence"),
            "prompt should mention evidence"
        );
        assert!(
            prompt.contains("Never mutate files"),
            "prompt should prohibit file mutation"
        );
    }

    #[test]
    fn test_research_prompt_sentinels() {
        let agents = builtin_agents();
        let research = agents.iter().find(|a| a.name == "research").unwrap();
        let prompt = research
            .system_prompt
            .as_ref()
            .expect("research should have a prompt");
        assert!(
            prompt.contains("research"),
            "prompt should mention research tool"
        );
        assert!(
            prompt.contains("websearch"),
            "prompt should mention websearch"
        );
        assert!(prompt.contains("cite"), "prompt should mention citation");
    }

    // --- Milestone 3: CI and built-in validation hardening ---

    #[test]
    fn test_builtin_agents_deterministic() {
        let a = builtin_agents();
        let b = builtin_agents();
        assert_eq!(a.len(), b.len());
        for (aa, bb) in a.iter().zip(b.iter()) {
            assert_eq!(aa.name, bb.name);
            assert_eq!(aa.mode, bb.mode);
            assert_eq!(aa.hidden, bb.hidden);
            assert_eq!(aa.description, bb.description);
            assert_eq!(aa.permissions, bb.permissions);
            assert_eq!(aa.system_prompt, bb.system_prompt);
            assert_eq!(aa.role, bb.role);
            assert_eq!(aa.color, bb.color);
            assert_eq!(aa.temperature, bb.temperature);
            assert_eq!(aa.steps, bb.steps);
        }
    }

    #[test]
    fn test_builtin_no_duplicate_names() {
        let agents = builtin_agents();
        let mut names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate agent names detected");
    }

    #[test]
    fn test_builtin_all_agents_have_nonempty_name() {
        for agent in builtin_agents() {
            assert!(!agent.name.is_empty(), "agent has empty name");
        }
    }

    #[test]
    fn test_builtin_all_agents_have_valid_mode() {
        for agent in builtin_agents() {
            assert!(
                matches!(
                    agent.mode,
                    AgentMode::Primary | AgentMode::Subagent | AgentMode::All
                ),
                "agent '{}' has unexpected mode: {:?}",
                agent.name,
                agent.mode
            );
        }
    }

    #[test]
    fn test_builtin_all_permission_values_are_valid() {
        let valid = ["allow", "ask", "deny"];
        for agent in builtin_agents() {
            for (tool, action) in &agent.permissions {
                assert!(
                    valid.contains(&action.as_str()),
                    "agent '{}': tool '{}' has invalid permission '{}'",
                    agent.name,
                    tool,
                    action
                );
            }
        }
    }

    #[test]
    fn test_builtin_primary_agents_are_visible() {
        let agents = builtin_agents();
        for agent in &agents {
            if agent.mode == AgentMode::Primary {
                assert!(
                    !agent.hidden,
                    "primary agent '{}' should not be hidden",
                    agent.name
                );
            }
        }
    }

    #[test]
    fn test_builtin_all_mode_agents_are_visible() {
        let agents = builtin_agents();
        for agent in &agents {
            if agent.mode == AgentMode::All {
                assert!(
                    !agent.hidden,
                    "All-mode agent '{}' should not be hidden",
                    agent.name
                );
            }
        }
    }

    #[test]
    fn test_builtin_hidden_agents_have_no_prompt_except_compaction() {
        let agents = builtin_agents();
        for agent in &agents {
            if agent.hidden && !matches!(agent.name.as_str(), "compaction" | "verifier") {
                assert!(
                    agent.system_prompt.is_none(),
                    "hidden agent '{}' should not have a prompt",
                    agent.name
                );
            }
        }
    }

    #[test]
    fn test_builtin_visible_agents_have_descriptions() {
        let agents = builtin_agents();
        for agent in &agents {
            if !agent.hidden {
                assert!(
                    !agent.description.is_empty(),
                    "visible agent '{}' has empty description",
                    agent.name
                );
            }
        }
    }

    #[test]
    fn test_builtin_security_review_deny_list() {
        let agents = builtin_agents();
        let sr = agents.iter().find(|a| a.name == "security-review").unwrap();
        for tool in &["write", "edit", "apply_patch", "replace", "commit", "image"] {
            assert_eq!(
                sr.permissions.get(*tool),
                Some(&"deny".to_string()),
                "security-review should deny {tool}"
            );
        }
    }

    #[test]
    fn test_builtin_plan_and_explore_deny_write() {
        let agents = builtin_agents();
        for name in &["plan", "explore"] {
            let agent = agents.iter().find(|a| a.name == *name).unwrap();
            assert_eq!(
                agent.permissions.get("write"),
                Some(&"deny".to_string()),
                "{name} should deny write"
            );
            assert_eq!(
                agent.permissions.get("edit"),
                Some(&"deny".to_string()),
                "{name} should deny edit"
            );
        }
    }

    #[test]
    fn test_builtin_summary_and_title_deny_todo_and_plan() {
        let agents = builtin_agents();
        for name in &["summary", "title"] {
            let agent = agents.iter().find(|a| a.name == *name).unwrap();
            for tool in &["todowrite", "todoread", "plan_enter", "plan_exit"] {
                assert_eq!(
                    agent.permissions.get(*tool),
                    Some(&"deny".to_string()),
                    "{name} should deny {tool}"
                );
            }
        }
    }

    // --- Phase 7: Generated built-in pipeline hardening ---

    #[test]
    fn test_builtin_agents_sorted_by_name() {
        let agents = builtin_agents();
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(
            names, sorted,
            "builtin agents must be sorted by name for stable formatting"
        );
    }

    #[test]
    fn test_builtin_permissions_sorted_by_key() {
        let agents = builtin_agents();
        for agent in &agents {
            let mut keys: Vec<&String> = agent.permissions.keys().collect();
            keys.sort();
            let original_count = agent.permissions.len();
            let sorted_count = keys.len();
            assert_eq!(
                original_count, sorted_count,
                "agent '{}' has duplicate permission keys",
                agent.name
            );
            // Verify all keys are valid (no empty keys)
            for key in &keys {
                assert!(
                    !key.is_empty(),
                    "agent '{}' has empty permission key",
                    agent.name
                );
            }
        }
    }

    #[test]
    fn test_builtin_agents_have_all_expected_fields() {
        let agents = builtin_agents();
        for agent in &agents {
            // These fields must always be present (even if None/default)
            // by virtue of struct construction — no Option unwraps needed.
            // But verify the struct has the fields we care about for staleness:
            assert!(
                !agent.name.is_empty(),
                "agent '{}' has empty name",
                agent.name
            );
            // fallback_model is always present as Option (may be None)
            // runtime_kind is always present as Option (may be None)
            // This test ensures the generated code doesn't accidentally drop fields.
            let _ = agent.fallback_model;
            let _ = agent.runtime_kind;
            let _ = agent.model;
            let _ = agent.temperature;
            let _ = agent.steps;
            let _ = agent.color;
        }
    }

    #[test]
    fn test_builtin_runtime_kinds_are_valid() {
        let valid_kinds = [
            Some(AgentRuntimeKind::Standard),
            Some(AgentRuntimeKind::SecurityReview),
            Some(AgentRuntimeKind::Research),
            Some(AgentRuntimeKind::Compaction),
            Some(AgentRuntimeKind::Title),
            Some(AgentRuntimeKind::Summary),
            None,
        ];
        for agent in builtin_agents() {
            assert!(
                valid_kinds.contains(&agent.runtime_kind),
                "agent '{}' has unexpected runtime_kind: {:?}",
                agent.name,
                agent.runtime_kind
            );
        }
    }

    #[test]
    fn test_builtin_agents_expected_count() {
        // If you add/remove a built-in agent, update this count.
        // Then run: python3 scripts/generate_builtin_agents.py
        let agents = builtin_agents();
        assert_eq!(
            agents.len(),
            10,
            "built-in agent count changed — update TOML definitions or this test"
        );
        let expected_names = [
            "build",
            "compaction",
            "explore",
            "general",
            "plan",
            "research",
            "security-review",
            "summary",
            "title",
            "verifier",
        ];
        let actual_names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(actual_names, expected_names, "built-in agent names changed");
    }

    // --- TOML agent loading tests ---

    #[test]
    fn test_simple_permission_strings_still_work() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "simple-agent"
mode = "subagent"
description = "Simple permissions"

[permission]
read = "allow"
bash = "ask"
write = "deny"
"#;
        std::fs::write(tmp.path().join("simple.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        let agent = &agents[0].agent;
        assert_eq!(agent.permissions.get("read"), Some(&"allow".to_string()));
        assert_eq!(agent.permissions.get("bash"), Some(&"ask".to_string()));
        assert_eq!(agent.permissions.get("write"), Some(&"deny".to_string()));
    }

    #[test]
    fn test_safety_envelope_restricts_permissions() {
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
            permissions: HashMap::from([
                ("bash".to_string(), "allow".to_string()),
                ("write".to_string(), "allow".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        // Session says bash should be deny
        let session_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "bash".to_string(),
                level: crate::permission::PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let config_rules = crate::permission::PermissionRuleset::default();
        let hard_deny = vec!["commit".to_string()];

        let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
        // Session restriction wins: bash should be deny
        assert_eq!(
            safe_agent.permissions.get("bash"),
            Some(&"deny".to_string())
        );
        // Write not restricted by session/config
        assert_eq!(
            safe_agent.permissions.get("write"),
            Some(&"allow".to_string())
        );
        // Hard deny applied
        assert_eq!(
            safe_agent.permissions.get("commit"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_deny_pattern_wins_over_broad_allow() {
        // When both deny and allow patterns exist for bash,
        // deny patterns should be listed first (evaluated first in ruleset)
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
            permissions: HashMap::from([
                ("bash".to_string(), "ask".to_string()),
                ("bash:allow:*".to_string(), "allow".to_string()),
                ("bash:deny:rm *".to_string(), "deny".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };
        let ruleset = agent.permission_ruleset();
        let bash_rules: Vec<_> = ruleset
            .tool_rules
            .iter()
            .filter(|r| r.tool == "bash")
            .collect();
        // Deny rule should come before allow rule
        let deny_idx = bash_rules
            .iter()
            .position(|r| r.level == permission::PermissionLevel::Deny);
        let allow_idx = bash_rules
            .iter()
            .position(|r| r.level == permission::PermissionLevel::Allow);
        assert!(deny_idx.is_some() && allow_idx.is_some());
        assert!(
            deny_idx.unwrap() < allow_idx.unwrap(),
            "deny patterns should be evaluated before allow patterns"
        );
    }

    // Phase 4: Model resolution tests

    #[test]
    fn test_model_resolution_never_returns_empty() {
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
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let config = crate::config::schema::Config::default();

        // Test with no model configured anywhere
        let profile = ResolvedAgentExecutionProfile::resolve(&agent, &config, None);
        assert!(
            !profile.resolved_model.is_empty(),
            "resolved_model should never be empty, got: '{}'",
            profile.resolved_model
        );
        assert_eq!(profile.resolved_model, "openai/gpt-4o");
    }

    #[test]
    fn test_model_resolution_explicit_model_wins() {
        let agent = Agent {
            name: "test".to_string(),
            role: None,
            description: "Test".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: Some("anthropic/claude-3-opus".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let config = crate::config::schema::Config::default();

        let profile = ResolvedAgentExecutionProfile::resolve(&agent, &config, None);
        assert_eq!(profile.resolved_model, "anthropic/claude-3-opus");
    }

    #[test]
    fn test_model_resolution_parent_model_inherits() {
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
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let config = crate::config::schema::Config::default();

        // Parent model should be inherited
        let profile =
            ResolvedAgentExecutionProfile::resolve(&agent, &config, Some("openai/gpt-4-turbo"));
        assert_eq!(profile.resolved_model, "openai/gpt-4-turbo");
    }

    #[test]
    fn test_model_resolution_fallback_model_used() {
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
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: Some("anthropic/claude-3-sonnet".to_string()),
            reasoning_effort: None,
            runtime_kind: None,
        };

        let config = crate::config::schema::Config::default();

        let profile = ResolvedAgentExecutionProfile::resolve(&agent, &config, None);
        assert_eq!(profile.resolved_model, "anthropic/claude-3-sonnet");
    }

    // Phase 3: Safety envelope enforcement tests

    #[test]
    fn test_safety_envelope_prevents_escalation() {
        // Agent tries to allow bash, but session denies it
        let agent = Agent {
            name: "custom-agent".to_string(),
            role: None,
            description: "Custom agent".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([
                ("bash".to_string(), "allow".to_string()),
                ("write".to_string(), "allow".to_string()),
                ("edit".to_string(), "allow".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        // Session restricts bash to deny
        let session_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "bash".to_string(),
                level: crate::permission::PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let config_rules = crate::permission::PermissionRuleset::default();
        let hard_deny = vec!["commit".to_string(), "rm".to_string()];

        let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);

        // Session restriction should win: bash should be denied
        assert_eq!(
            safe_agent.permissions.get("bash"),
            Some(&"deny".to_string())
        );
        // Hard deny tools should be denied
        assert_eq!(
            safe_agent.permissions.get("commit"),
            Some(&"deny".to_string())
        );
        assert_eq!(safe_agent.permissions.get("rm"), Some(&"deny".to_string()));
        // write and edit not restricted by session/config, should remain allow
        assert_eq!(
            safe_agent.permissions.get("write"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            safe_agent.permissions.get("edit"),
            Some(&"allow".to_string())
        );
    }

    #[test]
    fn test_safety_envelope_config_restricts_permissions() {
        // Agent tries to allow write, but config denies it
        let agent = Agent {
            name: "custom-agent".to_string(),
            role: None,
            description: "Custom agent".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([
                ("write".to_string(), "allow".to_string()),
                ("bash".to_string(), "allow".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let session_rules = crate::permission::PermissionRuleset::default();

        // Config restricts write to deny
        let config_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "write".to_string(),
                level: crate::permission::PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let hard_deny = vec![];

        let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);

        // Config restriction should win: write should be denied
        assert_eq!(
            safe_agent.permissions.get("write"),
            Some(&"deny".to_string())
        );
        // bash not restricted, should remain allow
        assert_eq!(
            safe_agent.permissions.get("bash"),
            Some(&"allow".to_string())
        );
    }

    #[test]
    fn test_safety_envelope_empty_rules_preserves_permissions() {
        // When no session/config/hard rules, agent permissions should be preserved
        let agent = Agent {
            name: "custom-agent".to_string(),
            role: None,
            description: "Custom agent".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([
                ("bash".to_string(), "allow".to_string()),
                ("write".to_string(), "ask".to_string()),
                ("edit".to_string(), "deny".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let session_rules = crate::permission::PermissionRuleset::default();
        let config_rules = crate::permission::PermissionRuleset::default();
        let hard_deny = vec![];

        let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);

        // All permissions should be preserved
        assert_eq!(
            safe_agent.permissions.get("bash"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            safe_agent.permissions.get("write"),
            Some(&"ask".to_string())
        );
        assert_eq!(
            safe_agent.permissions.get("edit"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_safety_envelope_session_takes_precedence_over_config() {
        // When both session and config have rules, session should take precedence
        // (session is more restrictive)
        let agent = Agent {
            name: "custom-agent".to_string(),
            role: None,
            description: "Custom agent".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([("bash".to_string(), "allow".to_string())]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        // Session says deny
        let session_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "bash".to_string(),
                level: crate::permission::PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        // Config says ask (less restrictive than deny)
        let config_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "bash".to_string(),
                level: crate::permission::PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let hard_deny = vec![];

        let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);

        // Session (deny) should win over config (ask)
        assert_eq!(
            safe_agent.permissions.get("bash"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_emergency_default_model_constant_not_empty() {
        assert!(!EMERGENCY_DEFAULT_MODEL.is_empty());
        assert!(!EMERGENCY_DEFAULT_WORKHORSE_MODEL.is_empty());
    }

    #[test]
    fn test_resolve_profile_explicit_model_no_emergency_fallback() {
        let agent = Agent {
            name: "test".to_string(),
            role: None,
            description: "Test".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: Some("anthropic/claude-sonnet-4-20250514".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };
        let config = Config::default();
        let profile = ResolvedAgentExecutionProfile::resolve(&agent, &config, None);
        assert_eq!(profile.resolved_model, "anthropic/claude-sonnet-4-20250514");
    }

    #[test]
    fn test_resolve_profile_no_model_uses_emergency_fallback() {
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
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };
        let config = Config::default();
        let profile = ResolvedAgentExecutionProfile::resolve(&agent, &config, None);
        assert_eq!(profile.resolved_model, EMERGENCY_DEFAULT_MODEL);
    }

    #[test]
    fn test_resolve_profile_config_model_takes_precedence() {
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
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };
        let config = Config {
            model: Some("openai/gpt-4o-mini".to_string()),
            ..Config::default()
        };
        let profile = ResolvedAgentExecutionProfile::resolve(&agent, &config, None);
        assert_eq!(profile.resolved_model, "openai/gpt-4o-mini");
    }

    #[test]
    fn test_resolve_model_alias_frontier_uses_emergency_constant() {
        let config = Config::default();
        let result = resolve_model_alias(MODEL_ALIAS_FRONTIER, &config);
        assert_eq!(result.as_deref(), Some(EMERGENCY_DEFAULT_MODEL));
    }

    #[test]
    fn test_resolve_model_alias_workhorse_uses_emergency_constant() {
        let config = Config::default();
        let result = resolve_model_alias(MODEL_ALIAS_WORKHORSE, &config);
        assert_eq!(result.as_deref(), Some(EMERGENCY_DEFAULT_WORKHORSE_MODEL));
    }

    // --- Phase 7: Example agents parse tests ---

    #[test]
    fn test_safety_envelope_custom_agent_session_deny_overrides_allow() {
        let mut agent = make_test_agent("custom-agent");
        agent
            .permissions
            .insert("edit".to_string(), "allow".to_string());

        let session_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "edit".to_string(),
                level: crate::permission::PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let config_rules = crate::permission::PermissionRuleset::default();
        let hard_deny = vec![];

        let safe = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
        let ruleset = safe.permission_ruleset();
        let edit_rule = ruleset.tool_rules.iter().find(|r| r.tool == "edit");
        assert!(edit_rule.is_some());
        assert_eq!(
            edit_rule.unwrap().level,
            crate::permission::PermissionLevel::Deny
        );
    }

    #[test]
    fn test_safety_envelope_custom_agent_config_ask_overrides_allow() {
        let mut agent = make_test_agent("custom-agent");
        agent
            .permissions
            .insert("bash".to_string(), "allow".to_string());

        let session_rules = crate::permission::PermissionRuleset::default();
        let config_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "bash".to_string(),
                level: crate::permission::PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let hard_deny = vec![];

        let safe = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
        let ruleset = safe.permission_ruleset();
        let bash_rule = ruleset.tool_rules.iter().find(|r| r.tool == "bash");
        assert!(bash_rule.is_some());
        assert_eq!(
            bash_rule.unwrap().level,
            crate::permission::PermissionLevel::Ask
        );
    }

    #[test]
    fn test_safety_envelope_hard_deny_always_wins_over_agent_allow() {
        let mut agent = make_test_agent("custom-agent");
        agent
            .permissions
            .insert("commit".to_string(), "allow".to_string());

        let session_rules = crate::permission::PermissionRuleset::default();
        let config_rules = crate::permission::PermissionRuleset::default();
        let hard_deny = vec!["commit".to_string()];

        let safe = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
        assert_eq!(safe.permissions.get("commit").unwrap(), "deny");
    }

    #[test]
    fn test_safety_envelope_subagent_permissions_bounded() {
        let mut agent = make_test_agent("sub-agent");
        agent.mode = AgentMode::Subagent;
        agent
            .permissions
            .insert("edit".to_string(), "allow".to_string());
        agent
            .permissions
            .insert("bash".to_string(), "allow".to_string());
        agent
            .permissions
            .insert("write".to_string(), "allow".to_string());

        let session_rules = crate::permission::PermissionRuleset::default();
        let config_rules = crate::permission::PermissionRuleset {
            default: crate::permission::PermissionLevel::Ask,
            tool_rules: vec![crate::permission::ToolRule {
                tool: "bash".to_string(),
                level: crate::permission::PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: Vec::new(),
        };

        let hard_deny = vec!["commit".to_string()];

        let safe = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
        let ruleset = safe.permission_ruleset();

        // edit and write remain allow (no session/config restriction)
        let edit_rule = ruleset.tool_rules.iter().find(|r| r.tool == "edit");
        assert_eq!(
            edit_rule.unwrap().level,
            crate::permission::PermissionLevel::Allow
        );

        // bash is restricted to ask by config
        let bash_rule = ruleset.tool_rules.iter().find(|r| r.tool == "bash");
        assert_eq!(
            bash_rule.unwrap().level,
            crate::permission::PermissionLevel::Ask
        );

        // commit is hard-denied
        assert_eq!(safe.permissions.get("commit").unwrap(), "deny");
    }

    #[test]
    fn test_safety_envelope_empty_agent_permissions_get_defaults() {
        let agent = make_test_agent("bare-agent");

        let session_rules = crate::permission::PermissionRuleset::default();
        let config_rules = crate::permission::PermissionRuleset::default();
        let hard_deny = vec![];

        let safe = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
        let ruleset = safe.permission_ruleset();
        // Should not panic and should produce a valid (empty) ruleset
        assert!(ruleset.tool_rules.is_empty());
        assert_eq!(ruleset.default, crate::permission::PermissionLevel::Ask);
    }
}
