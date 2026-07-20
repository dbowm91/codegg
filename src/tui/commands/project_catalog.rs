//! Async project-catalog commands (Multi-Project TUI milestone 1).
//!
//! These commands drive the bounded `ProjectCatalogState` cache. The
//! state is **frontend-local**; nothing here mutates daemon storage.
//! Capability negotiation is explicit: when the daemon does not
//! advertise `project_catalog.v1`, the catalog stays empty and the
//! compatibility tab continues to function.
//!
//! Concurrency: every `start_*` operation bumps a request generation
//! via `AsyncUiRequestState::begin` and the matching `apply_*` call
//! drops stale completions. New commands MUST follow the
//! `start_*/apply_*` pair pattern.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::dto::ProjectSummaryDto;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Kick off a `ProjectCatalogCapabilities` round-trip followed by a
/// bounded `ProjectList` load. The completion populates
/// `App.project_catalog`. Stale completions are dropped on apply.
pub(crate) fn start_refresh_project_catalog(app: &mut App) {
    if app.core_client.is_none() {
        // No daemon available. Mark capability as unsupported so the
        // TUI surfaces a clear diagnostic and the compat tab stays
        // usable.
        app.project_catalog.set_capability(false);
        return;
    }
    let request_id = app.project_catalog.list_request.begin();

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "project_catalog_refresh",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ProjectCatalogRefreshed {
                    request_id,
                    supported: false,
                    entries: Vec::new(),
                    truncated: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            // 1. Capability negotiation. Daemon defaults the flag to
            //    `false` for older builds, so this is the only way to
            //    distinguish "unsupported" from "no projects yet".
            let cap_request = crate::core::new_request(
                format!("project-catalog-capabilities-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectCatalogCapabilities,
            );
            let supported = match core_client.request(cap_request).await {
                Ok(CoreResponse::ProjectCatalogCapabilities { supported, .. }) => supported,
                Ok(CoreResponse::Error { .. }) => false,
                Ok(_) => false,
                Err(_) => false,
            };

            if !supported {
                return Some(TuiCommand::ProjectCatalogRefreshed {
                    request_id,
                    supported: false,
                    entries: Vec::new(),
                    truncated: false,
                    error: None,
                });
            }

            // 2. Bounded list. The daemon clamps the response to
            //    `max_list_items`; we pass the same default here.
            let list_request = crate::core::new_request(
                format!("project-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectList {
                    include_archived: false,
                    limit: 128,
                },
            );
            match core_client.request(list_request).await {
                Ok(CoreResponse::ProjectList {
                    projects,
                    truncated,
                }) => Some(TuiCommand::ProjectCatalogRefreshed {
                    request_id,
                    supported: true,
                    entries: projects,
                    truncated,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ProjectCatalogRefreshed {
                        request_id,
                        supported: true,
                        entries: Vec::new(),
                        truncated: false,
                        error: Some(format!("Project list failed ({}): {}", code, message)),
                    })
                }
                Ok(other) => Some(TuiCommand::ProjectCatalogRefreshed {
                    request_id,
                    supported: true,
                    entries: Vec::new(),
                    truncated: false,
                    error: Some(format!("Unexpected core response: {:?}", other)),
                }),
                Err(e) => Some(TuiCommand::ProjectCatalogRefreshed {
                    request_id,
                    supported: true,
                    entries: Vec::new(),
                    truncated: false,
                    error: Some(format!("Project list request failed: {}", e)),
                }),
            }
        },
    );
}

/// Apply the completion of a `ProjectCatalogRefreshed` async
/// operation. Drops stale completions by comparing the request id.
pub(crate) fn apply_project_catalog_refreshed(
    app: &mut App,
    request_id: u64,
    supported: bool,
    entries: Vec<ProjectSummaryDto>,
    truncated: bool,
    error: Option<String>,
) {
    app.project_catalog.set_capability(supported);
    if supported {
        if let Some(err) = error {
            let _ = app.project_catalog.apply_list_error(request_id, err);
        } else {
            let _ = app
                .project_catalog
                .apply_list(request_id, entries, truncated);
        }
    } else if let Some(err) = error {
        // Unsupported + errored round-trip. Drop the request state but
        // surface the diagnostic.
        let _ = app.project_catalog.apply_list_error(request_id, err);
    } else {
        // Unsupported but successful round-trip. Clear the loading
        // flag without recording an error.
        let _ = app.project_catalog.list_request.finish(request_id);
    }
}
