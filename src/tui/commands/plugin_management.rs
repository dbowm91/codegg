//! Plugin management command handlers for the TUI.
//!
//! Provides list/info/enable/disable/install/remove/doctor operations
//! for installed plugins. Uses the filesystem-backed
//! [`MarketplaceService`] for queries and the [`install`] module for
//! mutations. Enable/disable state is persisted in a TOML file inside
//! the plugins directory so it survives daemon restarts.
//!
//! All handlers are `pub(crate) fn` (non-async) and spawn background
//! tasks via [`spawn_tui_task`] for any I/O, posting a typed
//! [`TuiCommand`] completion variant back through the command channel.

use crate::plugin::install::plugins_dir;
use crate::plugin::marketplace::{MarketplacePlugin, MarketplaceService};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Format a single plugin as a one-line summary for the list view.
///
/// Format: `id | name | version | status | capabilities`
pub(crate) fn format_plugin_line(plugin: &MarketplacePlugin, enabled: bool) -> String {
    let hook_count = plugin.hooks.len();
    format!(
        "{} | {} | {} | {} | {}",
        plugin.id,
        plugin.name,
        plugin.version,
        if enabled { "enabled" } else { "disabled" },
        if hook_count > 0 {
            format!("{} hooks", hook_count)
        } else {
            "no capabilities".to_string()
        },
    )
}

/// Format a single plugin as detailed key-value lines for the info view.
pub(crate) fn format_plugin_detail(plugin: &MarketplacePlugin, enabled: bool) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("ID:          {}", plugin.id));
    lines.push(format!("Name:        {}", plugin.name));
    lines.push(format!("Version:     {}", plugin.version));
    lines.push(format!(
        "Status:      {}",
        if enabled { "enabled" } else { "disabled" }
    ));
    lines.push(format!("Tier:        {}", plugin.tier));
    if let Some(ref desc) = plugin.description {
        lines.push(format!("Description: {}", desc));
    }
    if let Some(ref author) = plugin.author {
        lines.push(format!("Author:      {}", author));
    }
    if let Some(ref homepage) = plugin.homepage {
        lines.push(format!("Homepage:    {}", homepage));
    }
    if !plugin.hooks.is_empty() {
        lines.push(format!("Hooks:       {}", plugin.hooks.join(", ")));
    } else {
        lines.push("Hooks:       (none)".to_string());
    }
    lines
}

/// Resolve a query string to a plugin id by exact match or fuzzy substring.
/// Returns the marketplace plugin if found.
pub(crate) fn resolve_plugin<'a>(
    plugins: &'a [MarketplacePlugin],
    query: &str,
) -> Result<&'a MarketplacePlugin, String> {
    // Exact match first
    if let Some(p) = plugins.iter().find(|p| p.id == query) {
        return Ok(p);
    }
    // Exact name match
    if let Some(p) = plugins.iter().find(|p| p.name == query) {
        return Ok(p);
    }
    // Substring match (case-insensitive)
    let q = query.to_lowercase();
    let matches: Vec<&MarketplacePlugin> = plugins
        .iter()
        .filter(|p| p.id.to_lowercase().contains(&q) || p.name.to_lowercase().contains(&q))
        .collect();
    match matches.len() {
        0 => Err(format!("No plugin found matching '{}'", query)),
        1 => Ok(matches[0]),
        _ => {
            let names: Vec<&str> = matches.iter().map(|p| p.id.as_str()).collect();
            Err(format!(
                "Ambiguous query '{}'; matches: {}",
                query,
                names.join(", ")
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Enable/disable state persistence
// ---------------------------------------------------------------------------

/// File that tracks which plugins are explicitly disabled.
/// Lives at `<plugins_dir>/disabled_plugins.toml`.
fn disabled_state_path() -> std::path::PathBuf {
    plugins_dir().join("disabled_plugins.toml")
}

/// Load the set of explicitly disabled plugin ids from disk.
fn load_disabled_set() -> std::collections::HashSet<String> {
    let path = disabled_state_path();
    if !path.exists() {
        return std::collections::HashSet::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return std::collections::HashSet::new(),
    };
    let mut disabled = std::collections::HashSet::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some((name, value)) = line.split_once('=') {
            let name = name.trim().trim_matches('"');
            let value = value.trim();
            if value == "true" {
                disabled.insert(name.to_string());
            }
        }
    }
    disabled
}

/// Persist the set of disabled plugin ids to disk.
fn save_disabled_set(disabled: &std::collections::HashSet<String>) -> Result<(), String> {
    let path = disabled_state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create plugins dir: {e}"))?;
    }
    let mut content = String::from("[disabled]\n");
    for name in disabled {
        content.push_str(&format!("\"{}\" = true\n", name));
    }
    std::fs::write(&path, content).map_err(|e| format!("Failed to write state: {e}"))
}

/// Check if a plugin is enabled (not in the disabled set).
#[allow(dead_code)]
pub(crate) fn is_plugin_enabled(plugin_id: &str) -> bool {
    !load_disabled_set().contains(plugin_id)
}

// ---------------------------------------------------------------------------
// Command handlers (spawn + completion pattern)
// ---------------------------------------------------------------------------

/// List all installed plugins.
pub(crate) fn show_plugins(app: &mut App) {
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_list", async move {
        let svc = MarketplaceService::new();
        let plugins = svc.list_local_plugins().await;
        let disabled = load_disabled_set();

        if plugins.is_empty() {
            return Some(TuiCommand::PluginListFinished {
                lines: vec!["No plugins installed.".to_string()],
                error: None,
            });
        }

        let mut lines = Vec::new();
        lines.push(format!("Installed plugins ({}):", plugins.len()));
        lines.push(String::new());
        for plugin in &plugins {
            let enabled = !disabled.contains(&plugin.id);
            lines.push(format_plugin_line(plugin, enabled));
        }

        Some(TuiCommand::PluginListFinished { lines, error: None })
    });
}

/// Show detailed info for a single plugin.
pub(crate) fn show_plugin_info(app: &mut App, query: &str) {
    let query = query.to_string();
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_info", async move {
        let svc = MarketplaceService::new();
        let plugins = svc.list_local_plugins().await;
        let disabled = load_disabled_set();

        match resolve_plugin(&plugins, &query) {
            Ok(plugin) => {
                let enabled = !disabled.contains(&plugin.id);
                let lines = format_plugin_detail(plugin, enabled);
                Some(TuiCommand::PluginInfoFinished {
                    plugin_id: plugin.id.clone(),
                    lines,
                    error: None,
                })
            }
            Err(e) => Some(TuiCommand::PluginInfoFinished {
                plugin_id: query,
                lines: Vec::new(),
                error: Some(e),
            }),
        }
    });
}

/// Enable a plugin (remove from disabled set).
pub(crate) fn enable_plugin(app: &mut App, query: &str) {
    let query = query.to_string();
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_enable", async move {
        let svc = MarketplaceService::new();
        let plugins = svc.list_local_plugins().await;

        let plugin_id = match resolve_plugin(&plugins, &query) {
            Ok(p) => p.id.clone(),
            Err(e) => {
                return Some(TuiCommand::PluginEnableFinished {
                    plugin_id: query,
                    error: Some(e),
                });
            }
        };

        let mut disabled = load_disabled_set();
        if disabled.remove(&plugin_id) {
            match save_disabled_set(&disabled) {
                Ok(()) => Some(TuiCommand::PluginEnableFinished {
                    plugin_id,
                    error: None,
                }),
                Err(e) => Some(TuiCommand::PluginEnableFinished {
                    plugin_id,
                    error: Some(e),
                }),
            }
        } else {
            Some(TuiCommand::PluginEnableFinished {
                plugin_id,
                error: Some("Plugin is already enabled".to_string()),
            })
        }
    });
}

/// Disable a plugin (add to disabled set).
pub(crate) fn disable_plugin(app: &mut App, query: &str) {
    let query = query.to_string();
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_disable", async move {
        let svc = MarketplaceService::new();
        let plugins = svc.list_local_plugins().await;

        let plugin_id = match resolve_plugin(&plugins, &query) {
            Ok(p) => p.id.clone(),
            Err(e) => {
                return Some(TuiCommand::PluginDisableFinished {
                    plugin_id: query,
                    error: Some(e),
                });
            }
        };

        let mut disabled = load_disabled_set();
        if disabled.insert(plugin_id.clone()) {
            match save_disabled_set(&disabled) {
                Ok(()) => Some(TuiCommand::PluginDisableFinished {
                    plugin_id,
                    error: None,
                }),
                Err(e) => Some(TuiCommand::PluginDisableFinished {
                    plugin_id,
                    error: Some(e),
                }),
            }
        } else {
            Some(TuiCommand::PluginDisableFinished {
                plugin_id,
                error: Some("Plugin is already disabled".to_string()),
            })
        }
    });
}

/// Run plugin diagnostics.
///
/// When `query` is `None`, diagnostics run for all installed plugins.
/// When `Some(name)`, diagnostics run for the matching plugin only.
pub(crate) fn doctor_plugin(app: &mut App, query: Option<&str>) {
    let query = query.map(|s| s.to_string());
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_doctor", async move {
        let svc = MarketplaceService::new();
        let plugins = svc.list_local_plugins().await;
        let disabled = load_disabled_set();

        let plugins_to_check: Vec<MarketplacePlugin> = match query {
            Some(ref q) => match resolve_plugin(&plugins, q) {
                Ok(p) => vec![p.clone()],
                Err(e) => {
                    return Some(TuiCommand::PluginDoctorFinished {
                        lines: Vec::new(),
                        error: Some(e),
                    });
                }
            },
            None => plugins,
        };

        let mut lines = Vec::new();
        lines.push(format!(
            "Plugin doctor: checking {} plugin(s)",
            plugins_to_check.len()
        ));
        lines.push(String::new());

        let plugins_dir = plugins_dir();
        for plugin in &plugins_to_check {
            let enabled = !disabled.contains(&plugin.id);
            let status_str = if enabled { "OK" } else { "DISABLED" };

            let manifest_path = plugins_dir.join(&plugin.id).join("manifest.toml");
            let manifest_ok = manifest_path.exists();

            let wasm_path = plugins_dir.join(&plugin.id).join("plugin.wasm");
            let wasm_ok = wasm_path.exists();

            lines.push(format!(
                "[{}] {} v{} — manifest={} wasm={} hooks={}",
                status_str,
                plugin.name,
                plugin.version,
                if manifest_ok { "found" } else { "MISSING" },
                if wasm_ok { "found" } else { "absent" },
                plugin.hooks.len(),
            ));

            if !manifest_ok {
                lines.push(format!(
                    "  WARNING: manifest.toml not found at {}",
                    manifest_path.display()
                ));
            }
            if plugin.hooks.is_empty() {
                lines.push("  NOTE: plugin declares no hooks or capabilities".to_string());
            }
        }

        lines.push(String::new());
        lines.push("Diagnostics complete.".to_string());

        Some(TuiCommand::PluginDoctorFinished {
            lines,
            error: None,
        })
    });
}

/// Remove (uninstall) an installed plugin.
///
/// Safety: only removes directories under the canonical plugins directory.
/// Refuses to operate on paths outside the plugins directory.
pub(crate) fn remove_plugin(app: &mut App, query: &str) {
    let query = query.to_string();
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_remove", async move {
        let svc = MarketplaceService::new();
        let plugins = svc.list_local_plugins().await;

        let plugin_id = match resolve_plugin(&plugins, &query) {
            Ok(p) => p.id.clone(),
            Err(e) => {
                return Some(TuiCommand::PluginRemoveFinished {
                    plugin_id: query,
                    error: Some(e),
                });
            }
        };

        // Safety check: only remove from the canonical plugins directory
        let plugins_dir = plugins_dir();
        let target = plugins_dir.join(&plugin_id);
        let target_canonical = match std::fs::canonicalize(&target) {
            Ok(p) => p,
            Err(e) => {
                return Some(TuiCommand::PluginRemoveFinished {
                    plugin_id,
                    error: Some(format!("Cannot resolve plugin path: {e}")),
                });
            }
        };
        let plugins_dir_canonical = match std::fs::canonicalize(&plugins_dir) {
            Ok(p) => p,
            Err(e) => {
                return Some(TuiCommand::PluginRemoveFinished {
                    plugin_id,
                    error: Some(format!("Cannot resolve plugins directory: {e}")),
                });
            }
        };

        if !target_canonical.starts_with(&plugins_dir_canonical) {
            return Some(TuiCommand::PluginRemoveFinished {
                plugin_id,
                error: Some("Refused: plugin path is outside the plugins directory".to_string()),
            });
        }

        match tokio::fs::remove_dir_all(&target).await {
            Ok(()) => Some(TuiCommand::PluginRemoveFinished {
                plugin_id,
                error: None,
            }),
            Err(e) => Some(TuiCommand::PluginRemoveFinished {
                plugin_id,
                error: Some(format!("Failed to remove plugin: {e}")),
            }),
        }
    });
}

/// Install a plugin from a local filesystem path.
///
/// The source directory must contain a `manifest.toml`. The plugin is
/// copied into the canonical plugins directory.
pub(crate) fn install_plugin(app: &mut App, source_path: &str) {
    let source_path = source_path.to_string();
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_install", async move {
        let path = std::path::PathBuf::from(&source_path);
        match crate::plugin::install::install_from_path(&path).await {
            Ok(dest) => {
                let manifest_path = dest.join("manifest.toml");
                let mut lines = vec![format!("Plugin installed to: {}", dest.display())];
                if let Ok(content) = tokio::fs::read_to_string(&manifest_path).await {
                    if let Ok(manifest) =
                        toml::from_str::<crate::plugin::manifest::PluginManifest>(&content)
                    {
                        lines.push(format!("Name:    {}", manifest.name));
                        lines.push(format!("Version: {}", manifest.version));
                        let cap_count = manifest.capabilities.len();
                        let hook_count = manifest.hooks.len();
                        if cap_count > 0 {
                            lines.push(format!("Capabilities: {}", cap_count));
                        }
                        if hook_count > 0 {
                            lines.push(format!("Legacy hooks: {}", hook_count));
                        }
                        if cap_count == 0 && hook_count == 0 {
                            lines.push("No capabilities or hooks declared".to_string());
                        }
                    }
                }
                Some(TuiCommand::PluginInstallFinished {
                    source: source_path,
                    lines,
                    error: None,
                })
            }
            Err(e) => Some(TuiCommand::PluginInstallFinished {
                source: source_path,
                lines: Vec::new(),
                error: Some(e.to_string()),
            }),
        }
    });
}

// ---------------------------------------------------------------------------
// Apply handlers (called from command_dispatch)
// ---------------------------------------------------------------------------

/// Apply the result of a plugin list operation.
pub(crate) fn apply_plugin_list_finished(
    app: &mut App,
    lines: Vec<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Plugin list failed: {err}"));
        return;
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}

/// Apply the result of a plugin info operation.
pub(crate) fn apply_plugin_info_finished(
    app: &mut App,
    _plugin_id: String,
    lines: Vec<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Plugin info: {err}"));
        return;
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}

/// Apply the result of a plugin enable operation.
pub(crate) fn apply_plugin_enable_finished(
    app: &mut App,
    plugin_id: String,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Enable failed: {err}"));
    } else {
        app.messages_state
            .toasts
            .success(&format!("Plugin '{plugin_id}' enabled"));
    }
}

/// Apply the result of a plugin disable operation.
pub(crate) fn apply_plugin_disable_finished(
    app: &mut App,
    plugin_id: String,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Disable failed: {err}"));
    } else {
        app.messages_state
            .toasts
            .success(&format!("Plugin '{plugin_id}' disabled"));
    }
}

/// Apply the result of a plugin doctor operation.
pub(crate) fn apply_plugin_doctor_finished(
    app: &mut App,
    lines: Vec<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Plugin doctor: {err}"));
        return;
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::DoctorReport,
        lines,
    );
}

/// Apply the result of a plugin remove operation.
pub(crate) fn apply_plugin_remove_finished(
    app: &mut App,
    plugin_id: String,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Remove failed: {err}"));
    } else {
        app.messages_state
            .toasts
            .success(&format!("Plugin '{plugin_id}' removed"));
    }
}

/// Apply the result of a plugin install operation.
pub(crate) fn apply_plugin_install_finished(
    app: &mut App,
    _source: String,
    lines: Vec<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Install failed: {err}"));
        return;
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plugin(id: &str, name: &str, hooks: Vec<&str>) -> MarketplacePlugin {
        MarketplacePlugin {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: Some(format!("A plugin called {name}")),
            author: Some("Test Author".to_string()),
            homepage: None,
            tier: crate::plugin::marketplace::PluginTier::Personal,
            hooks: hooks.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn format_plugin_line_enabled() {
        let plugin = sample_plugin("my-plugin", "My Plugin", vec!["auth.resolve"]);
        let line = format_plugin_line(&plugin, true);
        assert!(line.contains("my-plugin"));
        assert!(line.contains("My Plugin"));
        assert!(line.contains("1.0.0"));
        assert!(line.contains("enabled"));
        assert!(line.contains("1 hooks"));
    }

    #[test]
    fn format_plugin_line_disabled() {
        let plugin = sample_plugin("other", "Other", vec![]);
        let line = format_plugin_line(&plugin, false);
        assert!(line.contains("disabled"));
        assert!(line.contains("no capabilities"));
    }

    #[test]
    fn format_plugin_detail_basic() {
        let plugin = sample_plugin("test-id", "Test Name", vec!["auth.resolve", "tool.before"]);
        let lines = format_plugin_detail(&plugin, true);
        assert!(lines.iter().any(|l| l.contains("test-id")));
        assert!(lines.iter().any(|l| l.contains("Test Name")));
        assert!(lines.iter().any(|l| l.contains("enabled")));
        assert!(lines.iter().any(|l| l.contains("auth.resolve")));
    }

    #[test]
    fn format_plugin_detail_disabled() {
        let plugin = sample_plugin("x", "X", vec![]);
        let lines = format_plugin_detail(&plugin, false);
        assert!(lines.iter().any(|l| l.contains("disabled")));
        assert!(lines.iter().any(|l| l.contains("(none)")));
    }

    #[test]
    fn resolve_plugin_exact_id() {
        let plugins = vec![
            sample_plugin("alpha", "Alpha", vec![]),
            sample_plugin("beta", "Beta", vec![]),
        ];
        let result = resolve_plugin(&plugins, "alpha");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "alpha");
    }

    #[test]
    fn resolve_plugin_exact_name() {
        let plugins = vec![
            sample_plugin("a-alpha", "Alpha", vec![]),
            sample_plugin("b-beta", "Beta", vec![]),
        ];
        let result = resolve_plugin(&plugins, "Beta");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "b-beta");
    }

    #[test]
    fn resolve_plugin_fuzzy_substring() {
        let plugins = vec![
            sample_plugin("my-copilot", "Copilot Auth", vec![]),
            sample_plugin("my-gitlab", "GitLab Auth", vec![]),
        ];
        let result = resolve_plugin(&plugins, "copi");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "my-copilot");
    }

    #[test]
    fn resolve_plugin_no_match() {
        let plugins = vec![sample_plugin("alpha", "Alpha", vec![])];
        let result = resolve_plugin(&plugins, "nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No plugin found"));
    }

    #[test]
    fn resolve_plugin_ambiguous() {
        let plugins = vec![
            sample_plugin("auth-copilot", "Copilot Auth", vec![]),
            sample_plugin("auth-gitlab", "GitLab Auth", vec![]),
        ];
        let result = resolve_plugin(&plugins, "auth-");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Ambiguous"));
    }

    #[test]
    fn format_plugin_line_no_hooks_shows_no_capabilities() {
        let plugin = sample_plugin("bare", "Bare", vec![]);
        let line = format_plugin_line(&plugin, true);
        assert!(line.contains("no capabilities"));
    }

    #[test]
    fn format_plugin_detail_shows_description() {
        let mut plugin = sample_plugin("desc", "Desc Plugin", vec![]);
        plugin.description = Some("A detailed description".to_string());
        let lines = format_plugin_detail(&plugin, true);
        assert!(lines.iter().any(|l| l.contains("A detailed description")));
    }

    #[test]
    fn format_plugin_detail_shows_author() {
        let plugin = sample_plugin("auth-p", "Auth P", vec![]);
        let lines = format_plugin_detail(&plugin, true);
        assert!(lines.iter().any(|l| l.contains("Test Author")));
    }

    #[test]
    fn disabled_state_roundtrip() {
        let mut disabled = std::collections::HashSet::new();
        disabled.insert("test-plugin".to_string());
        disabled.insert("another-plugin".to_string());
        let result = save_disabled_set(&disabled);
        assert!(result.is_ok());

        let loaded = load_disabled_set();
        assert!(loaded.contains("test-plugin"));
        assert!(loaded.contains("another-plugin"));
        assert_eq!(loaded.len(), 2);

        // Cleanup
        let _ = std::fs::remove_file(disabled_state_path());
    }

    #[test]
    fn disabled_state_empty_file() {
        let path = disabled_state_path();
        let _ = std::fs::remove_file(&path);
        let loaded = load_disabled_set();
        assert!(loaded.is_empty());
    }

    #[test]
    fn resolve_plugin_case_insensitive() {
        let plugins = vec![sample_plugin("MyPlugin", "MyPlugin", vec![])];
        let result = resolve_plugin(&plugins, "myplugin");
        assert!(result.is_ok());
    }

    #[test]
    fn format_plugin_line_with_multiple_hooks() {
        let plugin = sample_plugin(
            "multi",
            "Multi",
            vec!["auth.resolve", "tool.before", "tool.after"],
        );
        let line = format_plugin_line(&plugin, true);
        assert!(line.contains("3 hooks"));
    }
}
