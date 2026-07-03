//! Agent management commands for the TUI.
//!
//! Handles `/agents` (list, show, diff, validate, reload) and
//! `/agent <name>` (select active agent).

use crate::agent::registry::{AgentRegistry, AgentSourceKind};
use crate::agent::{resolve_agents, AgentMode};
use crate::config::schema::Config;
use crate::tui::app::state::agent::AgentState;

/// Format `/agents` output: visible agents grouped by mode.
pub(crate) fn format_agents_list(agent_state: &AgentState, show_all: bool) -> Vec<String> {
    let config = Config::load().unwrap_or_default();
    let registry = match AgentRegistry::load(&config) {
        Ok(r) => r,
        Err(e) => return vec![format!("Failed to load agent registry: {e}")],
    };

    let agents: Vec<_> = if show_all {
        registry.list().collect()
    } else {
        registry.list_visible()
    };

    if agents.is_empty() {
        return vec!["No agents found.".to_string()];
    }

    let primary: Vec<_> = agents
        .iter()
        .filter(|ra| matches!(ra.agent.mode, AgentMode::Primary | AgentMode::All))
        .collect();
    let subagents: Vec<_> = agents
        .iter()
        .filter(|ra| matches!(ra.agent.mode, AgentMode::Subagent))
        .collect();

    let mut lines = Vec::new();
    let current_name = agent_state
        .agents
        .get(agent_state.current_agent)
        .map(|a| a.name.as_str())
        .unwrap_or("");

    if !primary.is_empty() {
        lines.push("Primary agents:".to_string());
        for ra in &primary {
            let marker = if ra.agent.name == current_name {
                "*"
            } else {
                " "
            };
            let hidden = if ra.agent.hidden { " (hidden)" } else { "" };
            lines.push(format!(
                "  {} {:<20} {}{}",
                marker,
                ra.agent.name,
                ra.agent.description,
                hidden
            ));
        }
    }

    if !subagents.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Subagents:".to_string());
        for ra in &subagents {
            let hidden = if ra.agent.hidden { " (hidden)" } else { "" };
            lines.push(format!(
                "    {:<20} {}{}",
                ra.agent.name, ra.agent.description, hidden
            ));
        }
    }

    if show_all {
        let hidden: Vec<_> = agents
            .iter()
            .filter(|ra| ra.agent.hidden)
            .collect();
        if !hidden.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("Hidden/system agents:".to_string());
            for ra in &hidden {
                lines.push(format!(
                    "    {:<20} {} [{:?}]",
                    ra.agent.name, ra.agent.description, ra.agent.mode
                ));
            }
        }
    }

    lines
}

/// Format `/agents show <name>` output: resolved agent metadata.
pub(crate) fn format_agent_show(name: &str) -> Vec<String> {
    let config = Config::load().unwrap_or_default();
    let registry = match AgentRegistry::load(&config) {
        Ok(r) => r,
        Err(e) => return vec![format!("Failed to load agent registry: {e}")],
    };

    let ra = match registry.get(name) {
        Some(ra) => ra,
        None => return vec![format!("Agent '{}' not found.", name)],
    };

    let agent = &ra.agent;
    let mut lines = Vec::new();

    lines.push(format!("name: {}", agent.name));
    if let Some(ref role) = agent.role {
        lines.push(format!("role: {role}"));
    }
    lines.push(format!("description: {}", agent.description));
    lines.push(format!("mode: {:?}", agent.mode));
    if let Some(ref runtime) = agent.runtime_kind {
        lines.push(format!("runtime: {runtime:?}"));
    }
    if let Some(ref model) = agent.model {
        lines.push(format!("model: {model}"));
    }
    if let Some(ref fallback) = agent.fallback_model {
        lines.push(format!("fallback_model: {fallback}"));
    }
    if let Some(ref variant) = agent.variant {
        lines.push(format!("variant: {variant}"));
    }
    if let Some(temp) = agent.temperature {
        lines.push(format!("temperature: {temp}"));
    }
    if let Some(ref color) = agent.color {
        lines.push(format!("color: {color}"));
    }
    if let Some(steps) = agent.steps {
        lines.push(format!("steps: {steps}"));
    }
    if agent.hidden {
        lines.push("hidden: true".to_string());
    }

    // Source stack
    lines.push(String::new());
    lines.push("sources:".to_string());
    for source in &ra.sources {
        let kind_str = match source.kind {
            AgentSourceKind::Builtin => "builtin",
            AgentSourceKind::GlobalFile => "global",
            AgentSourceKind::ProjectFile => "project",
            AgentSourceKind::ConfigAgent => "config",
            AgentSourceKind::ConfigMode => "mode",
            AgentSourceKind::Session => "session",
        };
        match &source.path {
            Some(path) => lines.push(format!("  {kind_str}: {}", path.display())),
            None => lines.push(format!("  {kind_str}: generated")),
        }
    }

    // Permissions
    if !agent.permissions.is_empty() {
        lines.push(String::new());
        lines.push("permissions:".to_string());
        let mut perms: Vec<_> = agent.permissions.iter().collect();
        perms.sort_by_key(|(k, _)| k.as_str());
        for (key, value) in perms {
            lines.push(format!("  {key}: {value}"));
        }
    }

    // Prompt preview
    if let Some(ref prompt) = agent.system_prompt {
        lines.push(String::new());
        lines.push("prompt:".to_string());
        let preview = if prompt.len() > 120 {
            format!("{}...", &prompt[..120])
        } else {
            prompt.clone()
        };
        for line in preview.lines() {
            lines.push(format!("  {line}"));
        }
    }

    // Diagnostics
    if !ra.diagnostics.is_empty() {
        lines.push(String::new());
        lines.push("diagnostics:".to_string());
        for diag in &ra.diagnostics {
            lines.push(format!("  [{:?}] {}", diag.severity, diag.message));
        }
    }

    lines
}

/// Format `/agents diff <name>` output: overlay changes.
pub(crate) fn format_agent_diff(name: &str) -> Vec<String> {
    let config = Config::load().unwrap_or_default();
    let registry = match AgentRegistry::load(&config) {
        Ok(r) => r,
        Err(e) => return vec![format!("Failed to load agent registry: {e}")],
    };

    let ra = match registry.get(name) {
        Some(ra) => ra,
        None => return vec![format!("Agent '{}' not found.", name)],
    };

    let mut lines = Vec::new();
    lines.push(name.to_string());

    // Source stack
    lines.push(String::new());
    lines.push("source stack:".to_string());
    for source in &ra.sources {
        let kind_str = match source.kind {
            AgentSourceKind::Builtin => "builtin",
            AgentSourceKind::GlobalFile => "global",
            AgentSourceKind::ProjectFile => "project",
            AgentSourceKind::ConfigAgent => "config",
            AgentSourceKind::ConfigMode => "mode",
            AgentSourceKind::Session => "session",
        };
        match &source.path {
            Some(path) => lines.push(format!("  {kind_str}: {}", path.display())),
            None => lines.push(format!("  {kind_str}: generated")),
        }
    }

    // Check for replace flag in source history
    let has_builtin = ra.sources.iter().any(|s| s.kind == AgentSourceKind::Builtin);
    let has_overlay = ra
        .sources
        .iter()
        .any(|s| matches!(s.kind, AgentSourceKind::GlobalFile | AgentSourceKind::ProjectFile));

    if has_builtin && has_overlay {
        // Check if any overlay replaced the builtin
        let builtin_agent = {
            let base_config = Config::default();
            if let Ok(base_reg) = AgentRegistry::load(&base_config) {
                base_reg.get(name).map(|ra| ra.agent.clone())
            } else {
                None
            }
        };

        if let Some(base) = builtin_agent {
            // Compare key fields to detect changes
            let agent = &ra.agent;
            let mut changed = Vec::new();

            if agent.model != base.model {
                changed.push(format!(
                    "model: {:?} -> {:?}",
                    base.model, agent.model
                ));
            }
            if agent.temperature != base.temperature {
                changed.push(format!(
                    "temperature: {:?} -> {:?}",
                    base.temperature, agent.temperature
                ));
            }
            if agent.description != base.description {
                changed.push(format!(
                    "description: {:?} -> {:?}",
                    base.description, agent.description
                ));
            }
            if agent.mode != base.mode {
                changed.push(format!("mode: {:?} -> {:?}", base.mode, agent.mode));
            }
            if agent.hidden != base.hidden {
                changed.push(format!("hidden: {} -> {}", base.hidden, agent.hidden));
            }
            if agent.permissions != base.permissions {
                // Show changed permissions
                let mut perm_changes = Vec::new();
                for (key, val) in &agent.permissions {
                    match base.permissions.get(key) {
                        Some(old_val) if old_val != val => {
                            perm_changes.push(format!("  {key}: {old_val} -> {val}"));
                        }
                        None => {
                            perm_changes.push(format!("  {key}: <none> -> {val}"));
                        }
                        _ => {}
                    }
                }
                for (key, val) in &base.permissions {
                    if !agent.permissions.contains_key(key) {
                        perm_changes.push(format!("  {key}: {val} -> <removed>"));
                    }
                }
                if !perm_changes.is_empty() {
                    changed.push("permissions:".to_string());
                    changed.extend(perm_changes);
                }
            }

            if changed.is_empty() {
                lines.push(String::new());
                lines.push("no changed fields (overlay is identical to built-in)".to_string());
            } else {
                lines.push(String::new());
                lines.push("changed fields:".to_string());
                for c in &changed {
                    lines.push(format!("  {c}"));
                }
            }

            // Unchanged critical fields
            let mut critical = Vec::new();
            if agent.runtime_kind == base.runtime_kind {
                if let Some(ref rk) = agent.runtime_kind {
                    critical.push(format!("runtime.kind: {rk:?}"));
                }
            }
            for key in &["edit", "write", "security", "lsp", "bash", "read"] {
                if let ( Some(base_val), Some(agent_val) ) = (
                    base.permissions.get(*key),
                    agent.permissions.get(*key),
                ) {
                    if base_val == agent_val {
                        critical.push(format!("  {key}: {agent_val}"));
                    }
                }
            }
            if !critical.is_empty() {
                lines.push(String::new());
                lines.push("unchanged critical fields:".to_string());
                for c in &critical {
                    lines.push(format!("  {c}"));
                }
            }
        }
    } else if !has_builtin && has_overlay {
        lines.push(String::new());
        lines.push("custom agent (no built-in base)".to_string());
    } else if has_builtin && !has_overlay {
        lines.push(String::new());
        lines.push("built-in only (no overlay applied)".to_string());
    }

    lines
}

/// Format `/agents validate` output: registry diagnostics.
pub(crate) fn format_agents_validate() -> Vec<String> {
    let config = Config::load().unwrap_or_default();
    let registry = match AgentRegistry::load(&config) {
        Ok(r) => r,
        Err(e) => return vec![format!("error: failed to load registry: {e}")],
    };

    let mut lines = Vec::new();
    let total = registry.list().count();
    let visible = registry.list_visible().len();
    let diags = registry.diagnostics();

    lines.push(format!("ok: {total} agents loaded ({visible} visible)"));

    if diags.is_empty() {
        lines.push("ok: no diagnostics".to_string());
    } else {
        for diag in diags {
            let prefix = match diag.severity {
                crate::agent::registry::AgentDiagnosticSeverity::Info => "info",
                crate::agent::registry::AgentDiagnosticSeverity::Warning => "warning",
                crate::agent::registry::AgentDiagnosticSeverity::Error => "error",
            };
            lines.push(format!("{prefix}: [{}] {}", diag.agent_name, diag.message));
        }
    }

    lines
}

/// Reload agents and return new agent list + diagnostics.
pub(crate) fn reload_agents() -> (Vec<crate::agent::Agent>, Vec<String>) {
    let config = Config::load().unwrap_or_default();
    match resolve_agents(&config) {
        Ok(agents) => {
            let count = agents.len();
            let visible = agents.iter().filter(|a| !a.hidden).count();
            let diags = vec![format!(
                "Reloaded {count} agents ({visible} visible)"
            )];
            (agents, diags)
        }
        Err(e) => {
            let diags = vec![format!("Reload failed: {e}")];
            (Vec::new(), diags)
        }
    }
}

/// Handle `/agent <name>`: validate and return the agent index if valid.
pub(crate) fn validate_agent_select(
    name: &str,
    agent_state: &AgentState,
) -> Result<usize, String> {
    // Find agent by name
    let idx = agent_state
        .agents
        .iter()
        .position(|a| a.name == name)
        .ok_or_else(|| format!("Agent '{}' not found.", name))?;

    let agent = &agent_state.agents[idx];

    // Reject subagent-only agents
    if matches!(agent.mode, AgentMode::Subagent) {
        return Err(format!(
            "Agent '{}' is a subagent-only agent and cannot be selected as the active agent. Use @{} to spawn it as a subagent.",
            name, name
        ));
    }

    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_agents_list_shows_primary_and_subagents() {
        let state = AgentState {
            agents: resolve_agents(&Config::default()).unwrap_or_default(),
            current_agent: 0,
            current_model: String::new(),
            models: Vec::new(),
            model_idx: 0,
            plan_mode: false,
            plan_topic: None,
        };
        let lines = format_agents_list(&state, false);
        let text = lines.join("\n");
        assert!(text.contains("Primary agents:"));
        assert!(text.contains("Subagents:"));
    }

    #[test]
    fn format_agent_show_displays_metadata() {
        let lines = format_agent_show("build");
        let text = lines.join("\n");
        assert!(text.contains("name: build"));
        assert!(text.contains("mode:"));
        assert!(text.contains("sources:"));
    }

    #[test]
    fn format_agent_show_unknown_agent() {
        let lines = format_agent_show("nonexistent");
        assert!(lines[0].contains("not found"));
    }

    #[test]
    fn format_agents_validate_reports_ok() {
        let lines = format_agents_validate();
        assert!(lines[0].contains("ok:"));
    }

    #[test]
    fn validate_agent_select_rejects_subagent() {
        let state = AgentState {
            agents: resolve_agents(&Config::default()).unwrap_or_default(),
            current_agent: 0,
            current_model: String::new(),
            models: Vec::new(),
            model_idx: 0,
            plan_mode: false,
            plan_topic: None,
        };
        // "general" is a Subagent-only agent
        let result = validate_agent_select("general", &state);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("subagent-only"));
    }

    #[test]
    fn validate_agent_select_accepts_primary() {
        let state = AgentState {
            agents: resolve_agents(&Config::default()).unwrap_or_default(),
            current_agent: 0,
            current_model: String::new(),
            models: Vec::new(),
            model_idx: 0,
            plan_mode: false,
            plan_topic: None,
        };
        let result = validate_agent_select("build", &state);
        assert!(result.is_ok());
    }
}
