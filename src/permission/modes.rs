use crate::config::schema::ModeConfig;
use crate::permission::{PermissionLevel, PermissionRuleset, ToolRule};

#[derive(Debug, Clone)]
pub struct ModeDefinition {
    pub name: String,
    pub description: String,
    pub default: PermissionLevel,
    pub allowed_tools: Vec<String>,
    pub restricted_tools: Vec<String>,
    pub tool_overrides: Vec<(String, PermissionLevel)>,
}

impl ModeDefinition {
    pub fn to_ruleset(&self) -> PermissionRuleset {
        let mut tool_rules = Vec::new();

        for tool in &self.allowed_tools {
            if !self.restricted_tools.contains(tool) {
                tool_rules.push(ToolRule {
                    tool: tool.clone(),
                    level: PermissionLevel::Allow,
                    paths: None,
                    bash_patterns: None,
                });
            }
        }

        for tool in &self.restricted_tools {
            tool_rules.push(ToolRule {
                tool: tool.clone(),
                level: PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            });
        }

        for (tool, level) in &self.tool_overrides {
            tool_rules.push(ToolRule {
                tool: tool.clone(),
                level: level.clone(),
                paths: None,
                bash_patterns: None,
            });
        }

        PermissionRuleset {
            default: self.default.clone(),
            tool_rules,
            path_rules: Vec::new(),
        }
    }

    pub fn from_config(config: &ModeConfig, base: Option<&PermissionRuleset>) -> Self {
        let mut tool_overrides = Vec::new();

        if let Some(tools) = &config.tools {
            for (tool, level_str) in tools {
                let level = crate::permission::parse_level(level_str);
                tool_overrides.push((tool.clone(), level));
            }
        }

        let default = config
            .default
            .as_deref()
            .map(crate::permission::parse_level)
            .unwrap_or(PermissionLevel::Ask);

        let mut merged_overrides = tool_overrides.clone();

        if config.inherit.unwrap_or(false) {
            if let Some(base_ruleset) = base {
                for rule in &base_ruleset.tool_rules {
                    if !merged_overrides.iter().any(|(t, _)| *t == rule.tool) {
                        merged_overrides.push((rule.tool.clone(), rule.level.clone()));
                    }
                }
            }
        }

        let allowed: Vec<String> = merged_overrides
            .iter()
            .filter(|(_, level)| matches!(level, PermissionLevel::Allow))
            .map(|(tool, _)| tool.clone())
            .collect();

        let restricted: Vec<String> = merged_overrides
            .iter()
            .filter(|(_, level)| matches!(level, PermissionLevel::Deny))
            .map(|(tool, _)| tool.clone())
            .collect();

        Self {
            name: "custom".to_string(),
            description: config.description.clone().unwrap_or_default(),
            default,
            allowed_tools: allowed,
            restricted_tools: restricted,
            tool_overrides: merged_overrides,
        }
    }
}

pub struct BuiltinModes;
impl BuiltinModes {
    pub fn review() -> ModeDefinition {
        ModeDefinition {
            name: "review".to_string(),
            description: "Code review mode - read-heavy, limited write access".to_string(),
            default: PermissionLevel::Ask,
            allowed_tools: vec![
                "read".to_string(),
                "glob".to_string(),
                "grep".to_string(),
                "list".to_string(),
                "question".to_string(),
                "webfetch".to_string(),
                "websearch".to_string(),
                "codesearch".to_string(),
                "lsp".to_string(),
            ],
            restricted_tools: vec![
                "edit".to_string(),
                "write".to_string(),
                "bash".to_string(),
                "task".to_string(),
                "todowrite".to_string(),
            ],
            tool_overrides: vec![],
        }
    }

    pub fn debug() -> ModeDefinition {
        ModeDefinition {
            name: "debug".to_string(),
            description: "Debug mode - bash allowed, limited edit".to_string(),
            default: PermissionLevel::Allow,
            allowed_tools: vec![
                "read".to_string(),
                "glob".to_string(),
                "grep".to_string(),
                "list".to_string(),
                "bash".to_string(),
                "question".to_string(),
                "webfetch".to_string(),
                "websearch".to_string(),
                "codesearch".to_string(),
                "edit".to_string(),
                "lsp".to_string(),
            ],
            restricted_tools: vec!["task".to_string(), "todowrite".to_string()],
            tool_overrides: vec![],
        }
    }

    pub fn docs() -> ModeDefinition {
        ModeDefinition {
            name: "docs".to_string(),
            description: "Documentation mode - edit/read allowed, no bash".to_string(),
            default: PermissionLevel::Ask,
            allowed_tools: vec![
                "read".to_string(),
                "glob".to_string(),
                "grep".to_string(),
                "list".to_string(),
                "question".to_string(),
                "webfetch".to_string(),
                "websearch".to_string(),
                "codesearch".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "lsp".to_string(),
            ],
            restricted_tools: vec![
                "bash".to_string(),
                "task".to_string(),
                "todowrite".to_string(),
            ],
            tool_overrides: vec![],
        }
    }
}

pub fn get_builtin_mode(name: &str) -> Option<ModeDefinition> {
    match name {
        "review" => Some(BuiltinModes::review()),
        "debug" => Some(BuiltinModes::debug()),
        "docs" => Some(BuiltinModes::docs()),
        _ => None,
    }
}

pub fn mode_ruleset(
    mode_config: &ModeConfig,
    base: Option<&PermissionRuleset>,
) -> PermissionRuleset {
    ModeDefinition::from_config(mode_config, base).to_ruleset()
}
