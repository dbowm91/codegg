use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::config::schema::Config;
use crate::error::AgentError;

use super::{
    builtin_agents, load_agents_from_dir, Agent, AgentMode,
    agent_from_config, merge_agent_config,
};

/// Declarative agent source representation for future TOML/MD agents.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub mode: Option<AgentMode>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub prompt: Option<String>,
    pub prompt_file: Option<String>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub hidden: Option<bool>,
    pub disable: Option<bool>,
    pub permission: Option<HashMap<String, String>>,
    pub options: BTreeMap<String, toml::Value>,
}

/// Tracks where a resolved agent came from.
#[derive(Debug, Clone)]
pub struct AgentSource {
    pub kind: AgentSourceKind,
    pub path: Option<PathBuf>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSourceKind {
    Builtin,
    GlobalFile,
    ProjectFile,
    ConfigAgent,
    ConfigMode,
    Session,
}

/// Issues found during resolution.
#[derive(Debug, Clone)]
pub struct AgentDiagnostic {
    pub severity: AgentDiagnosticSeverity,
    pub agent_name: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// An agent with provenance tracking.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub agent: Agent,
    pub sources: Vec<AgentSource>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

/// Central registry that resolves and indexes agents with source provenance.
#[derive(Debug)]
pub struct AgentRegistry {
    resolved: BTreeMap<String, ResolvedAgent>,
    diagnostics: Vec<AgentDiagnostic>,
}

impl AgentRegistry {
    /// Load agents by replicating the exact resolution logic from `resolve_agents()`,
    /// but tracking source provenance and emitting diagnostics.
    pub fn load(config: &Config) -> Result<Self, AgentError> {
        let mut resolved: BTreeMap<String, ResolvedAgent> = BTreeMap::new();
        let mut diagnostics: Vec<AgentDiagnostic> = Vec::new();

        // Layer 1: Compiled generated built-ins
        for agent in builtin_agents() {
            let name = agent.name.clone();
            let ra = ResolvedAgent {
                agent,
                sources: vec![AgentSource {
                    kind: AgentSourceKind::Builtin,
                    path: None,
                    name: name.clone(),
                }],
                diagnostics: Vec::new(),
            };
            resolved.insert(name, ra);
        }

        // Layer 2: Global user agent files (~/.config/codegg/agents/*.toml)
        // Overlay merge by default; replace=true replaces the entire definition.
        if let Some(config_dir) = dirs::config_dir() {
            let agents_dir = config_dir.join("codegg").join("agents");
            if let Ok(file_agents) = load_agents_from_dir(&agents_dir) {
                for file_agent in file_agents {
                    let name = file_agent.agent.name.clone();
                    let path = PathBuf::from(file_agent.source.clone());
                    let replace = file_agent.overlay.replace.unwrap_or(false);
                    let disable = file_agent.overlay.disable.unwrap_or(false);

                    if disable {
                        // Remove agent from resolution
                        resolved.remove(&name);
                        diagnostics.push(AgentDiagnostic {
                            severity: AgentDiagnosticSeverity::Info,
                            agent_name: name.clone(),
                            message: format!(
                                "agent '{name}' disabled by global file overlay"
                            ),
                        });
                        continue;
                    }

                    if let Some(existing) = resolved.get_mut(&name) {
                        let merged = existing.agent.merge_overlay(&file_agent.agent, replace);
                        let diag_severity = if replace {
                            AgentDiagnosticSeverity::Warning
                        } else {
                            AgentDiagnosticSeverity::Info
                        };
                        let action = if replace { "replaces" } else { "merged into" };
                        diagnostics.push(AgentDiagnostic {
                            severity: diag_severity,
                            agent_name: name.clone(),
                            message: format!(
                                "global file overlay {action} built-in {name}"
                            ),
                        });
                        existing.agent = merged;
                        existing.sources.push(AgentSource {
                            kind: AgentSourceKind::GlobalFile,
                            path: Some(path),
                            name: name.clone(),
                        });
                    } else {
                        resolved.insert(
                            name.clone(),
                            ResolvedAgent {
                                agent: file_agent.agent,
                                sources: vec![AgentSource {
                                    kind: AgentSourceKind::GlobalFile,
                                    path: Some(path),
                                    name: name,
                                }],
                                diagnostics: Vec::new(),
                            },
                        );
                    }
                }
            }
        }

        // Layer 3: Project agent files (.codegg/agents/*.toml relative to PWD)
        // Overlay merge by default; replace=true replaces the entire definition.
        if let Some(project_dir) = std::env::var("PWD").ok().filter(|p| !p.is_empty()) {
            let project_agents_dir = PathBuf::from(&project_dir)
                .join(".codegg")
                .join("agents");
            if let Ok(file_agents) = load_agents_from_dir(&project_agents_dir) {
                for file_agent in file_agents {
                    let name = file_agent.agent.name.clone();
                    let path = PathBuf::from(file_agent.source.clone());
                    let replace = file_agent.overlay.replace.unwrap_or(false);
                    let disable = file_agent.overlay.disable.unwrap_or(false);

                    if disable {
                        resolved.remove(&name);
                        diagnostics.push(AgentDiagnostic {
                            severity: AgentDiagnosticSeverity::Info,
                            agent_name: name.clone(),
                            message: format!(
                                "agent '{name}' disabled by project file overlay"
                            ),
                        });
                        continue;
                    }

                    if let Some(existing) = resolved.get_mut(&name) {
                        let merged = existing.agent.merge_overlay(&file_agent.agent, replace);
                        let diag_severity = if replace {
                            AgentDiagnosticSeverity::Warning
                        } else {
                            AgentDiagnosticSeverity::Info
                        };
                        let action = if replace { "replaces" } else { "merged into" };
                        diagnostics.push(AgentDiagnostic {
                            severity: diag_severity,
                            agent_name: name.clone(),
                            message: format!(
                                "project file overlay {action} existing agent {name}"
                            ),
                        });
                        existing.agent = merged;
                        existing.sources.push(AgentSource {
                            kind: AgentSourceKind::ProjectFile,
                            path: Some(path),
                            name: name.clone(),
                        });
                    } else {
                        resolved.insert(
                            name.clone(),
                            ResolvedAgent {
                                agent: file_agent.agent,
                                sources: vec![AgentSource {
                                    kind: AgentSourceKind::ProjectFile,
                                    path: Some(path),
                                    name: name,
                                }],
                                diagnostics: Vec::new(),
                            },
                        );
                    }
                }
            }
        }

        // Layer 4: Config `agent` overrides (merges or adds, skips disabled)
        if let Some(agent_map) = &config.agent {
            for (key, agent_cfg) in agent_map {
                if agent_cfg.disable == Some(true) {
                    diagnostics.push(AgentDiagnostic {
                        severity: AgentDiagnosticSeverity::Info,
                        agent_name: key.clone(),
                        message: format!("agent '{key}' is disabled in config, skipping override"),
                    });
                    continue;
                }

                if let Some(existing) = resolved.get_mut(key) {
                    let merged = merge_agent_config(&existing.agent, agent_cfg)?;
                    let mode_diagnostic = if let Some(ref mode_str) = agent_cfg.mode {
                        match super::parse_mode(mode_str) {
                            Err(_) => Some(AgentDiagnostic {
                                severity: AgentDiagnosticSeverity::Error,
                                agent_name: key.clone(),
                                message: format!("invalid mode: {mode_str}"),
                            }),
                            Ok(_) => None,
                        }
                    } else {
                        None
                    };
                    existing.agent = merged;
                    existing.sources.push(AgentSource {
                        kind: AgentSourceKind::ConfigAgent,
                        path: None,
                        name: key.clone(),
                    });
                    if let Some(diag) = mode_diagnostic {
                        existing.diagnostics.push(diag);
                        diagnostics.push(AgentDiagnostic {
                            severity: AgentDiagnosticSeverity::Error,
                            agent_name: key.clone(),
                            message: format!("invalid mode ignored during resolution: {mode_str}", mode_str = agent_cfg.mode.as_deref().unwrap_or("unknown")),
                        });
                    }
                } else {
                    match agent_from_config(key, agent_cfg) {
                        Ok(agent) => {
                            let agent_name = agent.name.clone();
                            let mode_diagnostic = if let Some(ref mode_str) = agent_cfg.mode {
                                match super::parse_mode(mode_str) {
                                    Err(_) => Some(AgentDiagnostic {
                                        severity: AgentDiagnosticSeverity::Error,
                                        agent_name: agent_name.clone(),
                                        message: format!("invalid mode: {mode_str}"),
                                    }),
                                    Ok(_) => None,
                                }
                            } else {
                                None
                            };
                            let mut agent_diags = Vec::new();
                            if let Some(diag) = mode_diagnostic {
                                agent_diags.push(diag);
                                diagnostics.push(AgentDiagnostic {
                                    severity: AgentDiagnosticSeverity::Error,
                                    agent_name: agent_name.clone(),
                                    message: format!("invalid mode ignored during resolution: {mode_str}", mode_str = agent_cfg.mode.as_deref().unwrap_or("unknown")),
                                });
                            }
                            resolved.insert(
                                agent_name.clone(),
                                ResolvedAgent {
                                    agent,
                                    sources: vec![AgentSource {
                                        kind: AgentSourceKind::ConfigAgent,
                                        path: None,
                                        name: agent_name,
                                    }],
                                    diagnostics: agent_diags,
                                },
                            );
                        }
                        Err(e) => {
                            diagnostics.push(AgentDiagnostic {
                                severity: AgentDiagnosticSeverity::Error,
                                agent_name: key.clone(),
                                message: format!("failed to create agent: {e}"),
                            });
                        }
                    }
                }
            }
        }

        // Layer 5: Config `mode` compatibility overrides (supports inherit)
        if let Some(mode_map) = &config.mode {
            for (key, mode_cfg) in mode_map {
                if mode_cfg.inherit.unwrap_or(false) {
                    if let Some(existing) = resolved.get_mut(key) {
                        let base_ruleset = existing.agent.permission_ruleset();
                        let _mode_ruleset =
                            crate::permission::modes::mode_ruleset(mode_cfg, Some(&base_ruleset));
                        existing.agent = existing.agent.clone().with_config_mode(mode_cfg, Some(&base_ruleset));
                        existing.sources.push(AgentSource {
                            kind: AgentSourceKind::ConfigMode,
                            path: None,
                            name: key.clone(),
                        });
                    } else {
                        // inherit=true but agent doesn't exist yet: create it
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
                        resolved.insert(
                            key.clone(),
                            ResolvedAgent {
                                agent,
                                sources: vec![AgentSource {
                                    kind: AgentSourceKind::ConfigMode,
                                    path: None,
                                    name: key.clone(),
                                }],
                                diagnostics: Vec::new(),
                            },
                        );
                    }
                } else if let Some(existing) = resolved.get_mut(key) {
                    existing.agent = existing.agent.clone().with_config_mode(mode_cfg, None);
                    existing.sources.push(AgentSource {
                        kind: AgentSourceKind::ConfigMode,
                        path: None,
                        name: key.clone(),
                    });
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
                    resolved.insert(
                        key.clone(),
                        ResolvedAgent {
                            agent,
                            sources: vec![AgentSource {
                                kind: AgentSourceKind::ConfigMode,
                                path: None,
                                name: key.clone(),
                            }],
                            diagnostics: Vec::new(),
                        },
                    );
                }
            }
        }

        Ok(AgentRegistry {
            resolved,
            diagnostics,
        })
    }

    /// Get a resolved agent by name.
    pub fn get(&self, name: &str) -> Option<&ResolvedAgent> {
        self.resolved.get(name)
    }

    /// Iterate over all resolved agents (deterministic BTreeMap order).
    pub fn list(&self) -> impl Iterator<Item = &ResolvedAgent> {
        self.resolved.values()
    }

    /// Return all non-hidden agents.
    pub fn list_visible(&self) -> Vec<&ResolvedAgent> {
        self.resolved.values().filter(|ra| !ra.agent.hidden).collect()
    }

    /// Return agents with Primary or All mode.
    pub fn list_primary(&self) -> Vec<&ResolvedAgent> {
        self.resolved
            .values()
            .filter(|ra| matches!(ra.agent.mode, AgentMode::Primary | AgentMode::All))
            .collect()
    }

    /// Return agents with Subagent or All mode (spawnable via `task`).
    pub fn list_spawnable(&self) -> Vec<&ResolvedAgent> {
        self.resolved
            .values()
            .filter(|ra| matches!(ra.agent.mode, AgentMode::Subagent | AgentMode::All))
            .collect()
    }

    /// Return all diagnostics emitted during resolution.
    pub fn diagnostics(&self) -> &[AgentDiagnostic] {
        &self.diagnostics
    }

    /// Return the source stack for a named agent.
    pub fn source_stack(&self, name: &str) -> Option<&[AgentSource]> {
        self.resolved.get(name).map(|ra| ra.sources.as_slice())
    }

    /// Convert into a plain Vec<Agent> for backward compatibility.
    pub fn into_agents(self) -> Vec<Agent> {
        self.resolved.into_values().map(|ra| ra.agent).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::Config;

    #[test]
    fn test_registry_loads_builtins() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        assert_eq!(registry.list().count(), 9);
    }

    #[test]
    fn test_registry_returns_visible_agents() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let visible = registry.list_visible();
        assert_eq!(visible.len(), 6);
        assert!(visible.iter().all(|ra| !ra.agent.hidden));
    }

    #[test]
    fn test_registry_returns_primary_agents() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let primary = registry.list_primary();
        assert!(primary
            .iter()
            .all(|ra| ra.agent.mode == AgentMode::Primary || ra.agent.mode == AgentMode::All));
    }

    #[test]
    fn test_registry_returns_spawnable_agents() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let spawnable = registry.list_spawnable();
        assert!(spawnable
            .iter()
            .all(|ra| ra.agent.mode == AgentMode::Subagent || ra.agent.mode == AgentMode::All));
    }

    #[test]
    fn test_registry_source_stack_for_builtin() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let sources = registry.source_stack("build").unwrap();
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].kind, AgentSourceKind::Builtin));
    }

    #[test]
    fn test_registry_into_agents_equivalent() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let agents = registry.into_agents();
        assert_eq!(agents.len(), 9);
    }

    #[test]
    fn test_registry_diagnostics_empty_for_default() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        assert!(registry.diagnostics().is_empty());
    }

    #[test]
    fn test_registry_get_agent() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let build = registry.get("build").unwrap();
        assert_eq!(build.agent.name, "build");
        assert!(matches!(build.sources[0].kind, AgentSourceKind::Builtin));
    }

    #[test]
    fn test_registry_config_agent_merge() {
        use crate::config::schema::AgentConfig;
        use std::collections::HashMap;

        let mut agent_map = HashMap::new();
        agent_map.insert(
            "build".to_string(),
            AgentConfig {
                description: Some("Custom builder".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agent_map),
            ..Default::default()
        };
        let registry = AgentRegistry::load(&config).unwrap();
        let build = registry.get("build").unwrap();
        assert_eq!(build.agent.description, "Custom builder");
        assert_eq!(build.sources.len(), 2);
        assert!(matches!(build.sources[0].kind, AgentSourceKind::Builtin));
        assert!(matches!(build.sources[1].kind, AgentSourceKind::ConfigAgent));
    }

    #[test]
    fn test_registry_config_agent_adds_new() {
        use crate::config::schema::AgentConfig;
        use std::collections::HashMap;

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
        let registry = AgentRegistry::load(&config).unwrap();
        let reviewer = registry.get("Reviewer").unwrap();
        assert_eq!(reviewer.agent.mode, AgentMode::Primary);
        assert!(matches!(reviewer.sources[0].kind, AgentSourceKind::ConfigAgent));
    }

    #[test]
    fn test_registry_disabled_agent_skipped() {
        use crate::config::schema::AgentConfig;
        use std::collections::HashMap;

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
        let registry = AgentRegistry::load(&config).unwrap();
        // Disabled agent still exists (builtin stays), but config override was skipped
        let build = registry.get("build").unwrap();
        assert_eq!(build.agent.description, "Default agent with full permissions");
        // But diagnostics should record the disabled info
        assert!(registry
            .diagnostics()
            .iter()
            .any(|d| d.agent_name == "build"
                && d.severity == AgentDiagnosticSeverity::Info
                && d.message.contains("disabled in config")));
    }

    #[test]
    fn test_registry_invalid_mode_emits_diagnostic() {
        use crate::config::schema::AgentConfig;
        use std::collections::HashMap;

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
        let registry = AgentRegistry::load(&config).unwrap();
        // Should still resolve (error diagnostic recorded)
        assert!(registry
            .diagnostics()
            .iter()
            .any(|d| d.agent_name == "bad"
                && d.severity == AgentDiagnosticSeverity::Error
                && d.message.contains("unknown agent mode")));
    }

    #[test]
    fn test_registry_mode_config_creates_agent() {
        use crate::config::schema::ModeConfig;
        use std::collections::HashMap;

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
        let registry = AgentRegistry::load(&config).unwrap();
        let review = registry.get("review");
        assert!(review.is_some());
        let review = review.unwrap();
        assert!(matches!(review.sources[0].kind, AgentSourceKind::ConfigMode));
    }

    #[test]
    fn test_registry_list_deterministic_order() {
        let config = Config::default();
        let registry = AgentRegistry::load(&config).unwrap();
        let names: Vec<&str> = registry.list().map(|ra| ra.agent.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
