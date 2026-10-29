//! Diagnostics and doctor command handlers.

use crate::protocol::projection::dto::ToolContractProjection;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// How long the cached footer LSP status is reused.
pub const LSP_STATUS_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Enqueue an LSP status refresh when the cached value has aged out.
///
/// The status is derived from the live LSP service, so computing it awaits
/// that service. Doing that from `render` would require `Handle::block_on`,
/// which panics inside the event loop's runtime context, so the fetch runs
/// as a registered background task and the render pass reads the cache.
pub(crate) fn maybe_refresh_lsp_status(app: &mut App) {
    if app.lsp_tool.is_none() && app.core_client.is_none() {
        return;
    }
    let due = app
        .lsp_status_requested_at
        .map(|at| at.elapsed() >= LSP_STATUS_REFRESH_INTERVAL)
        .unwrap_or(true);
    if !due {
        return;
    }
    app.lsp_status_requested_at = Some(std::time::Instant::now());
    app.enqueue_tui_command(TuiCommand::LspStatusRefreshRequested);
}

/// Start the off-render-path LSP status fetch.
pub(crate) fn refresh_lsp_status(app: &mut App) {
    if let Some(client) = app.core_client.clone() {
        let scope = crate::tui::command_route::scope_for(app);
        let tx = app.tui_cmd_tx.clone();
        spawn_registered_tui_task(
            tx,
            &mut app.task_registry,
            TuiTaskKind::Command,
            "lsp_status_footer",
            async move {
                let request = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::protocol::core::CoreRequest::LspServerStatusGet {
                        request: crate::protocol::lsp::LspServerStatusGetRequestDto {
                            session_id: scope.session_id.unwrap_or_default(),
                            server_id: None,
                        },
                    },
                );
                let status = match client.request(request).await {
                    Ok(crate::protocol::core::CoreResponse::LspServerStatusGetResult {
                        result,
                    }) => result.servers.first().map(|server| {
                        format!(
                            "LSP: {} {} gen={}",
                            server.server_id, server.state, server.generation
                        )
                    }),
                    Err(_) => None,
                    _ => None,
                };
                Some(TuiCommand::LspStatusRefreshed { status })
            },
        );
        return;
    }
    let Some(lsp_tool) = app.lsp_tool.clone() else {
        app.lsp_status_cache = None;
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "lsp_status",
        async move {
            let status = lsp_tool.lsp_status_line().await;
            Some(TuiCommand::LspStatusRefreshed { status })
        },
    );
}

/// Start the off-render-path fetch of the multi-line `/lsp-status` toast.
pub(crate) fn request_lsp_status_detail(app: &mut App) {
    request_lsp_server_status(app, "lsp_status", None);
}

pub(crate) fn request_lsp_server_status(
    app: &mut App,
    command_id: &'static str,
    server_id: Option<String>,
) {
    if let Some(client) = app.core_client.clone() {
        let scope = crate::tui::command_route::scope_for(app);
        let tx = app.tui_cmd_tx.clone();
        spawn_registered_tui_task(
            tx,
            &mut app.task_registry,
            TuiTaskKind::Command,
            "lsp_server_status",
            async move {
                let request = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::tui::command_route::command_execute(
                        scope,
                        command_id,
                        "",
                        server_id.as_deref().unwrap_or_default(),
                    ),
                );
                let response = match client.request(request).await {
                    Ok(response) => crate::tui::command_route::unwrap_inner_result(Ok(response)),
                    Err(error) => Err(error),
                };
                let detail = match response {
                    Ok(crate::protocol::core::CoreResponse::LspServerStatusGetResult {
                        result,
                    }) => Some(format_lsp_server_status(
                        &result,
                        command_id,
                        server_id.as_deref(),
                    )),
                    Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => Some(message),
                    Ok(crate::protocol::core::CoreResponse::CommandExecute { result }) => {
                        let (_, message) = crate::tui::command_route::refusal_parts(&result);
                        Some(message)
                    }
                    Ok(other) => Some(format!("Unexpected LSP status response: {other:?}")),
                    Err(error) => Some(error.to_string()),
                };
                Some(TuiCommand::LspStatusDetailLoaded { detail })
            },
        );
        return;
    }
    let Some(lsp_tool) = app.lsp_tool.clone() else {
        app.messages_state.toasts.info("LSP not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "lsp_status_detail",
        async move {
            let detail = match (command_id, server_id) {
                ("lsp_servers", _) => lsp_tool.lsp_servers_detail().await,
                ("lsp_capabilities", Some(server_id)) => {
                    lsp_tool.lsp_capabilities_for_key(&server_id).await
                }
                ("lsp_errors", Some(server_id)) => lsp_tool.lsp_errors_for_key(&server_id).await,
                _ => lsp_tool.lsp_summary_detail().await,
            };
            Some(TuiCommand::LspStatusDetailLoaded { detail })
        },
    );
}

fn format_lsp_server_status(
    result: &crate::protocol::lsp::LspServerStatusResultDto,
    command_id: &str,
    server_id: Option<&str>,
) -> String {
    if result.server_not_found {
        return format!("Server not found: {}", server_id.unwrap_or_default());
    }
    if result.servers.is_empty() {
        return if server_id.is_some() {
            format!("No errors reported for {}", server_id.unwrap_or_default())
        } else {
            "No active LSP servers".to_string()
        };
    }
    result
        .servers
        .iter()
        .map(|server| {
            let mut line = format!(
                "{}: {} gen={} pending={} documents={}",
                server.server_id,
                server.state,
                server.generation,
                server.pending_requests,
                server.open_documents
            );
            if let Some(error) = server.last_error.as_deref() {
                line.push_str("\n  ");
                line.push_str(error);
            } else if server_id.is_some() {
                line.push_str("\n  No recent server error");
            }
            if command_id == "lsp_capabilities" {
                line.push_str("\n  capabilities: ");
                let capabilities = server.capabilities.join(", ");
                line.push_str(if capabilities.is_empty() {
                    "none reported"
                } else {
                    &capabilities
                });
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn request_lsp_restart(app: &mut App, server_id: String) {
    if let Some(client) = app.core_client.clone() {
        let scope = crate::tui::command_route::scope_for(app);
        let tx = app.tui_cmd_tx.clone();
        spawn_registered_tui_task(
            tx,
            &mut app.task_registry,
            TuiTaskKind::Command,
            "lsp_restart",
            async move {
                let request = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::tui::command_route::command_execute(
                        scope,
                        "lsp_restart",
                        "",
                        &server_id,
                    ),
                );
                let detail = match client.request(request).await {
                    Ok(response) => {
                        match crate::tui::command_route::unwrap_inner_result(Ok(response)) {
                            Ok(crate::protocol::core::CoreResponse::LspControlResult {
                                result,
                            }) => Some(format!(
                                "Restarted {} (generation {} → {}, {})",
                                result.server_id,
                                result.previous_generation,
                                result.generation,
                                result.state
                            )),
                            Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                                Some(message)
                            }
                            Ok(crate::protocol::core::CoreResponse::CommandExecute { result }) => {
                                Some(crate::tui::command_route::refusal_parts(&result).1)
                            }
                            Ok(other) => {
                                Some(format!("Unexpected LSP restart response: {other:?}"))
                            }
                            Err(error) => Some(error.to_string()),
                        }
                    }
                    Err(error) => Some(error.to_string()),
                };
                Some(TuiCommand::LspStatusDetailLoaded { detail })
            },
        );
        return;
    }
    let Some(lsp_tool) = app.lsp_tool.clone() else {
        app.messages_state.toasts.info("LSP not available");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "lsp_restart_standalone",
        async move {
            let detail = Some(lsp_tool.lsp_restart_server(&server_id).await);
            Some(TuiCommand::LspStatusDetailLoaded { detail })
        },
    );
}

pub(crate) fn request_lsp_control(app: &mut App, command_id: &'static str, argument: String) {
    let Some(client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("LSP control requires a daemon connection");
        return;
    };
    let scope = crate::tui::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        command_id,
        async move {
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                crate::tui::command_route::command_execute(scope, command_id, "", &argument),
            );
            let detail = match client.request(request).await {
                Ok(response) => {
                    match crate::tui::command_route::unwrap_inner_result(Ok(response)) {
                        Ok(crate::protocol::core::CoreResponse::LspControlResult { result }) => {
                            let text = match command_id {
                                "lsp_stop" => {
                                    format!("Stopped {} ({})", result.server_id, result.state)
                                }
                                "lsp_cache_status" => format!(
                                    "LSP semantic cache: {} entries, {} bytes ({})",
                                    result.cache_entries, result.cache_bytes, result.state
                                ),
                                "lsp_cache_clear" => format!(
                                    "Cleared {} LSP cache entries; {} remain",
                                    result.cleared_entries, result.cache_entries
                                ),
                                _ => format!("LSP control completed ({})", result.state),
                            };
                            Some(text)
                        }
                        Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                            Some(message)
                        }
                        Ok(crate::protocol::core::CoreResponse::CommandExecute { result }) => {
                            Some(crate::tui::command_route::refusal_parts(&result).1)
                        }
                        Ok(other) => Some(format!("Unexpected LSP control response: {other:?}")),
                        Err(error) => Some(error.to_string()),
                    }
                }
                Err(error) => Some(error.to_string()),
            };
            Some(TuiCommand::LspStatusDetailLoaded { detail })
        },
    );
}

pub(crate) fn request_lsp_workflow(app: &mut App, command_id: &'static str, argument: String) {
    let Some(client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("This LSP workflow requires the daemon connection");
        return;
    };
    let scope = crate::tui::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "lsp_workflow",
        async move {
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                crate::tui::command_route::command_execute(scope, command_id, "", &argument),
            );
            let detail = match client.request(request).await {
                Ok(response) => {
                    match crate::tui::command_route::unwrap_inner_result(Ok(response)) {
                        Ok(crate::protocol::core::CoreResponse::LspWorkflowResult { result }) => {
                            Some(result.rendered)
                        }
                        Ok(crate::protocol::core::CoreResponse::CommandExecute { result }) => {
                            let (_, message) = crate::tui::command_route::refusal_parts(&result);
                            Some(message)
                        }
                        Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                            Some(message)
                        }
                        Ok(other) => Some(format!("Unexpected LSP workflow response: {other:?}")),
                        Err(error) => Some(error.to_string()),
                    }
                }
                Err(error) => Some(error.to_string()),
            };
            Some(TuiCommand::LspStatusDetailLoaded { detail })
        },
    );
}

/// Eggwork M002: ask the daemon for the configured nodes' host telemetry.
///
/// The read is off the render path and goes through the shared guarded task
/// registry, so a late completion is droppable rather than stale.
pub(crate) fn request_node_telemetry(app: &mut App) {
    let Some(client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Not connected to a daemon; node telemetry is unavailable");
        return;
    };
    // M007-D: the scope names the project, workspace and session.
    let scope = crate::tui::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "node_telemetry",
        async move {
            // M007-D: the daemon owns the effect. The telemetry read runs as the
            // catalog's `nodes` command; the presented response is the daemon's.
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                crate::tui::command_route::command_execute(scope, "nodes", "", ""),
            );
            let (nodes, error) =
                match crate::tui::command_route::unwrap_inner_result(client.request(request).await)
                {
                    Ok(crate::protocol::core::CoreResponse::NodeTelemetry { nodes }) => {
                        (nodes, None)
                    }
                    Ok(crate::protocol::core::CoreResponse::Error { code, message }) => {
                        (Vec::new(), Some(format!("{code}: {message}")))
                    }
                    // An older daemon answers with some other shape. Treat that as an
                    // absent surface, not a failure the user must act on.
                    Ok(_) => (Vec::new(), None),
                    Err(error) => (Vec::new(), Some(error.to_string())),
                };
            Some(TuiCommand::NodeTelemetryLoaded { nodes, error })
        },
    );
}

/// Render the node telemetry snapshots as one bounded, readable summary.
///
/// The rendering keeps the node's own words for "not reported": a dimension
/// it could not measure shows as `unsupported`, never as `0`.
pub(crate) fn format_node_telemetry(nodes: &[crate::protocol::dto::NodeTelemetryDto]) -> String {
    if nodes.is_empty() {
        return "No remote nodes are configured".to_string();
    }
    let mut lines = Vec::with_capacity(nodes.len());
    for node in nodes {
        let mut line = format!("{} [{}]", node.node_id, telemetry_state_label(node.state));
        if let Some(age) = node.age_ms {
            line.push_str(&format!(" age {}ms", age));
        }
        if let Some(diagnostic) = node.diagnostic.as_deref() {
            line.push_str(&format!(" - {diagnostic}"));
        } else {
            line.push_str(&format!(
                " cpu {} mem {}/{}",
                metric_pct(
                    node.cpu.as_ref().map(|cpu| cpu.state),
                    node.cpu.as_ref().and_then(|cpu| cpu.usage_pct),
                ),
                metric_bytes(
                    node.memory.as_ref().map(|memory| memory.state),
                    node.memory.as_ref().and_then(|memory| memory.used_bytes),
                ),
                metric_bytes(
                    node.memory.as_ref().map(|memory| memory.state),
                    node.memory.as_ref().and_then(|memory| memory.total_bytes),
                ),
            ));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn telemetry_state_label(state: crate::protocol::dto::NodeTelemetryStateDto) -> &'static str {
    use crate::protocol::dto::NodeTelemetryStateDto;
    match state {
        NodeTelemetryStateDto::Disabled => "disabled",
        NodeTelemetryStateDto::Warming => "warming",
        NodeTelemetryStateDto::Ready => "ready",
        NodeTelemetryStateDto::Stale => "stale",
        NodeTelemetryStateDto::Unavailable => "unavailable",
        NodeTelemetryStateDto::Invalid => "invalid",
    }
}

/// Render a percentage metric, keeping "not reported" distinct from a real zero.
fn metric_pct(
    state: Option<crate::protocol::dto::NodeMetricStateDto>,
    value: Option<f64>,
) -> String {
    use crate::protocol::dto::NodeMetricStateDto;
    match state {
        Some(NodeMetricStateDto::Value) => match value {
            Some(value) => format!("{value:.1}%"),
            None => "n/a".to_string(),
        },
        Some(NodeMetricStateDto::Unknown) => "unknown".to_string(),
        Some(NodeMetricStateDto::Unsupported) => "unsupported".to_string(),
        None => "n/a".to_string(),
    }
}

/// Render a byte metric, keeping "not reported" distinct from a real zero.
fn metric_bytes(
    state: Option<crate::protocol::dto::NodeMetricStateDto>,
    value: Option<u64>,
) -> String {
    use crate::protocol::dto::NodeMetricStateDto;
    match state {
        Some(NodeMetricStateDto::Value) => match value {
            Some(value) => format!("{value}B"),
            None => "n/a".to_string(),
        },
        Some(NodeMetricStateDto::Unknown) => "unknown".to_string(),
        Some(NodeMetricStateDto::Unsupported) => "unsupported".to_string(),
        None => "n/a".to_string(),
    }
}

#[allow(dead_code)]
#[cfg(test)]
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

/// Render the tool contracts the daemon publishes for this session.
///
/// The published caller policy is the string this list has always shown
/// (`direct`, `direct+program`, `program-only`); the published effect and
/// idempotency are the contract's own names. The count is the published
/// collection's own.
pub(crate) fn handle_tool_contracts(app: &mut App) {
    let published = app
        .projection_client
        .active_snapshot()
        .map(|(_, snapshot)| snapshot.tool_contracts.clone());
    let Some(contracts) = published else {
        app.messages_state
            .toasts
            .warning("The daemon's tool contracts are unavailable");
        return;
    };

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("Tool contracts ({} tools):", contracts.len()));
    lines.push(String::new());

    let mut ordered: Vec<&ToolContractProjection> = contracts.iter().collect();
    ordered.sort_by(|a, b| a.name.cmp(&b.name));

    for contract in ordered {
        lines.push(format!(
            "  {:<24} caller={:<16} effect={:<20} idempotent={}",
            contract.name, contract.caller_policy, contract.effect_class, contract.idempotency
        ));
    }

    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::DoctorReport,
        lines,
    );
}

#[cfg(test)]
mod tool_contract_projection_tests {
    use super::handle_tool_contracts;
    use crate::protocol::projection::dto::ToolContractProjection;
    use crate::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use crate::protocol::projection::snapshot::SessionProjectionSnapshot;
    use crate::tui::app::App;
    use crate::tui::components::dialogs::info::InfoDialog;

    fn published_contract(
        name: &str,
        caller_policy: &str,
        effect_class: &str,
        idempotency: &str,
    ) -> ToolContractProjection {
        ToolContractProjection {
            name: name.to_string(),
            caller_policy: caller_policy.to_string(),
            effect_class: effect_class.to_string(),
            idempotency: idempotency.to_string(),
            implementation_id: format!("builtin.{name}"),
        }
    }

    fn app_with_published_contracts(contracts: Vec<ToolContractProjection>) -> App {
        let mut app = App::new_for_testing("/tmp".into());
        let mut snapshot = SessionProjectionSnapshot::empty("sess", "proj", "ws");
        snapshot.set_tool_contracts(contracts);
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

    fn dialog_text(app: &mut App) -> String {
        app.focus_manager
            .dialog_mut_any::<InfoDialog>()
            .expect("the contracts opened an info dialog")
            .content_lines()
            .join("\n")
    }

    #[test]
    fn tool_contracts_render_the_published_contracts() {
        let mut app = app_with_published_contracts(vec![
            published_contract("glob", "program-only", "ReadOnly", "Idempotent"),
            published_contract("bash", "direct", "Mutating", "NonIdempotent"),
        ]);

        handle_tool_contracts(&mut app);

        let text = dialog_text(&mut app);
        assert!(
            text.contains("Tool contracts (2 tools):"),
            "the count is the published collection's: {text}"
        );
        let bash_row = text
            .lines()
            .find(|line| line.contains("bash"))
            .expect("a row for bash");
        assert!(
            bash_row.contains("caller=direct"),
            "the row carries it: {bash_row}"
        );
        let glob_row = text
            .lines()
            .find(|line| line.contains("glob"))
            .expect("a row for glob");
        assert!(
            glob_row.contains("caller=program-only"),
            "the row carries it: {glob_row}"
        );
    }

    #[test]
    fn tool_contracts_say_so_when_no_daemon_is_attached() {
        let mut app = App::new_for_testing("/tmp".into());

        handle_tool_contracts(&mut app);

        let text = app
            .messages_state
            .toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("daemon's tool contracts are unavailable"),
            "an unattached projection is named, not faked: {text}"
        );
    }
}
