use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::config::schema::{AgentConfig, Config};
use crate::error::AgentError;

use super::{
    builtin_agents, load_agents_from_dir, Agent, AgentMode, AgentRuntimeKind,
    agent_from_config, merge_agent_config, parse_mode,
};

/// Declarative agent source representation for future TOML/MD agents.
///
/// All fields are `Option` to preserve explicitness during overlay merges.
/// An explicit `Some(false)` on `hidden` should override a base `Some(true)`.
#[derive(Debug, Clone, Default)]
pub struct AgentSpec {
    pub name: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub mode: Option<AgentMode>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub prompt: Option<String>,
    pub prompt_file: Option<String>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub hidden: Option<bool>,
    pub disable: Option<bool>,
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
    pub runtime_kind: Option<AgentRuntimeKind>,
    pub permission: Option<HashMap<String, String>>,
    pub options: BTreeMap<String, toml::Value>,
}

impl AgentSpec {
    /// Create an AgentSpec from an AgentConfig, preserving which fields were
    /// explicitly set (all `Option` fields come through as-is).
    pub fn from_agent_config(name: &str, cfg: &AgentConfig) -> Result<Self, AgentError> {
        let mode = cfg
            .mode
            .as_deref()
            .map(parse_mode)
            .transpose()?;

        let permission = cfg.permission.as_ref().map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let value = match v {
                        crate::config::schema::PermissionRule::Action(s) => s.clone(),
                        crate::config::schema::PermissionRule::Object(obj) => obj
                            .get("default")
                            .or_else(|| obj.get("action"))
                            .cloned()
                            .unwrap_or_else(|| "ask".to_string()),
                    };
                    (k.clone(), value)
                })
                .collect()
        });

        let runtime_kind = cfg
            .runtime_kind
            .as_deref()
            .map(|s| s.parse::<AgentRuntimeKind>())
            .transpose()
            .map_err(|e| AgentError::Invalid(e))?;

        Ok(AgentSpec {
            name: Some(cfg.name.clone().unwrap_or_else(|| name.to_string())),
            role: cfg.role.clone(),
            description: cfg.description.clone(),
            mode,
            model: cfg.model.clone(),
            fallback_model: cfg.fallback_model.clone(),
            variant: cfg.variant.clone(),
            temperature: cfg.temperature,
            top_p: cfg.top_p,
            prompt: cfg.prompt.clone(),
            prompt_file: cfg.prompt_file.clone(),
            color: cfg.color.clone(),
            steps: cfg.steps,
            hidden: cfg.hidden,
            disable: cfg.disable,
            thinking_budget: None,
            reasoning_effort: None,
            runtime_kind,
            permission,
            options: BTreeMap::new(),
        })
    }

    /// Create an AgentSpec from a concrete Agent, wrapping all fields in Some.
    /// Used to capture the base agent's state before merging overlays.
    pub fn from_agent(agent: &Agent) -> Self {
        AgentSpec {
            name: Some(agent.name.clone()),
            role: agent.role.clone(),
            description: Some(agent.description.clone()),
            mode: Some(agent.mode.clone()),
            model: agent.model.clone(),
            fallback_model: agent.fallback_model.clone(),
            variant: agent.variant.clone(),
            temperature: agent.temperature,
            top_p: agent.top_p,
            prompt: None,
            prompt_file: None,
            color: agent.color.clone(),
            steps: agent.steps.map(|s| s as u32),
            hidden: Some(agent.hidden),
            disable: None,
            thinking_budget: agent.thinking_budget,
            reasoning_effort: agent.reasoning_effort.clone(),
            runtime_kind: agent.runtime_kind.clone(),
            permission: Some(agent.permissions.clone()),
            options: BTreeMap::new(),
        }
    }

    /// Deterministic overlay merge: overlay fields take precedence only when
    /// explicitly set (Some). Scalar fields replace only when the overlay
    /// has them. `replace=true` discards the base entirely.
    pub fn merge_overlay(&self, overlay: &AgentSpec, replace: bool) -> AgentSpec {
        if replace {
            return overlay.clone();
        }

        AgentSpec {
            name: overlay.name.clone().or_else(|| self.name.clone()),
            role: overlay.role.clone().or_else(|| self.role.clone()),
            description: overlay.description.clone().or_else(|| self.description.clone()),
            mode: overlay.mode.clone().or_else(|| self.mode.clone()),
            model: overlay.model.clone().or_else(|| self.model.clone()),
            fallback_model: overlay.fallback_model.clone().or_else(|| self.fallback_model.clone()),
            variant: overlay.variant.clone().or_else(|| self.variant.clone()),
            temperature: overlay.temperature.or(self.temperature),
            top_p: overlay.top_p.or(self.top_p),
            prompt: overlay.prompt.clone().or_else(|| self.prompt.clone()),
            prompt_file: overlay.prompt_file.clone().or_else(|| self.prompt_file.clone()),
            color: overlay.color.clone().or_else(|| self.color.clone()),
            steps: overlay.steps.or(self.steps),
            hidden: overlay.hidden.or(self.hidden),
            disable: overlay.disable.or(self.disable),
            thinking_budget: overlay.thinking_budget.or(self.thinking_budget),
            reasoning_effort: overlay.reasoning_effort.clone().or_else(|| self.reasoning_effort.clone()),
            runtime_kind: overlay.runtime_kind.clone().or_else(|| self.runtime_kind.clone()),
            permission: overlay.permission.clone().or_else(|| self.permission.clone()),
            options: if !overlay.options.is_empty() {
                overlay.options.clone()
            } else {
                self.options.clone()
            },
        }
    }

    /// Resolve this spec against a base Agent to produce a concrete Agent.
    /// Each field uses the spec value if set, otherwise the base value.
    pub fn resolve(&self, base: &Agent) -> Result<Agent, AgentError> {
        let name = self.name.clone().unwrap_or_else(|| base.name.clone());
        let mode = self
            .mode
            .clone()
            .unwrap_or_else(|| base.mode.clone());

        Ok(Agent {
            name: name.clone(),
            role: self.role.clone().or_else(|| base.role.clone()),
            description: self.description.clone().unwrap_or_else(|| base.description.clone()),
            mode,
            mode_name: None,
            model: self.model.clone().or_else(|| base.model.clone()),
            fallback_model: self.fallback_model.clone().or_else(|| base.fallback_model.clone()),
            variant: self.variant.clone().or_else(|| base.variant.clone()),
            temperature: self.temperature.or(base.temperature),
            top_p: self.top_p.or(base.top_p),
            color: self.color.clone().or_else(|| base.color.clone()),
            steps: self.steps.map(|s| s as usize).or(base.steps),
            system_prompt: self.prompt.clone().or_else(|| base.system_prompt.clone()),
            permissions: self.permission.clone().unwrap_or_else(|| base.permissions.clone()),
            hidden: self.hidden.unwrap_or(base.hidden),
            thinking_budget: self.thinking_budget.or(base.thinking_budget),
            reasoning_effort: self.reasoning_effort.clone().or_else(|| base.reasoning_effort.clone()),
            runtime_kind: self.runtime_kind.clone().or_else(|| base.runtime_kind.clone()),
        })
    }
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
    pub source: Option<AgentSourceKind>,
    pub field: Option<String>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl AgentDiagnostic {
    pub fn new(
        severity: AgentDiagnosticSeverity,
        agent_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            agent_name: agent_name.into(),
            message: message.into(),
            source: None,
            field: None,
            suggestion: None,
        }
    }

    pub fn with_source(mut self, source: AgentSourceKind) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
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
                            source: Some(AgentSourceKind::GlobalFile),
                            field: None,
                            suggestion: None,
                        });
                        continue;
                    }

                    if let Some(existing) = resolved.get_mut(&name) {
                        let base_spec = super::registry::AgentSpec::from_agent(&existing.agent);
                        let merged_spec = base_spec.merge_overlay(&file_agent.spec, replace);
                        let merged_agent = merged_spec.resolve(&existing.agent)?;
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
                            source: Some(AgentSourceKind::GlobalFile),
                            field: None,
                            suggestion: None,
                        });
                        existing.agent = merged_agent;
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
                            source: Some(AgentSourceKind::ProjectFile),
                            field: None,
                            suggestion: None,
                        });
                        continue;
                    }

                    if let Some(existing) = resolved.get_mut(&name) {
                        let base_spec = super::registry::AgentSpec::from_agent(&existing.agent);
                        let merged_spec = base_spec.merge_overlay(&file_agent.spec, replace);
                        let merged_agent = merged_spec.resolve(&existing.agent)?;
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
                            source: Some(AgentSourceKind::ProjectFile),
                            field: None,
                            suggestion: None,
                        });
                        existing.agent = merged_agent;
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
                    diagnostics.push(
                        AgentDiagnostic::new(
                            AgentDiagnosticSeverity::Info,
                            key,
                            format!("agent '{key}' is disabled in config, skipping override"),
                        )
                        .with_source(AgentSourceKind::ConfigAgent),
                    );
                    continue;
                }

                if let Some(existing) = resolved.get_mut(key) {
                    let merged = merge_agent_config(&existing.agent, agent_cfg)?;
                    let mode_diagnostic = if let Some(ref mode_str) = agent_cfg.mode {
                        match parse_mode(mode_str) {
                            Err(_) => Some(
                                AgentDiagnostic::new(
                                    AgentDiagnosticSeverity::Error,
                                    key,
                                    format!("invalid mode: {mode_str}"),
                                )
                                .with_source(AgentSourceKind::ConfigAgent)
                                .with_field("mode")
                                .with_suggestion("use one of: primary, subagent, all"),
                            ),
                            Ok(_) => None,
                        }
                    } else {
                        None
                    };
                    // Validate runtime_kind from config
                    let runtime_kind_diag = agent_cfg.runtime_kind.as_ref().and_then(|rk| {
                        if rk.parse::<AgentRuntimeKind>().is_err() {
                            Some(
                                AgentDiagnostic::new(
                                    AgentDiagnosticSeverity::Error,
                                    key,
                                    format!("invalid runtime_kind: {rk}"),
                                )
                                .with_source(AgentSourceKind::ConfigAgent)
                                .with_field("runtime_kind")
                                .with_suggestion(
                                    "use one of: standard, security_review, research, compaction, title, summary",
                                ),
                            )
                        } else {
                            None
                        }
                    });
                    // Validate permission actions from config
                    let perm_diags: Vec<AgentDiagnostic> = agent_cfg
                        .permission
                        .as_ref()
                        .map(|perms| {
                            perms
                                .iter()
                                .filter_map(|(tool, rule)| {
                                    let action = match rule {
                                        crate::config::schema::PermissionRule::Action(s) => {
                                            s.as_str()
                                        }
                                        crate::config::schema::PermissionRule::Object(obj) => {
                                            obj.get("default")
                                                .or_else(|| obj.get("action"))
                                                .map(|s| s.as_str())
                                                .unwrap_or("ask")
                                        }
                                    };
                                    if !matches!(action, "allow" | "deny" | "ask") {
                                        Some(
                                            AgentDiagnostic::new(
                                                AgentDiagnosticSeverity::Error,
                                                key,
                                                format!(
                                                    "invalid permission action '{action}' for tool '{tool}'"
                                                ),
                                            )
                                            .with_source(AgentSourceKind::ConfigAgent)
                                            .with_field("permission")
                                            .with_suggestion("use one of: allow, deny, ask"),
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    existing.agent = merged;
                    existing.sources.push(AgentSource {
                        kind: AgentSourceKind::ConfigAgent,
                        path: None,
                        name: key.clone(),
                    });
                    if let Some(diag) = mode_diagnostic {
                        existing.diagnostics.push(diag.clone());
                        diagnostics.push(diag);
                    }
                    if let Some(diag) = runtime_kind_diag {
                        existing.diagnostics.push(diag.clone());
                        diagnostics.push(diag);
                    }
                    for diag in perm_diags {
                        existing.diagnostics.push(diag.clone());
                        diagnostics.push(diag);
                    }
                } else {
                    match agent_from_config(key, agent_cfg) {
                        Ok(agent) => {
                            let agent_name = agent.name.clone();
                            let mode_diagnostic = if let Some(ref mode_str) = agent_cfg.mode {
                                match parse_mode(mode_str) {
                                    Err(_) => Some(
                                        AgentDiagnostic::new(
                                            AgentDiagnosticSeverity::Error,
                                            agent_name.clone(),
                                            format!("invalid mode: {mode_str}"),
                                        )
                                        .with_source(AgentSourceKind::ConfigAgent)
                                        .with_field("mode")
                                        .with_suggestion("use one of: primary, subagent, all"),
                                    ),
                                    Ok(_) => None,
                                }
                            } else {
                                None
                            };
                            // Validate runtime_kind from config
                            let runtime_kind_diag = agent_cfg.runtime_kind.as_ref().and_then(|rk| {
                                if rk.parse::<AgentRuntimeKind>().is_err() {
                                    Some(
                                        AgentDiagnostic::new(
                                            AgentDiagnosticSeverity::Error,
                                            agent_name.clone(),
                                            format!("invalid runtime_kind: {rk}"),
                                        )
                                        .with_source(AgentSourceKind::ConfigAgent)
                                        .with_field("runtime_kind")
                                        .with_suggestion(
                                            "use one of: standard, security_review, research, compaction, title, summary",
                                        ),
                                    )
                                } else {
                                    None
                                }
                            });
                            // Validate permission actions from config
                            let perm_diags: Vec<AgentDiagnostic> = agent_cfg
                                .permission
                                .as_ref()
                                .map(|perms| {
                                    perms
                                        .iter()
                                        .filter_map(|(tool, rule)| {
                                            let action = match rule {
                                                crate::config::schema::PermissionRule::Action(s) => {
                                                    s.as_str()
                                                }
                                                crate::config::schema::PermissionRule::Object(obj) => {
                                                    obj.get("default")
                                                        .or_else(|| obj.get("action"))
                                                        .map(|s| s.as_str())
                                                        .unwrap_or("ask")
                                                }
                                            };
                                            if !matches!(action, "allow" | "deny" | "ask") {
                                                Some(
                                                    AgentDiagnostic::new(
                                                        AgentDiagnosticSeverity::Error,
                                                        agent_name.clone(),
                                                        format!(
                                                            "invalid permission action '{action}' for tool '{tool}'"
                                                        ),
                                                    )
                                                    .with_source(AgentSourceKind::ConfigAgent)
                                                    .with_field("permission")
                                                    .with_suggestion("use one of: allow, deny, ask"),
                                                )
                                            } else {
                                                None
                                            }
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let mut agent_diags = Vec::new();
                            if let Some(diag) = mode_diagnostic {
                                agent_diags.push(diag.clone());
                                diagnostics.push(diag);
                            }
                            if let Some(diag) = runtime_kind_diag {
                                agent_diags.push(diag.clone());
                                diagnostics.push(diag);
                            }
                            for diag in perm_diags {
                                agent_diags.push(diag.clone());
                                diagnostics.push(diag);
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
                                source: Some(AgentSourceKind::ConfigAgent),
                                field: None,
                                suggestion: None,
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
        // The error message from parse_mode is "unknown agent mode: {s}"
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

    // --- AgentSpec overlay merge tests ---

    #[test]
    fn test_agent_spec_merge_overlay_explicit_false_overrides_true() {
        // Proves that an explicit `hidden = false` in the overlay overrides
        // a base `hidden = true`, which was previously impossible with Agent::merge_overlay.
        let base = AgentSpec {
            name: Some("test".into()),
            hidden: Some(true),
            description: Some("base desc".into()),
            ..Default::default()
        };
        let overlay = AgentSpec {
            hidden: Some(false),
            ..Default::default()
        };
        let merged = base.merge_overlay(&overlay, false);
        // Explicit false should win
        assert_eq!(merged.hidden, Some(false));
    }

    #[test]
    fn test_agent_spec_merge_overlay_none_preserves_base() {
        // Proves that an unset (None) overlay field preserves the base value.
        let base = AgentSpec {
            name: Some("test".into()),
            hidden: Some(true),
            temperature: Some(0.7),
            ..Default::default()
        };
        let overlay = AgentSpec {
            // Nothing set — all None
            ..Default::default()
        };
        let merged = base.merge_overlay(&overlay, false);
        assert_eq!(merged.hidden, Some(true));
        assert_eq!(merged.temperature, Some(0.7));
        assert_eq!(merged.name, Some("test".into()));
    }

    #[test]
    fn test_agent_spec_merge_overlay_explicit_overrides_base() {
        // Proves that explicit overlay values override base values for all scalar fields.
        let base = AgentSpec {
            name: Some("old-name".into()),
            description: Some("old desc".into()),
            model: Some("gpt-4".into()),
            temperature: Some(0.7),
            top_p: Some(0.9),
            color: Some("red".into()),
            steps: Some(10),
            hidden: Some(false),
            ..Default::default()
        };
        let overlay = AgentSpec {
            name: Some("new-name".into()),
            description: Some("new desc".into()),
            model: Some("claude-3".into()),
            temperature: Some(0.3),
            top_p: Some(0.5),
            color: Some("blue".into()),
            steps: Some(5),
            hidden: Some(true),
            ..Default::default()
        };
        let merged = base.merge_overlay(&overlay, false);
        assert_eq!(merged.name, Some("new-name".into()));
        assert_eq!(merged.description, Some("new desc".into()));
        assert_eq!(merged.model, Some("claude-3".into()));
        assert_eq!(merged.temperature, Some(0.3));
        assert_eq!(merged.top_p, Some(0.5));
        assert_eq!(merged.color, Some("blue".into()));
        assert_eq!(merged.steps, Some(5));
        assert_eq!(merged.hidden, Some(true));
    }

    #[test]
    fn test_agent_spec_merge_overlay_replace_discards_base() {
        // Proves that replace=true discards the base entirely.
        let base = AgentSpec {
            name: Some("old".into()),
            description: Some("old desc".into()),
            temperature: Some(0.9),
            hidden: Some(false),
            ..Default::default()
        };
        let overlay = AgentSpec {
            name: Some("new".into()),
            // description and temperature NOT set
            ..Default::default()
        };
        let merged = base.merge_overlay(&overlay, true);
        assert_eq!(merged.name, Some("new".into()));
        // Base values should be gone — overlay had None for these
        assert_eq!(merged.description, None);
        assert_eq!(merged.temperature, None);
        assert_eq!(merged.hidden, None);
    }

    #[test]
    fn test_agent_spec_merge_overlay_partial_override() {
        // Proves that overlaying only some fields leaves others from the base.
        let base = AgentSpec {
            name: Some("agent".into()),
            description: Some("base".into()),
            temperature: Some(0.7),
            color: Some("green".into()),
            ..Default::default()
        };
        let overlay = AgentSpec {
            temperature: Some(0.1),
            // name, description, color NOT set
            ..Default::default()
        };
        let merged = base.merge_overlay(&overlay, false);
        assert_eq!(merged.name, Some("agent".into()));
        assert_eq!(merged.description, Some("base".into()));
        assert_eq!(merged.temperature, Some(0.1)); // overridden
        assert_eq!(merged.color, Some("green".into())); // preserved
    }

    #[test]
    fn test_agent_spec_resolve_applies_spec_to_base() {
        // Proves that AgentSpec::resolve applies spec values on top of a base Agent.
        let base_agent = Agent {
            name: "base".into(),
            description: "base desc".into(),
            hidden: true,
            temperature: Some(0.9),
            ..Default::default()
        };
        let spec = AgentSpec {
            name: Some("resolved".into()),
            hidden: Some(false),
            temperature: Some(0.3),
            ..Default::default()
        };
        let resolved = spec.resolve(&base_agent).unwrap();
        assert_eq!(resolved.name, "resolved");
        assert!(!resolved.hidden);
        assert_eq!(resolved.temperature, Some(0.3));
        // description comes from base since spec didn't set it
        assert_eq!(resolved.description, "base desc");
    }

    #[test]
    fn test_agent_spec_from_agent_config_preserves_explicit_none() {
        // Proves that from_agent_config preserves which fields were actually set
        // in the AgentConfig (None stays None, not filled with defaults).
        use crate::config::schema::AgentConfig;

        let cfg = AgentConfig {
            name: Some("custom".into()),
            // All other fields are None
            ..Default::default()
        };
        let spec = AgentSpec::from_agent_config("fallback-name", &cfg).unwrap();
        assert_eq!(spec.name, Some("custom".into()));
        assert_eq!(spec.description, None); // not set, not defaulted
        assert_eq!(spec.mode, None); // not set
        assert_eq!(spec.model, None); // not set
        assert_eq!(spec.hidden, None); // not set
    }

    #[test]
    fn test_agent_spec_from_agent_config_uses_key_for_name() {
        // Proves that when name is not set in config, the key is used.
        use crate::config::schema::AgentConfig;

        let cfg = AgentConfig {
            description: Some("desc".into()),
            ..Default::default()
        };
        let spec = AgentSpec::from_agent_config("my-agent", &cfg).unwrap();
        assert_eq!(spec.name, Some("my-agent".into()));
    }

    #[test]
    fn test_agent_spec_from_agent_roundtrip() {
        // Proves that from_agent captures all fields from a concrete Agent.
        let agent = Agent {
            name: "test".into(),
            role: Some("tester".into()),
            description: "a test agent".into(),
            mode: AgentMode::Subagent,
            model: Some("gpt-4".into()),
            fallback_model: Some("gpt-3.5".into()),
            variant: Some("turbo".into()),
            temperature: Some(0.5),
            top_p: Some(0.8),
            color: Some("yellow".into()),
            steps: Some(42),
            hidden: true,
            thinking_budget: Some(1000),
            reasoning_effort: Some("high".into()),
            permissions: HashMap::from([("read".into(), "allow".into())]),
            ..Default::default()
        };
        let spec = AgentSpec::from_agent(&agent);
        assert_eq!(spec.name, Some("test".into()));
        assert_eq!(spec.role, Some("tester".into()));
        assert_eq!(spec.description, Some("a test agent".into()));
        assert_eq!(spec.mode, Some(AgentMode::Subagent));
        assert_eq!(spec.model, Some("gpt-4".into()));
        assert_eq!(spec.fallback_model, Some("gpt-3.5".into()));
        assert_eq!(spec.variant, Some("turbo".into()));
        assert_eq!(spec.temperature, Some(0.5));
        assert_eq!(spec.top_p, Some(0.8));
        assert_eq!(spec.color, Some("yellow".into()));
        assert_eq!(spec.steps, Some(42));
        assert_eq!(spec.hidden, Some(true));
        assert_eq!(spec.thinking_budget, Some(1000));
        assert_eq!(spec.reasoning_effort, Some("high".into()));
        assert_eq!(
            spec.permission,
            Some(HashMap::from([("read".into(), "allow".into())]))
        );
    }
}
