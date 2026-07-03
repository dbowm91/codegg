//! Plugin management command handlers for the TUI.
//!
//! Provides list/info/enable/disable/install/remove/doctor operations
//! for installed plugins. Uses the canonical [`PluginManager`] (wrapping
//! [`PluginService`]/[`PluginRegistry`] with builtins) as the source of
//! truth. Enable/disable state is held in the live registry and is
//! runtime-only until a persistence backend lands.
//!
//! All handlers are `pub(crate) fn` (non-async) and spawn background
//! tasks via [`spawn_tui_task`] for any I/O, posting a typed
//! [`TuiCommand`] completion variant back through the command channel.

use crate::plugin::management::PluginDoctorReport;
use crate::plugin::management_ui::{
    doctor_report_node, node_to_lines, plugin_info_node, plugins_table,
};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;

// ---------------------------------------------------------------------------
// Command handlers (spawn + completion pattern)
// ---------------------------------------------------------------------------

/// List all registered plugins (builtins + installed).
pub(crate) fn show_plugins(app: &mut App) {
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_list", async move {
        let views = mgr.list().await;

        if views.is_empty() {
            return Some(TuiCommand::PluginListFinished {
                lines: vec!["No plugins registered.".to_string()],
                error: None,
            });
        }

        let node = plugins_table(&views);
        let mut lines = node_to_lines(&node);
        lines.insert(0, format!("Registered plugins ({}):", views.len()));
        lines.insert(1, String::new());
        lines.insert(
            2,
            "Note: enable/disable is runtime-only until persistence lands.".to_string(),
        );
        lines.insert(3, String::new());

        Some(TuiCommand::PluginListFinished { lines, error: None })
    });
}

/// Show detailed info for a single plugin.
pub(crate) fn show_plugin_info(app: &mut App, query: &str) {
    let query = query.to_string();
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_info", async move {
        match mgr.info(&query).await {
            Ok(view) => {
                let node = plugin_info_node(&view);
                let lines = node_to_lines(&node);
                Some(TuiCommand::PluginInfoFinished {
                    plugin_id: view.id,
                    lines,
                    error: None,
                })
            }
            Err(e) => Some(TuiCommand::PluginInfoFinished {
                plugin_id: query,
                lines: Vec::new(),
                error: Some(e.to_string()),
            }),
        }
    });
}

/// Enable a plugin by selector.
pub(crate) fn enable_plugin(app: &mut App, query: &str) {
    let query = query.to_string();
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_enable", async move {
        match mgr.enable(&query).await {
            Ok(view) => Some(TuiCommand::PluginEnableFinished {
                plugin_id: view.id,
                error: None,
            }),
            Err(e) => Some(TuiCommand::PluginEnableFinished {
                plugin_id: query,
                error: Some(e.to_string()),
            }),
        }
    });
}

/// Disable a plugin by selector.
pub(crate) fn disable_plugin(app: &mut App, query: &str) {
    let query = query.to_string();
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_disable", async move {
        match mgr.disable(&query).await {
            Ok(view) => Some(TuiCommand::PluginDisableFinished {
                plugin_id: view.id,
                error: None,
            }),
            Err(e) => Some(TuiCommand::PluginDisableFinished {
                plugin_id: query,
                error: Some(e.to_string()),
            }),
        }
    });
}

/// Run plugin diagnostics.
///
/// When `query` is `None`, diagnostics run for all registered plugins.
/// When `Some(name)`, diagnostics run for the matching plugin only.
pub(crate) fn doctor_plugin(app: &mut App, query: Option<&str>) {
    let query = query.map(|s| s.to_string());
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_doctor", async move {
        let reports: Vec<PluginDoctorReport> = match query {
            Some(ref q) => match mgr.doctor(q).await {
                Ok(report) => vec![report],
                Err(e) => {
                    return Some(TuiCommand::PluginDoctorFinished {
                        lines: Vec::new(),
                        error: Some(e.to_string()),
                    });
                }
            },
            None => mgr.doctor_all().await,
        };

        let mut all_lines = Vec::new();
        all_lines.push(format!(
            "Plugin doctor: checking {} plugin(s)",
            reports.len()
        ));
        all_lines.push(String::new());

        for report in &reports {
            let node = doctor_report_node(report);
            let mut node_lines = node_to_lines(&node);
            all_lines.append(&mut node_lines);
            all_lines.push(String::new());
        }

        all_lines.push("Diagnostics complete.".to_string());

        Some(TuiCommand::PluginDoctorFinished {
            lines: all_lines,
            error: None,
        })
    });
}

/// Remove (uninstall) an installed plugin.
///
/// Unregisters from the live registry and removes the filesystem
/// directory if it is under the canonical plugins directory.
pub(crate) fn remove_plugin(app: &mut App, query: &str) {
    let query = query.to_string();
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_remove", async move {
        match mgr.uninstall(&query).await {
            Ok(result) => Some(TuiCommand::PluginRemoveFinished {
                plugin_id: result.view.id,
                removed_files: result.removed_files,
                install_path: result
                    .install_path
                    .as_ref()
                    .map(|p| p.display().to_string()),
                warning: result.warning,
                error: None,
            }),
            Err(e) => Some(TuiCommand::PluginRemoveFinished {
                plugin_id: query,
                removed_files: false,
                install_path: None,
                warning: None,
                error: Some(e.to_string()),
            }),
        }
    });
}

/// Install a plugin from a local filesystem path.
///
/// The source directory must contain a `manifest.toml`. The plugin is
/// copied into the canonical plugins directory and registered in the
/// live registry.
pub(crate) fn install_plugin(app: &mut App, source_path: &str) {
    let source_path = source_path.to_string();
    let Some(mgr) = app.plugin_manager.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_tui_task(tx, "plugin_install", async move {
        let path = std::path::PathBuf::from(&source_path);
        match mgr.install_from_path(&path).await {
            Ok(view) => {
                let mut lines = vec![format!("Plugin '{}' installed and registered.", view.id)];
                lines.push(format!("Name:    {}", view.name));
                lines.push(format!("Version: {}", view.version));
                lines.push(format!("Runtime: {}", view.runtime_kind));
                if let Some(ref src) = view.source_path {
                    lines.push(format!("Install path: {}", src));
                }
                lines.push(format!(
                    "State:   {} (runtime-only; enable/disable does not persist)",
                    if view.enabled { "enabled" } else { "disabled" }
                ));
                if view.command_count > 0 {
                    lines.push(format!("Commands: {}", view.command_count));
                }
                if view.hook_count > 0 {
                    lines.push(format!("Hooks:    {}", view.hook_count));
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
pub(crate) fn apply_plugin_list_finished(app: &mut App, lines: Vec<String>, error: Option<String>) {
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
///
/// The TUI message distinguishes between four scenarios:
/// - files removed: "Plugin '<id>' unregistered and removed from <path>."
/// - no source path: "Plugin '<id>' unregistered. No install path was recorded,
///   so no files were removed."
/// - delete failed: "Plugin '<id>' unregistered, but failed to remove files: <error>."
/// - hard error: "Remove failed: <error>."
pub(crate) fn apply_plugin_remove_finished(
    app: &mut App,
    plugin_id: String,
    removed_files: bool,
    install_path: Option<String>,
    warning: Option<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state
            .toasts
            .error(&format!("Remove failed: {err}"));
        return;
    }

    if let Some(warn) = warning {
        let path_msg = install_path
            .as_deref()
            .map(|p| format!(" (target was {p})"))
            .unwrap_or_default();
        app.messages_state.toasts.warning(&format!(
            "Plugin '{plugin_id}' unregistered, but failed to remove files{path_msg}: {warn}"
        ));
        return;
    }

    if removed_files {
        let path = install_path.as_deref().unwrap_or("(unknown path)");
        app.messages_state.toasts.success(&format!(
            "Plugin '{plugin_id}' unregistered and removed from {path}"
        ));
        return;
    }

    // No files removed, no error. Either builtin or source-less plugin.
    app.messages_state.toasts.info(&format!(
        "Plugin '{plugin_id}' unregistered. No install path was recorded, so no files were removed."
    ));
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
    use crate::plugin::management::PluginManager;
    use crate::plugin::manifest::{PluginManifest, PluginRuntimeSpec, PluginTrustClass};
    use crate::plugin::registry::{PluginInfo, PluginRegistry};
    use crate::plugin::service::PluginService;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    /// Build a minimal PluginManager with a few test plugins registered.
    async fn make_test_manager() -> PluginManager {
        let registry = PluginRegistry::new();
        registry
            .register(PluginInfo {
                id: "test-plugin:1".into(),
                manifest: PluginManifest {
                    name: "test-plugin".into(),
                    version: "1.0.0".into(),
                    api_version: 1,
                    runtime: PluginRuntimeSpec::Builtin {
                        handler: "h".into(),
                    },
                    ..Default::default()
                },
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        registry
            .register(PluginInfo {
                id: "another-plugin:1".into(),
                manifest: PluginManifest {
                    name: "another-plugin".into(),
                    version: "2.0.0".into(),
                    api_version: 1,
                    runtime: PluginRuntimeSpec::Builtin {
                        handler: "h".into(),
                    },
                    ..Default::default()
                },
                enabled: false,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = std::sync::Arc::new(PluginService::new(std::sync::Arc::new(registry)));
        PluginManager::new(service)
    }

    /// Helper: create an App with a non-None plugin_manager for testing.
    async fn app_with_manager() -> App {
        let mut app = make_test_app();
        app.plugin_manager = Some(make_test_manager().await);
        app
    }

    // --- Apply handler tests ---

    #[test]
    fn apply_plugin_list_finished_error_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_list_finished(&mut app, Vec::new(), Some("boom".to_string()));
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Plugin list failed"));
        assert!(toasts[0].message.contains("boom"));
    }

    #[test]
    fn apply_plugin_list_finished_ok_routes_to_dialog() {
        let mut app = make_test_app();
        apply_plugin_list_finished(&mut app, vec!["a".into(), "b".into()], None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        let dialog_open = app.dialog_state.info_dialog.is_some();
        assert!(!toasts.is_empty() || dialog_open);
    }

    #[test]
    fn apply_plugin_info_finished_error_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_info_finished(
            &mut app,
            "missing-plugin".into(),
            Vec::new(),
            Some("not found".to_string()),
        );
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Plugin info"));
        assert!(toasts[0].message.contains("not found"));
    }

    #[test]
    fn apply_plugin_enable_finished_success_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_enable_finished(&mut app, "my-plugin".into(), None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("my-plugin"));
        assert!(toasts[0].message.contains("enabled"));
    }

    #[test]
    fn apply_plugin_enable_finished_error_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_enable_finished(&mut app, "my-plugin".into(), Some("nope".into()));
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Enable failed"));
    }

    #[test]
    fn apply_plugin_disable_finished_success_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_disable_finished(&mut app, "my-plugin".into(), None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("disabled"));
    }

    #[test]
    fn apply_plugin_remove_finished_files_removed_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_remove_finished(
            &mut app,
            "old-plugin".into(),
            true,
            Some("/data/codegg/plugins/old-plugin".into()),
            None,
            None,
        );
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0]
            .message
            .contains("unregistered and removed from /data/codegg/plugins/old-plugin"));
    }

    #[test]
    fn apply_plugin_remove_finished_no_source_path_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_remove_finished(&mut app, "old-plugin".into(), false, None, None, None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("No install path was recorded"));
    }

    #[test]
    fn apply_plugin_remove_finished_warning_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_remove_finished(
            &mut app,
            "old-plugin".into(),
            false,
            Some("/data/codegg/plugins/old-plugin".into()),
            Some("permission denied".into()),
            None,
        );
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("failed to remove files"));
        assert!(toasts[0].message.contains("permission denied"));
    }

    #[test]
    fn apply_plugin_remove_finished_error_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_remove_finished(
            &mut app,
            "old-plugin".into(),
            false,
            None,
            None,
            Some("not found".into()),
        );
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Remove failed"));
        assert!(toasts[0].message.contains("not found"));
    }

    #[test]
    fn apply_plugin_install_finished_error_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_install_finished(
            &mut app,
            "/src/path".into(),
            Vec::new(),
            Some("invalid manifest".into()),
        );
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Install failed"));
    }

    #[test]
    fn apply_plugin_doctor_finished_error_routes_to_toast() {
        let mut app = make_test_app();
        apply_plugin_doctor_finished(&mut app, Vec::new(), Some("missing".into()));
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Plugin doctor"));
    }

    #[test]
    fn apply_plugin_doctor_finished_ok_with_lines() {
        let mut app = make_test_app();
        let lines = vec![
            "Plugin doctor: checking 1 plugin(s)".to_string(),
            String::new(),
            "== Plugin Doctor: Test ==".to_string(),
            "1 checks, 1 passed, 0 failed".to_string(),
            "[PASS] manifest_present: ok".to_string(),
        ];
        apply_plugin_doctor_finished(&mut app, lines, None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        let dialog_open = app.dialog_state.info_dialog.is_some();
        assert!(!toasts.is_empty() || dialog_open);
    }

    #[test]
    fn apply_plugin_disable_finished_success_shows_builtin_name() {
        let mut app = make_test_app();
        apply_plugin_disable_finished(&mut app, "builtin:codex".into(), None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("builtin:codex"));
        assert!(toasts[0].message.contains("disabled"));
    }

    #[test]
    fn apply_plugin_enable_finished_error_surfaces_conflict() {
        let mut app = make_test_app();
        apply_plugin_enable_finished(
            &mut app,
            "plugin-a".into(),
            Some("duplicate command 'deploy' already registered by plugin-b".into()),
        );
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].message.contains("Enable failed"));
        assert!(toasts[0].message.contains("duplicate command"));
    }

    // --- PluginManager integration smoke tests ---

    #[test]
    fn app_new_for_testing_has_no_plugin_manager_by_default() {
        let app = make_test_app();
        // new_for_testing initializes plugin_manager as None (lazy init)
        assert!(app.plugin_manager.is_none());
    }

    #[tokio::test]
    async fn app_with_manager_has_non_none_plugin_manager() {
        let app = app_with_manager().await;
        assert!(app.plugin_manager.is_some());
    }

    #[tokio::test]
    async fn plugin_manager_list_returns_test_plugins() {
        let mgr = make_test_manager().await;
        let views = mgr.list().await;
        assert_eq!(views.len(), 2);
        let ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
        assert!(ids.contains(&"test-plugin:1"));
        assert!(ids.contains(&"another-plugin:1"));
    }

    #[tokio::test]
    async fn plugin_manager_enable_disable() {
        let mgr = make_test_manager().await;
        let view = mgr.disable("test-plugin:1").await.unwrap();
        assert!(!view.enabled);
        let view = mgr.enable("test-plugin:1").await.unwrap();
        assert!(view.enabled);
    }

    #[tokio::test]
    async fn plugin_manager_info_resolves() {
        let mgr = make_test_manager().await;
        let view = mgr.info("test-plugin:1").await.unwrap();
        assert_eq!(view.id, "test-plugin:1");
        assert_eq!(view.name, "test-plugin");
    }

    #[tokio::test]
    async fn plugin_manager_doctor_all() {
        let mgr = make_test_manager().await;
        let reports = mgr.doctor_all().await;
        assert_eq!(reports.len(), 2);
        for report in &reports {
            assert!(!report.checks.is_empty());
        }
    }

    #[tokio::test]
    async fn plugin_manager_doctor_single() {
        let mgr = make_test_manager().await;
        let report = mgr.doctor("test-plugin:1").await.unwrap();
        assert_eq!(report.plugin_id, "test-plugin:1");
        assert!(!report.checks.is_empty());
    }

    // --- Handler smoke tests (no panic) ---

    #[tokio::test]
    async fn show_plugins_does_not_panic_with_manager() {
        let mut app = app_with_manager().await;
        show_plugins(&mut app);
    }

    #[test]
    fn show_plugins_toasts_when_no_manager() {
        let mut app = make_test_app();
        show_plugins(&mut app);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert!(toasts.iter().any(|t| t.message.contains("not available")));
    }

    #[test]
    fn enable_plugin_toasts_when_no_manager() {
        let mut app = make_test_app();
        enable_plugin(&mut app, "codex");
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert!(toasts.iter().any(|t| t.message.contains("not available")));
    }

    #[test]
    fn disable_plugin_toasts_when_no_manager() {
        let mut app = make_test_app();
        disable_plugin(&mut app, "codex");
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert!(toasts.iter().any(|t| t.message.contains("not available")));
    }

    #[test]
    fn doctor_plugin_toasts_when_no_manager() {
        let mut app = make_test_app();
        doctor_plugin(&mut app, None);
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert!(toasts.iter().any(|t| t.message.contains("not available")));
    }

    #[test]
    fn remove_plugin_toasts_when_no_manager() {
        let mut app = make_test_app();
        remove_plugin(&mut app, "codex");
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert!(toasts.iter().any(|t| t.message.contains("not available")));
    }

    #[test]
    fn install_plugin_toasts_when_no_manager() {
        let mut app = make_test_app();
        install_plugin(&mut app, "/some/path");
        let toasts: Vec<_> = app.messages_state.toasts.iter().collect();
        assert!(toasts.iter().any(|t| t.message.contains("not available")));
    }
}
