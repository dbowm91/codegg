//! Agent management commands for the TUI.
//!
//! Handles `/agents` (list, show, diff, validate, reload) and
//! `/agent <name>` (select active agent).

use crate::agent::asset_context::{AssetContext, AssetContextBuilder, ProjectId};
use crate::agent::registry::{AgentRegistry, AgentSourceKind};
#[cfg(test)]
use crate::agent::resolve_agents;
use crate::agent::AgentMode;
use crate::config::schema::Config;
use crate::protocol::core::{
    AssetRefreshReasonDto, AssetRefreshRequestDto, AssetRefreshScopeDto, CoreRequest, CoreResponse,
};
use crate::tui::app::state::agent::AgentState;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;
use crate::util::truncate::truncate_prefix;

/// Route `/reload` and all focused reload aliases through the daemon-owned
/// refresh coordinator. The TUI never discovers assets or edits foreign
/// harness directories locally.
pub(crate) fn start_refresh_assets(app: &mut crate::tui::app::App) {
    let Some(session) = app.session_state.session.as_ref() else {
        app.messages_state
            .toasts
            .error("Cannot refresh assets without an attached session");
        return;
    };
    let Some(workspace_id) = session.workspace_id.clone() else {
        app.messages_state
            .toasts
            .error("Asset refresh requires a canonical project/workspace binding");
        return;
    };
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let session_id = session.id.clone();
    let project_id = session.project_id.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "refresh-assets",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::AssetRefreshFinished {
                    report: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("asset-refresh-{}", uuid::Uuid::new_v4()),
                CoreRequest::AssetRefresh {
                    request: AssetRefreshRequestDto {
                        scope: AssetRefreshScopeDto {
                            project_id,
                            workspace_id,
                        },
                        reason: AssetRefreshReasonDto::Reload,
                        session_id: Some(session_id),
                    },
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::AssetRefresh { report }) => {
                    Some(TuiCommand::AssetRefreshFinished {
                        report: Some(report),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::AssetRefreshFinished {
                        report: None,
                        error: Some(format!("Asset refresh failed ({}): {}", code, message)),
                    })
                }
                Ok(_) => Some(TuiCommand::AssetRefreshFinished {
                    report: None,
                    error: Some("Unexpected daemon response for asset refresh".to_string()),
                }),
                Err(error) => Some(TuiCommand::AssetRefreshFinished {
                    report: None,
                    error: Some(format!("Asset refresh request failed: {error}")),
                }),
            }
        },
    );
}

pub(crate) fn apply_asset_refresh_finished(
    app: &mut crate::tui::app::App,
    report: Option<crate::protocol::core::AssetRefreshReportDto>,
    error: Option<String>,
) {
    if let Some(error) = error {
        app.messages_state.toasts.error(&error);
        return;
    }
    let Some(report) = report else {
        app.messages_state
            .toasts
            .error("Asset refresh returned no report");
        return;
    };
    let outcome = format!("{:?}", report.outcome).to_lowercase();
    let message = format!(
        "Runtime assets {} (generation {}): +{} -{} ~{} shadowed {} invalid {} retained {}",
        outcome,
        report
            .generation
            .map(|generation| generation.to_string())
            .unwrap_or_else(|| "none".to_string()),
        report.added.len(),
        report.removed.len(),
        report.changed.len(),
        report.shadowed.len(),
        report.invalid.len(),
        report.retained.len(),
    );
    if matches!(
        report.outcome,
        crate::protocol::core::AssetRefreshOutcomeDto::Published
    ) {
        app.messages_state.toasts.success(&message);
    } else {
        app.messages_state.toasts.info(&message);
    }
}

/// Build an `AssetContext` rooted at the given workspace for
/// `/agents` commands. CLI bootstrap is allowed to fall back to
/// process-global cwd for the project root; the result is still an
/// explicit context with a synthetic `ProjectId` so the registry no
/// longer reads `PWD` directly.
fn cli_compat_context(workspace_root: &std::path::Path) -> AssetContext {
    AssetContextBuilder::new()
        .with_synthetic_project_id(ProjectId::new())
        .with_workspace_root(workspace_root)
        .build()
        .expect("workspace root is valid")
}

/// Production CLI bootstrap helper. Reads `current_dir` exactly once at
/// the boundary so the agent registry no longer needs to.
fn cli_compat_context_from_cwd() -> AssetContext {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    cli_compat_context(&cwd)
}

fn load_cli_registry() -> Result<AgentRegistry, crate::error::AgentError> {
    let config = Config::load().unwrap_or_default();
    let ctx = cli_compat_context_from_cwd();
    AgentRegistry::load_for_context(&config, &ctx)
}

/// Format `/agents` output: visible agents grouped by mode.
pub(crate) fn format_agents_list(agent_state: &AgentState, show_all: bool) -> Vec<String> {
    let registry = match load_cli_registry() {
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
                marker, ra.agent.name, ra.agent.description, hidden
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
        let hidden: Vec<_> = agents.iter().filter(|ra| ra.agent.hidden).collect();
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
    let registry = match load_cli_registry() {
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
            format!("{}...", truncate_prefix(prompt, 120))
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
    let registry = match load_cli_registry() {
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
    let has_builtin = ra
        .sources
        .iter()
        .any(|s| s.kind == AgentSourceKind::Builtin);
    let has_overlay = ra.sources.iter().any(|s| {
        matches!(
            s.kind,
            AgentSourceKind::GlobalFile | AgentSourceKind::ProjectFile
        )
    });

    if has_builtin && has_overlay {
        // Check if any overlay replaced the builtin
        let builtin_agent = {
            let base_config = Config::default();
            let base_ctx = AssetContextBuilder::new()
                .with_synthetic_project_id(ProjectId::new())
                .with_workspace_root(std::path::PathBuf::from("."))
                .build()
                .expect("base context is valid");
            if let Ok(base_reg) = AgentRegistry::load_for_context(&base_config, &base_ctx) {
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
                changed.push(format!("model: {:?} -> {:?}", base.model, agent.model));
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
                if let (Some(base_val), Some(agent_val)) =
                    (base.permissions.get(*key), agent.permissions.get(*key))
                {
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
    let (lines, _has_errors) = format_agents_validate_inner();
    lines
}

/// Validate agents and return diagnostics with error status.
/// Returns (lines, has_errors) for headless/CLI mode.
fn format_agents_validate_inner() -> (Vec<String>, bool) {
    let registry = match load_cli_registry() {
        Ok(r) => r,
        Err(e) => return (vec![format!("error: failed to load registry: {e}")], true),
    };

    let mut lines = Vec::new();
    let total = registry.list().count();
    let visible = registry.list_visible().len();
    let diags = registry.diagnostics();

    lines.push(format!("ok: {total} agents loaded ({visible} visible)"));

    let mut has_errors = false;
    if diags.is_empty() {
        lines.push("ok: no diagnostics".to_string());
    } else {
        let errors = diags
            .iter()
            .filter(|d| d.severity == crate::agent::registry::AgentDiagnosticSeverity::Error)
            .count();
        let warnings = diags
            .iter()
            .filter(|d| d.severity == crate::agent::registry::AgentDiagnosticSeverity::Warning)
            .count();
        let infos = diags
            .iter()
            .filter(|d| d.severity == crate::agent::registry::AgentDiagnosticSeverity::Info)
            .count();
        has_errors = errors > 0;
        lines.push(format!(
            "{errors} error(s), {warnings} warning(s), {infos} info(s)"
        ));
        lines.push(String::new());
        for diag in diags {
            let prefix = match diag.severity {
                crate::agent::registry::AgentDiagnosticSeverity::Info => "info",
                crate::agent::registry::AgentDiagnosticSeverity::Warning => "warning",
                crate::agent::registry::AgentDiagnosticSeverity::Error => "error",
            };
            let source_str = diag
                .source
                .as_ref()
                .map(|s| format!(" ({:?})", s))
                .unwrap_or_default();
            let field_str = diag
                .field
                .as_ref()
                .map(|f| format!(" [field: {f}]"))
                .unwrap_or_default();
            let suggestion_str = diag
                .suggestion
                .as_ref()
                .map(|s| format!(" — {s}"))
                .unwrap_or_default();
            lines.push(format!(
                "{prefix}: [{}] {}{source_str}{field_str}{suggestion_str}",
                diag.agent_name, diag.message
            ));
        }
    }

    (lines, has_errors)
}

/// Rebuild the agent registry from scratch and return new agent list + diagnostics.
/// Unlike reload, this uses the full AgentRegistry to capture source provenance
/// and diagnostics, then converts to plain agents.
pub(crate) fn rebuild_agents() -> (Vec<crate::agent::Agent>, Vec<String>) {
    match load_cli_registry() {
        Ok(registry) => {
            let count = registry.list().count();
            let visible = registry.list_visible().len();
            let diags_count = registry.diagnostics().len();
            let errors = registry
                .diagnostics()
                .iter()
                .filter(|d| d.severity == crate::agent::registry::AgentDiagnosticSeverity::Error)
                .count();
            let agents = registry.into_agents();
            let mut diags = vec![format!("Rebuilt {count} agents ({visible} visible)")];
            if diags_count > 0 {
                diags.push(format!("{diags_count} diagnostic(s) ({errors} error(s))"));
            }
            (agents, diags)
        }
        Err(e) => {
            let diags = vec![format!("Rebuild failed: {e}")];
            (Vec::new(), diags)
        }
    }
}

/// Handle `/agent <name>`: validate and return the agent index if valid.
pub(crate) fn validate_agent_select(name: &str, agent_state: &AgentState) -> Result<usize, String> {
    // Find agent by name
    let idx = agent_state
        .agents
        .iter()
        .position(|a| a.name == name)
        .ok_or_else(|| format!("Agent '{}' not found.", name))?;

    let agent = &agent_state.agents[idx];

    // Reject hidden/system agents
    if agent.hidden {
        return Err(format!(
            "Agent '{}' is a hidden/system agent and cannot be selected as the active agent.",
            name
        ));
    }

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
            snapshot: None,
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
            snapshot: None,
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
            snapshot: None,
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

    #[test]
    fn format_agents_list_all_shows_hidden() {
        let state = AgentState {
            snapshot: None,
            agents: resolve_agents(&Config::default()).unwrap_or_default(),
            current_agent: 0,
            current_model: String::new(),
            models: Vec::new(),
            model_idx: 0,
            plan_mode: false,
            plan_topic: None,
        };
        let lines = format_agents_list(&state, true);
        let text = lines.join("\n");
        assert!(
            text.contains("Hidden/system agents:"),
            "Expected 'Hidden/system agents:' section in --all output"
        );
        assert!(
            text.contains("compactor") || text.contains("summary") || text.contains("title"),
            "Expected at least one hidden agent name in output"
        );
    }

    #[test]
    fn validate_agent_select_rejects_hidden() {
        let hidden_agent = crate::agent::Agent {
            name: "compaction".to_string(),
            hidden: true,
            mode: crate::agent::AgentMode::Primary,
            description: "hidden agent".to_string(),
            ..Default::default()
        };
        let state = AgentState {
            snapshot: None,
            agents: vec![hidden_agent],
            current_agent: 0,
            current_model: String::new(),
            models: Vec::new(),
            model_idx: 0,
            plan_mode: false,
            plan_topic: None,
        };
        let result = validate_agent_select("compaction", &state);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("hidden"),
            "Error should mention hidden agent"
        );
    }

    #[test]
    fn format_agent_diff_nonexistent_agent() {
        let lines = format_agent_diff("nonexistent-agent");
        let text = lines.join("\n");
        assert!(
            text.contains("not found"),
            "Expected 'not found' for unknown agent"
        );
    }

    #[test]
    fn format_agents_validate_clean() {
        let lines = format_agents_validate();
        assert!(
            lines[0].starts_with("ok:"),
            "Expected output to start with 'ok:', got: {}",
            lines[0]
        );
    }
}
