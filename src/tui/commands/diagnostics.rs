//! Diagnostics and doctor command handlers.

use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

#[allow(dead_code)]
pub(crate) async fn handle_run_doctor(app: &mut App) {
    use crate::search_backend::bootstrap;
    let config = match crate::config::schema::Config::load() {
        Ok(c) => c,
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("doctor: failed to load config: {e}"));
            return;
        }
    };
    let (_svc, report) = bootstrap::bootstrap_search_backend(&config).await;
    let summary = if report.connected {
        format!(
            "doctor: {} OK ({})",
            report.search_backend.as_deref().unwrap_or("?"),
            report.tools.join(", ")
        )
    } else if let Some(err) = &report.connection_error {
        format!(
            "doctor: {} unavailable ({err})",
            report.search_backend.as_deref().unwrap_or("?")
        )
    } else {
        format!(
            "doctor: {} (no MCP service)",
            report.search_backend.as_deref().unwrap_or("?")
        )
    };
    for line in report.summary_lines() {
        tracing::info!(target: "codegg::doctor", "{}", line);
    }
    if let Some(mcp) = config.mcp.as_ref() {
        tracing::info!(target: "codegg::doctor", "MCP servers: {}", mcp.len());
    }
    app.messages_state.toasts.info(&summary);
}

pub(crate) fn start_run_doctor(app: &mut App) {
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "run_doctor",
        async move {
            use crate::search_backend::bootstrap;
            let config = match crate::config::schema::Config::load() {
                Ok(c) => c,
                Err(e) => {
                    return Some(TuiCommand::DoctorResult {
                        summary: format!("doctor: failed to load config: {e}"),
                        is_error: true,
                    });
                }
            };
            let (_svc, report) = bootstrap::bootstrap_search_backend(&config).await;
            let summary = if report.connected {
                format!(
                    "doctor: {} OK ({})",
                    report.search_backend.as_deref().unwrap_or("?"),
                    report.tools.join(", ")
                )
            } else if let Some(err) = &report.connection_error {
                format!(
                    "doctor: {} unavailable ({err})",
                    report.search_backend.as_deref().unwrap_or("?")
                )
            } else {
                format!(
                    "doctor: {} (no MCP service)",
                    report.search_backend.as_deref().unwrap_or("?")
                )
            };
            for line in report.summary_lines() {
                tracing::info!(target: "codegg::doctor", "{}", line);
            }
            if let Some(mcp) = config.mcp.as_ref() {
                tracing::info!(target: "codegg::doctor", "MCP servers: {}", mcp.len());
            }
            Some(TuiCommand::DoctorResult {
                summary,
                is_error: false,
            })
        },
    );
}

pub(crate) fn apply_doctor_result(app: &mut App, summary: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&summary);
    } else {
        app.messages_state.toasts.info(&summary);
    }
}
