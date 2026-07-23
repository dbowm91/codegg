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
    let mut lines: Vec<String> = Vec::new();

    if report.connected {
        lines.push(format!(
            "search: {} OK ({})",
            report.search_backend.as_deref().unwrap_or("?"),
            report.tools.join(", ")
        ));
    } else if let Some(err) = &report.connection_error {
        lines.push(format!(
            "search: {} unavailable ({err})",
            report.search_backend.as_deref().unwrap_or("?")
        ));
    } else {
        lines.push(format!(
            "search: {} (no MCP service)",
            report.search_backend.as_deref().unwrap_or("?")
        ));
    }

    // Deterministic tools status
    let integrated = crate::tool::integrated_config::resolve_integrated_config(&config);
    if let Some(det) = &integrated.deterministic {
        if det.enabled {
            lines.push(format!(
                "deterministic: {} profile={}",
                det.backend, det.profile
            ));
        } else {
            lines.push("deterministic: disabled".to_string());
        }
    }

    if let Some(pf) = &integrated.preflight {
        lines.push(format!("preflight: mode={}", pf.mode));
    }

    let summary = lines.join("\n");
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
            let mut lines: Vec<String> = Vec::new();

            if report.connected {
                lines.push(format!(
                    "search: {} OK ({})",
                    report.search_backend.as_deref().unwrap_or("?"),
                    report.tools.join(", ")
                ));
            } else if let Some(err) = &report.connection_error {
                lines.push(format!(
                    "search: {} unavailable ({err})",
                    report.search_backend.as_deref().unwrap_or("?")
                ));
            } else {
                lines.push(format!(
                    "search: {} (no MCP service)",
                    report.search_backend.as_deref().unwrap_or("?")
                ));
            }
            for line in report.summary_lines() {
                tracing::info!(target: "codegg::doctor", "{}", line);
            }
            if let Some(mcp) = config.mcp.as_ref() {
                tracing::info!(target: "codegg::doctor", "MCP servers: {}", mcp.len());
            }

            // Deterministic tools status
            let integrated = crate::tool::integrated_config::resolve_integrated_config(&config);
            if let Some(det) = &integrated.deterministic {
                if det.enabled {
                    lines.push(format!(
                        "deterministic: {} profile={}",
                        det.backend, det.profile
                    ));
                } else {
                    lines.push("deterministic: disabled".to_string());
                }
            } else {
                lines.push("deterministic: not configured".to_string());
            }

            // Preflight status
            if let Some(pf) = &integrated.preflight {
                lines.push(format!("preflight: mode={}", pf.mode));
            }

            let summary = lines.join("\n");
            let is_error = !report.connected;
            Some(TuiCommand::DoctorResult { summary, is_error })
        },
    );
}

pub(crate) fn apply_doctor_result(app: &mut App, summary: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&summary);
    } else {
        let lines: Vec<String> = summary.lines().map(|s| s.to_string()).collect();
        if lines.len() > 2 {
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::DoctorReport,
                lines,
            );
        } else {
            app.messages_state.toasts.info(&summary);
        }
    }
}

pub(crate) fn handle_tool_contracts(app: &mut App) {
    use crate::tool::broker::ToolBroker;
    use crate::tool::contract::ToolCallerPolicy;

    let registry = crate::tool::ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&registry);
    let catalog = broker.catalog();

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("Tool contracts ({} tools):", catalog.len()));
    lines.push(String::new());

    let mut names: Vec<&str> = catalog.tool_names().collect();
    names.sort();

    for name in names {
        let contract = catalog.get(name).unwrap();
        let caller = match contract.caller_policy {
            ToolCallerPolicy::DirectOnly => "direct",
            ToolCallerPolicy::DirectOrProgrammatic => "direct+program",
            ToolCallerPolicy::ProgrammaticOnly => "program-only",
        };
        let effect = format!("{:?}", contract.effect_class);
        let idempotent = format!("{:?}", contract.idempotency);
        lines.push(format!(
            "  {:<24} caller={:<16} effect={:<20} idempotent={}",
            name, caller, effect, idempotent
        ));
    }

    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::DoctorReport,
        lines,
    );
}
