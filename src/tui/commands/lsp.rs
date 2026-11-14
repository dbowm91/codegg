//! The LSP command surface: previews, repair/review workflows, server control and the semantic cache.

use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use egglsp::render_workflow_display;
use egglsp::LspWorkflowInvocation;
use egglsp::LspWorkflowRecipe;

pub(crate) fn dispatch_lsp_previews(app: &mut App) {
    app.ui_state.command_mode = false;
    if let Some(ref lsp_tool) = app.lsp_tool {
        let output = lsp_tool.preview_list_text();
        app.messages_state.toasts.info(&output);
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_preview(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let id = argument;
    if id.is_empty() {
        app.messages_state.toasts.info("Usage: /lsp-preview <id>");
    } else if app.core_client.is_some() {
        app.messages_state
            .toasts
            .info("LSP preview details are not available through this daemon yet");
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        match lsp_tool.preview_detail_text(id) {
            Some(detail) => app.messages_state.toasts.info(&detail),
            None => app
                .messages_state
                .toasts
                .info(&format!("Preview not found: {id}")),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_preview_clear(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if let Some(ref lsp_tool) = app.lsp_tool {
        if arg == "--all" || arg.is_empty() {
            let count = lsp_tool.clear_preview(None);
            app.messages_state
                .toasts
                .info(&format!("Cleared {count} preview(s)"));
        } else {
            let count = lsp_tool.clear_preview(Some(arg));
            if count > 0 {
                app.messages_state
                    .toasts
                    .info(&format!("Cleared preview: {arg}"));
            } else {
                app.messages_state
                    .toasts
                    .info(&format!("Preview not found: {arg}"));
            }
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_preview_refresh(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let id = argument;
    if id.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-preview-refresh <id>");
    } else if app.core_client.is_some() {
        app.messages_state
            .toasts
            .info("LSP preview refresh is not available through this daemon yet");
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        match lsp_tool.refresh_preview_staleness(id) {
            Some((true, detail)) => {
                app.messages_state
                    .toasts
                    .info(&format!("Preview is STALE\n{detail}"));
            }
            Some((false, _)) => {
                app.messages_state.toasts.info("Preview is FRESH");
            }
            None => {
                app.messages_state
                    .toasts
                    .info(&format!("Preview not found: {id}"));
            }
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_servers(app: &mut App) {
    app.ui_state.command_mode = false;
    app.enqueue_tui_command(TuiCommand::LspServersRequested);
}

pub(crate) fn dispatch_lsp_capabilities(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let key = argument;
    if key.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-capabilities <server-key>\nUse /lsp-servers to see available keys.");
    } else {
        app.enqueue_tui_command(TuiCommand::LspCapabilitiesRequested {
            server_id: key.to_string(),
        });
    }
}

pub(crate) fn dispatch_lsp_errors(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let key = argument;
    if key.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-errors <server-key>\nUse /lsp-servers to see available keys.");
    } else {
        app.enqueue_tui_command(TuiCommand::LspErrorsRequested {
            server_id: key.to_string(),
        });
    }
}

pub(crate) fn dispatch_lsp_root(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let path_arg = argument;
    if path_arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-root <file-path>");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_root",
            argument: path_arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let text = lsp_tool.lsp_root_diagnose(path_arg);
        app.messages_state.toasts.info(&text);
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_doctor(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let path_arg = argument;
    if path_arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-doctor <file-path>");
    } else if app.core_client.is_some() {
        app.messages_state
            .toasts
            .info("LSP doctor is not available through this daemon yet");
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let text = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.lsp_doctor(path_arg))
        });
        app.messages_state.toasts.info(&text);
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_context_diagnostics(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let path_arg = argument;
    if path_arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-context-diagnostics <file-path>");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_context_diagnostics",
            argument: path_arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let text = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.lsp_context_diagnostics(path_arg))
        });
        app.messages_state.toasts.info(&text);
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_repair_local(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-repair-local <path[:line]>");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_repair_local",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let (path, line, col) = parse_path_line_col(arg);
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::RepairLocal,
            primary_path: Some(path),
            line,
            column: col,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_repair_hunk(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-repair-hunk <path> [hunk-id|range]");
    } else if app.core_client.is_some() {
        app.messages_state
            .toasts
            .info("LSP hunk repair is not available through this daemon yet");
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let parts: Vec<&str> = arg.splitn(2, ' ').collect();
        let path = parts[0].to_string();
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::RepairHunk,
            primary_path: Some(path),
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_review_file(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-review-file <path>");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_review_file",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::ReviewFile,
            primary_path: Some(arg.to_string()),
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_review_diff(app: &mut App) {
    app.ui_state.command_mode = false;
    if app.core_client.is_some() {
        app.messages_state
            .toasts
            .info("LSP diff review is not available through this daemon yet");
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::ReviewDiff,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_security_review(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if app.core_client.is_some() {
        app.messages_state
            .toasts
            .info("LSP security review is not available through this daemon yet");
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let review_mode = if arg.is_empty() {
            Some("diff".to_string())
        } else {
            Some(arg.to_string())
        };
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::SecurityReviewEnriched,
            review_mode,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_impact(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-impact <path:line:col>");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_impact",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let (path, line, col) = parse_path_line_col(arg);
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::ImpactAnalysis,
            primary_path: Some(path),
            line,
            column: col,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_test_repair(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-test-repair <test-file> [failure-text]");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_test_repair",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let parts: Vec<&str> = arg.splitn(2, ' ').collect();
        let test_file = parts[0].to_string();
        let failure_text = parts.get(1).map(|s| s.to_string());
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::TestFailureRepair,
            primary_path: Some(test_file),
            failure_text,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_interface(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-interface <path[:symbol]>");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_interface",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let parts: Vec<&str> = arg.splitn(2, ':').collect();
        let path = parts[0].to_string();
        let symbol = parts.get(1).map(|s| s.to_string());
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::InterfaceBoundary,
            primary_path: Some(path),
            symbol,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_cross_repair(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-cross-repair <primary> [related...]");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_cross_repair",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let parts: Vec<&str> = arg.split_whitespace().collect();
        let primary = parts[0].to_string();
        let related_files: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::CrossFileRepair,
            primary_path: Some(primary),
            related_files,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_call_neighbors(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if arg.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-call-neighbors <path:line:col> [incoming|outgoing|both]");
    } else if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspWorkflowRequested {
            command_id: "lsp_call_neighbors",
            argument: arg.to_string(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let parts: Vec<&str> = arg.splitn(2, ' ').collect();
        let (path, line, col) = parse_path_line_col(parts[0]);
        let direction = match parts.get(1).copied() {
            Some("incoming") => Some(egglsp::context::HierarchyDirection::Incoming),
            Some("outgoing") => Some(egglsp::context::HierarchyDirection::Outgoing),
            Some("both") => Some(egglsp::context::HierarchyDirection::Both),
            _ => None,
        };
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::CallNeighborhood,
            primary_path: Some(path),
            line,
            column: col,
            direction,
            ..Default::default()
        };
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.run_lsp_workflow(&invocation))
        }) {
            Ok(display) => {
                let text = render_workflow_display(&display);
                app.messages_state.toasts.info(&text);
            }
            Err(e) => app.messages_state.toasts.info(&e),
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_restart(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let key = argument;
    if key.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /lsp-restart <server-key>\nUse /lsp-servers to see available keys.");
    } else {
        app.enqueue_tui_command(TuiCommand::LspRestartRequested {
            server_id: key.to_string(),
        });
    }
}

pub(crate) fn dispatch_lsp_stop(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if app.core_client.is_some() {
        if arg.trim().is_empty() || arg.trim() == "--all" {
            app.messages_state
                .toasts
                .info("Usage: /lsp-stop <server-key> (select one server from /lsp-servers)");
        } else {
            app.enqueue_tui_command(TuiCommand::LspControlRequested {
                command_id: "lsp_stop",
                argument: arg.trim().to_string(),
            });
        }
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let key_opt = if arg.is_empty() || arg == "--all" {
            None
        } else {
            Some(arg)
        };
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(lsp_tool.lsp_stop_server(key_opt))
        });
        app.messages_state.toasts.info(&result);
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_cache_status(app: &mut App) {
    app.ui_state.command_mode = false;
    if app.core_client.is_some() {
        app.enqueue_tui_command(TuiCommand::LspControlRequested {
            command_id: "lsp_cache_status",
            argument: String::new(),
        });
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        let text = lsp_tool.lsp_cache_status();
        app.messages_state.toasts.info(&text);
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

pub(crate) fn dispatch_lsp_cache_clear(app: &mut App, argument: &str) {
    app.ui_state.command_mode = false;
    let arg = argument;
    if app.core_client.is_some() {
        if !arg.trim().is_empty() && arg.trim() != "--all" {
            app.messages_state.toasts.info(
                "Cache clear is scoped to the active session workspace; no root argument is accepted",
            );
        } else {
            app.enqueue_tui_command(TuiCommand::LspControlRequested {
                command_id: "lsp_cache_clear",
                argument: String::new(),
            });
        }
    } else if let Some(ref lsp_tool) = app.lsp_tool {
        if arg.is_empty() || arg == "--all" {
            let count = lsp_tool.clear_semantic_cache();
            app.messages_state
                .toasts
                .info(&format!("Cleared {count} cached entries"));
        } else {
            let root = std::path::PathBuf::from(arg);
            let count = lsp_tool.clear_semantic_cache_for_root(&root);
            app.messages_state.toasts.info(&format!(
                "Cleared {count} cached entries for root: {}",
                root.display()
            ));
        }
    } else {
        app.messages_state.toasts.info("LSP not available");
    }
}

/// Split `<path>[:line[:col]]` from an LSP command argument.
fn parse_path_line_col(s: &str) -> (String, Option<u32>, Option<u32>) {
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    let path = parts[0].to_string();
    let line = parts.get(1).and_then(|l| l.parse::<u32>().ok());
    let col = parts.get(2).and_then(|c| c.parse::<u32>().ok());
    (path, line, col)
}
