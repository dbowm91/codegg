//! M007-G (ADR-0057): the TUI's half of the daemon's plugin catalogue. With a
//! daemon attached it is the **single authority**, so the three read-only
//! forms render the catalogue the daemon published, and the four mutating
//! forms travel to the daemon as `command.execute` requests.

use crate::protocol::core::CoreResponse;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::task_lifecycle::TuiTaskKind;

pub(crate) fn daemon_owns_plugins(app: &App) -> bool {
    app.core_client.is_some()
}

fn published_catalogue(
    app: &App,
) -> Option<Vec<crate::protocol::projection::dto::PluginCatalogueEntryProjection>> {
    app.projection_client
        .active_snapshot()
        .map(|(_, snapshot)| snapshot.plugin_catalogue.iter().cloned().collect())
}

/// Render the published catalogue as the `/plugins` list.
pub(crate) fn show_published_plugins(app: &mut App) {
    let Some(entries) = published_catalogue(app) else {
        app.messages_state
            .toasts
            .warning("The daemon's plugin catalogue is unavailable");
        return;
    };
    let mut lines = vec![
        format!("Registered plugins ({}):", entries.len()),
        String::new(),
    ];
    if entries.is_empty() {
        lines.push("No plugins registered.".to_string());
    } else {
        for entry in &entries {
            lines.push(format!(
                "  {:<28} {:<10} {}",
                entry.plugin_id,
                entry.version,
                if entry.enabled { "enabled" } else { "disabled" }
            ));
        }
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}

pub(crate) fn show_published_plugin_info(app: &mut App, query: &str) {
    let Some(entries) = published_catalogue(app) else {
        app.messages_state
            .toasts
            .warning("The daemon's plugin catalogue is unavailable");
        return;
    };
    let query = query.to_ascii_lowercase();
    let matches: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry.plugin_id.to_ascii_lowercase() == query
                || entry.name.to_ascii_lowercase() == query
        })
        .collect();
    if matches.is_empty() {
        app.messages_state
            .toasts
            .error(&format!("No plugin matches '{query}'"));
        return;
    }
    let mut lines = Vec::new();
    for entry in matches {
        lines.push(format!("Plugin: {}", entry.plugin_id));
        lines.push(format!("  name: {}", entry.name));
        lines.push(format!("  version: {}", entry.version));
        lines.push(format!("  runtime: {}", entry.runtime_kind));
        lines.push(format!("  trust: {}", entry.trust));
        lines.push(format!("  enabled: {}", entry.enabled));
        lines.push(format!("  commands: {}", entry.command_count));
        lines.push(format!("  hooks: {}", entry.hook_count));
        lines.push(format!("  diagnostics: {}", entry.diagnostic_count));
        if entry.has_error {
            lines.push("  the plugin carries an error-level diagnostic".to_string());
        }
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}

pub(crate) fn show_published_plugin_doctor(app: &mut App, query: Option<&str>) {
    let Some(entries) = published_catalogue(app) else {
        app.messages_state
            .toasts
            .warning("The daemon's plugin catalogue is unavailable");
        return;
    };
    let query = query.map(|value| value.to_ascii_lowercase());
    let selected: Vec<_> = entries
        .iter()
        .filter(|entry| match &query {
            Some(value) => {
                entry.plugin_id.to_ascii_lowercase() == *value
                    || entry.name.to_ascii_lowercase() == *value
            }
            None => true,
        })
        .collect();
    let mut lines = vec![
        format!("Plugin diagnostics ({}):", selected.len()),
        String::new(),
    ];
    if selected.is_empty() {
        lines.push("No plugin matches the selector.".to_string());
    }
    for entry in selected {
        lines.push(format!(
            "  {:<28} diagnostics: {:<4} {}",
            entry.plugin_id,
            entry.diagnostic_count,
            if entry.has_error {
                "error-level diagnostic present"
            } else {
                "no error-level diagnostic"
            }
        ));
    }
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}

/// Issue one of the four mutating plugin forms to the daemon. The request
/// travels as a `command.execute`, so the daemon records it as a canonical
/// `Run` and authorizes the inner request with `plugin.manage`.
pub(crate) fn request_plugin_manage(app: &mut App, command_id: &'static str, argument: &str) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Plugin manager not available");
        return;
    };
    let scope = crate::tui::command_route::scope_for(app);
    let argument = argument.to_string();
    let tx = app.tui_cmd_tx.clone();
    crate::tui::async_cmd::spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "plugin_manage",
        async move {
            let response = crate::tui::command_route::send_command(
                &core_client,
                scope,
                command_id,
                "",
                &argument,
            )
            .await;
            Some(plugin_manage_outcome(command_id, response))
        },
    );
}

fn plugin_manage_outcome(
    command_id: &str,
    response: Result<CoreResponse, crate::error::AppError>,
) -> TuiCommand {
    let result = match response {
        Ok(CoreResponse::PluginManageResult { result }) => result,
        Ok(CoreResponse::Error { code, message }) => {
            return failure_outcome(command_id, format!("{code}: {message}"))
        }
        Ok(CoreResponse::CommandExecute { result }) => {
            let (code, reason) = crate::tui::command_route::refusal_parts(&result);
            return failure_outcome(command_id, format!("{code}: {reason}"));
        }
        Ok(_) => {
            return failure_outcome(
                command_id,
                "the daemon returned an unexpected response".to_string(),
            )
        }
        Err(error) => return failure_outcome(command_id, error.to_string()),
    };
    let message = result.message.clone();
    match command_id {
        "plugin_enable" => TuiCommand::PluginEnableFinished {
            plugin_id: result.plugin_id,
            error: None,
        },
        "plugin_disable" => TuiCommand::PluginDisableFinished {
            plugin_id: result.plugin_id,
            error: None,
        },
        "plugin_remove" => TuiCommand::PluginRemoveFinished {
            plugin_id: result.plugin_id,
            removed_files: !result.removed_paths.is_empty(),
            install_path: result.install_path.clone(),
            warning: None,
            error: None,
        },
        _ => TuiCommand::PluginInstallFinished {
            source: result.plugin_id.clone(),
            lines: vec![message],
            error: None,
        },
    }
}

fn failure_outcome(command_id: &str, error: String) -> TuiCommand {
    match command_id {
        "plugin_enable" => TuiCommand::PluginEnableFinished {
            plugin_id: String::new(),
            error: Some(error),
        },
        "plugin_disable" => TuiCommand::PluginDisableFinished {
            plugin_id: String::new(),
            error: Some(error),
        },
        "plugin_remove" => TuiCommand::PluginRemoveFinished {
            plugin_id: String::new(),
            removed_files: false,
            install_path: None,
            warning: None,
            error: Some(error),
        },
        _ => TuiCommand::PluginInstallFinished {
            source: String::new(),
            lines: Vec::new(),
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::error::AppError;
    use crate::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
    use crate::protocol::projection::dto::PluginCatalogueEntryProjection;
    use crate::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use crate::protocol::projection::snapshot::SessionProjectionSnapshot;
    use std::sync::{Arc, Mutex};

    fn entry(plugin_id: &str, name: &str, enabled: bool) -> PluginCatalogueEntryProjection {
        PluginCatalogueEntryProjection {
            plugin_id: plugin_id.to_string(),
            name: name.to_string(),
            version: "1.2.3".to_string(),
            enabled,
            trust: "builtin".to_string(),
            runtime_kind: "builtin".to_string(),
            command_count: 2,
            hook_count: 1,
            diagnostic_count: 3,
            has_error: false,
            source_path: None,
        }
    }

    fn app_with_catalogue(entries: Vec<PluginCatalogueEntryProjection>) -> App {
        let mut app = App::new_for_testing("/tmp".to_string());
        let mut snapshot = SessionProjectionSnapshot::empty("sess", "proj", "ws");
        snapshot.set_plugin_catalogue(entries);
        app.projection_client.controller_mut().install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            ProjectionStreamDescriptor {
                stream_id: ProjectionStreamId::new("stream-1").expect("a valid stream id"),
                kind: ProjectionStreamKind::Session,
                project_id: "proj".into(),
                workspace_id: Some("ws".into()),
                session_id: Some("sess".into()),
                projection_version: 1,
                retention_floor_seq: 0,
                high_water_seq: 0,
                latest_checkpoint_seq: None,
            },
            snapshot,
        );
        app
    }

    fn shown_text(app: &mut App) -> String {
        let dialog = app
            .focus_manager
            .dialog_mut_any::<crate::tui::components::dialogs::info::InfoDialog>()
            .map(|dialog| dialog.content_lines().join("\n"))
            .unwrap_or_default();
        let toasts = app
            .messages_state
            .toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        format!("{dialog}\n{toasts}")
    }

    #[test]
    fn the_plugin_list_renders_the_published_catalogue() {
        let mut app = app_with_catalogue(vec![
            entry("plugin:alpha", "alpha", true),
            entry("plugin:beta", "beta", false),
        ]);

        show_published_plugins(&mut app);

        let text = shown_text(&mut app);
        assert!(
            text.contains("Registered plugins (2):"),
            "the count is the published collection's: {text}"
        );
        assert!(text.contains("plugin:alpha"), "the row names it: {text}");
        assert!(text.contains("enabled"), "the row states it: {text}");
        assert!(text.contains("disabled"), "the row states it: {text}");
    }

    #[test]
    fn the_plugin_info_form_reads_the_published_catalogue() {
        let mut app = app_with_catalogue(vec![
            entry("plugin:alpha", "alpha", true),
            entry("plugin:beta", "beta", false),
        ]);

        show_published_plugin_info(&mut app, "beta");

        let text = shown_text(&mut app);
        assert!(
            text.contains("plugin:beta"),
            "the report names the plugin it matched: {text}"
        );
        assert!(
            text.contains("version: 1.2.3"),
            "the report carries the published version: {text}"
        );
        assert!(
            !text.contains("plugin:alpha"),
            "the report names one plugin: {text}"
        );
    }

    #[test]
    fn the_plugin_doctor_form_reads_the_published_catalogue() {
        let mut app = app_with_catalogue(vec![entry("plugin:alpha", "alpha", true)]);

        show_published_plugin_doctor(&mut app, None);

        let text = shown_text(&mut app);
        assert!(
            text.contains("Plugin diagnostics (1):"),
            "the count is the published collection's: {text}"
        );
        assert!(
            text.contains("plugin:alpha"),
            "the row names the plugin: {text}"
        );
    }

    /// A daemon that records the requests it is given.
    #[derive(Default)]
    struct RecordingDaemon {
        seen: Mutex<Vec<CoreRequest>>,
    }

    #[async_trait::async_trait]
    impl CoreClient for RecordingDaemon {
        async fn request(
            &self,
            request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, AppError> {
            self.seen
                .lock()
                .expect("the recorder is not poisoned")
                .push(request.payload);
            Ok(CoreResponse::PluginManageResult {
                result: crate::protocol::plugin_catalogue::PluginManageResultDto {
                    action: crate::protocol::plugin_catalogue::PluginManageActionDto::Enable,
                    plugin_id: "plugin:alpha".to_string(),
                    enabled: Some(true),
                    removed_paths: Vec::new(),
                    install_path: None,
                    message: "the plugin is enabled".to_string(),
                },
            })
        }

        fn subscribe(
            &self,
        ) -> tokio::sync::mpsc::Receiver<
            crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>,
        > {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    #[tokio::test]
    async fn the_mutating_plugin_forms_travel_to_the_daemon() {
        for command_id in [
            "plugin_enable",
            "plugin_disable",
            "plugin_remove",
            "plugin_install",
        ] {
            let daemon = Arc::new(RecordingDaemon::default());
            let mut app = App::new_for_testing("/tmp".to_string());
            let (tx, mut rx) = tokio::sync::mpsc::channel(16);
            app.tui_cmd_tx = Some(tx);
            app.set_core_client(daemon.clone());

            request_plugin_manage(&mut app, command_id, "probe-plugin");

            let command = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("the daemon answered in time")
                .expect("the task produced a completion command");
            assert!(
                matches!(
                    command,
                    TuiCommand::PluginEnableFinished { .. }
                        | TuiCommand::PluginDisableFinished { .. }
                        | TuiCommand::PluginRemoveFinished { .. }
                        | TuiCommand::PluginInstallFinished { .. }
                ),
                "{command_id} must complete through a plugin completion command"
            );

            let seen = daemon
                .seen
                .lock()
                .expect("the recorder is not poisoned")
                .clone();
            assert_eq!(seen.len(), 1, "{command_id} must issue one request");
            let CoreRequest::CommandExecute { request } = &seen[0] else {
                panic!("{command_id} must travel as a command execution");
            };
            assert_eq!(request.command_id, command_id);
            assert_eq!(request.argument, "probe-plugin");
        }
    }

    #[test]
    fn the_tui_keeps_its_own_manager_when_no_daemon_is_attached() {
        let app = App::new_for_testing("/tmp".to_string());
        assert!(
            !daemon_owns_plugins(&app),
            "a TUI with no daemon attached owns its own plugin registry"
        );
    }
}
