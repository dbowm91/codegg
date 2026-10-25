//! M007-D: where a resolved command form goes.
//!
//! ADR-0052 (accepted 2026-10-24) gives the daemon the command catalog and
//! splits every command form into exactly two kinds:
//!
//! * a **daemon effect** — the daemon owns the effect and the authoritative
//!   data. The frontend asks the daemon to execute it through
//!   [`CoreRequest::CommandExecute`], and the daemon records the execution as
//!   a canonical `Run`.
//! * a **frontend effect** — the frontend owns it, and the daemon's declared
//!   frontend-action set names the action that serves it.
//!
//! This module is the TUI's side of that split. It answers one question —
//! *where does this form go?* — from the three sources of truth, and it
//! builds the one request a daemon-effect form travels through.
//!
//! The three sources are:
//!
//! * the **catalog** (`codegg_core::command_catalog`) — which forms exist and
//!   what kind each is;
//! * the **declared frontend-action set**
//!   (`codegg_core::frontend_action`) — which action serves a frontend-effect
//!   form;
//! * the **daemon's effect table** (`crate::core::daemon_commands`) — which
//!   daemon-effect forms the daemon can serve from a scope and an argument.
//!
//! A daemon-effect form the daemon cannot serve yet is
//! [`CommandRoute::NotServed`]: the TUI keeps its local arm, and the
//! classification artifact records the form. The daemon is never asked to
//! guess what a command means.

#[cfg(test)]
#[path = "command_route_tests.rs"]
mod command_route_tests;

use codegg_core::command_catalog::{find_by_command_id, CommandKind};
use codegg_core::frontend_action::{action_for_form, FrontendAction};
use codegg_protocol::core::{
    CommandCatalogScopeDto, CommandExecuteRequestDto, CoreRequest, CoreResponse,
};

/// The refusal's code and reason, recovered from a refused execution.
///
/// A migrated call site sees a refusal wrapped in the command execution's
/// result rather than as a bare error response, so the code and the
/// message have to be recovered from the summary to keep the text a user
/// reads the same as it was before the migration.
///
/// The summary a refusal carries is `"{code}: {reason}"`, and a code never
/// contains a colon, so the first colon is the boundary.
pub fn refusal_parts(result: &codegg_protocol::core::CommandExecuteResultDto) -> (String, String) {
    let line = result
        .lines
        .first()
        .cloned()
        .unwrap_or_else(|| result.summary.clone());
    match line.split_once(": ") {
        Some((code, reason)) => (code.to_string(), reason.to_string()),
        None => (String::new(), line),
    }
}

/// Resolve a command execution to the request the daemon would run.
///
/// M007-D: the daemon builds the inner request from the scope and the
/// argument, so a test double that answers a command execution answers the
/// **daemon's** request rather than a copy of it. A double that parses the
/// argument itself can drift from the daemon's row; this cannot.
#[cfg(test)]
pub(crate) fn inner_request_for_test(request: &CommandExecuteRequestDto) -> Option<CoreRequest> {
    let ctx = crate::core::daemon_commands::CommandContext {
        project_id: &request.scope.project_id,
        workspace_id: &request.scope.workspace_id,
        session_id: request.scope.session_id.as_deref(),
        argument: &request.argument,
    };
    match crate::core::daemon_commands::build_inner_request(
        &request.command_id,
        &request.form,
        &ctx,
    ) {
        Some(crate::core::daemon_commands::CommandEffect::Ready(inner)) => Some(*inner),
        _ => None,
    }
}

/// Wrap an inner response the way the daemon's executor does.
///
/// The daemon's executor returns the inner response verbatim, so a double
/// that wraps its answer this way is indistinguishable from the daemon.
#[cfg(test)]
pub(crate) fn command_execute_response_for_test(
    request: &CommandExecuteRequestDto,
    inner: CoreResponse,
) -> CoreResponse {
    CoreResponse::CommandExecute {
        result: codegg_protocol::core::CommandExecuteResultDto {
            run_id: "run-1".to_string(),
            command_id: request.command_id.clone(),
            status: codegg_protocol::core::CommandOutcomeStatusDto::Completed,
            summary: "ok".to_string(),
            lines: Vec::new(),
            report: None,
            started_at_ms: 0,
            completed_at_ms: 1,
            response: serde_json::to_value(&inner).ok(),
        },
    }
}

/// Where one command form goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRoute {
    /// The daemon owns the effect and serves it.
    DaemonEffect {
        /// The command's stable identity.
        command_id: &'static str,
        /// The argument form.
        form: &'static str,
    },
    /// The frontend owns the effect; the declared action serves it.
    FrontendEffect {
        /// The declared action.
        action: FrontendAction,
    },
    /// Nobody serves the effect yet.
    ///
    /// The TUI keeps its local arm for this form. The classification
    /// artifact records it, so the gap is visible rather than implicit.
    NotServed,
}

/// The route for one catalog form.
///
/// The decision is made from the catalog's own classification, so a frontend
/// and the daemon can never disagree about what a form is.
pub fn route_for(command_id: &str, form: &str) -> CommandRoute {
    let Some(command) = find_by_command_id(command_id) else {
        return CommandRoute::NotServed;
    };
    let Some(catalog_form) = command.forms.iter().find(|f| f.form == form) else {
        return CommandRoute::NotServed;
    };
    match catalog_form.kind {
        CommandKind::FrontendEffect => match action_for_form(command_id, form) {
            Some(action) => CommandRoute::FrontendEffect { action },
            // A frontend-effect form with no declared action is a defect the
            // frontend-action guard rejects; the route falls back to the
            // local arm rather than to a daemon request.
            None => CommandRoute::NotServed,
        },
        CommandKind::DaemonEffect => {
            if crate::core::daemon_commands::serves_form(command_id, form) {
                CommandRoute::DaemonEffect {
                    command_id: command.command_id,
                    form: catalog_form.form,
                }
            } else {
                CommandRoute::NotServed
            }
        }
    }
}

/// The scope a command acts in.
///
/// The TUI learns the project and the workspace from the attached session's
/// canonical binding. A command the user runs with no session attached has
/// no project or workspace to name, and the scope says so with empty
/// identities: the daemon then refuses the command with a typed reason
/// rather than acting on a project the caller did not name.
///
/// The session is the caller's selection, so it travels in the scope rather
/// than being guessed by the daemon.
pub fn scope_for(app: &crate::tui::app::App) -> CommandCatalogScopeDto {
    let session = app.session_state.session.as_ref();
    CommandCatalogScopeDto {
        project_id: session
            .map(|session| session.project_id.clone())
            .unwrap_or_default(),
        workspace_id: session
            .and_then(|session| session.workspace_id.clone())
            .unwrap_or_default(),
        session_id: session.map(|session| session.id.clone()),
    }
}

/// The request that executes a daemon-effect command.
///
/// The daemon builds the inner request from the scope and the argument, so
/// the frontend never supplies one: `command.execute` is the authority to
/// *run* a command, and the daemon re-authorizes the inner request on its
/// own operation.
pub fn command_execute(
    scope: CommandCatalogScopeDto,
    command_id: &str,
    form: &str,
    argument: &str,
) -> CoreRequest {
    CoreRequest::CommandExecute {
        request: CommandExecuteRequestDto {
            scope,
            command_id: command_id.to_string(),
            form: form.to_string(),
            argument: argument.to_string(),
            catalog_version: codegg_core::command_catalog::catalog_version(),
        },
    }
}

/// Send a command to the daemon and unwrap its inner response.
///
/// This is the whole of a migrated command's effect call: the daemon
/// executes the command's effect, and the caller receives the same response
/// it received before the migration, so the presentation code around it does
/// not change.
pub async fn send_command(
    core_client: &std::sync::Arc<dyn crate::core::CoreClient>,
    scope: CommandCatalogScopeDto,
    command_id: &str,
    form: &str,
    argument: &str,
) -> Result<CoreResponse, crate::error::AppError> {
    let request = crate::core::new_request(
        format!("command-{command_id}-{}", uuid::Uuid::new_v4()),
        command_execute(scope, command_id, form, argument),
    );
    let response = core_client.request(request).await?;
    Ok(unwrap_inner_response(response))
}

/// Unwrap a command execution's inner response.
///
/// The daemon's executor returns the inner response verbatim, so a call site
/// that already matches on that response keeps matching on it: the
/// observable outcome of a migrated command does not change.
///
/// A response that carries no inner response — a refusal, or an effect that
/// produced nothing — is returned unchanged.
pub fn unwrap_inner_response(response: CoreResponse) -> CoreResponse {
    let CoreResponse::CommandExecute { result } = &response else {
        return response;
    };
    let Some(inner) = result.response.as_ref() else {
        return response;
    };
    match serde_json::from_value::<CoreResponse>(inner.clone()) {
        Ok(inner) => inner,
        Err(_) => response,
    }
}

/// Unwrap a command execution's inner response, keeping the error.
///
/// The counterpart of [`unwrap_inner_response`] for a call site that matches
/// on the response *and* on the transport error.
pub fn unwrap_inner_result(
    result: Result<CoreResponse, crate::error::AppError>,
) -> Result<CoreResponse, crate::error::AppError> {
    Ok(unwrap_inner_response(result?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_effect_the_daemon_serves_routes_to_the_daemon() {
        assert_eq!(
            route_for("reload", ""),
            CommandRoute::DaemonEffect {
                command_id: "reload",
                form: ""
            }
        );
    }

    #[test]
    fn a_frontend_effect_routes_to_its_declared_action() {
        assert_eq!(
            route_for("exit", ""),
            CommandRoute::FrontendEffect {
                action: FrontendAction::Quit
            }
        );
    }

    #[test]
    fn a_daemon_effect_the_daemon_does_not_serve_is_not_served() {
        // The LSP workflow family has no daemon request: the recipe runs in
        // the frontend's LSP tool.
        assert_eq!(route_for("lsp_doctor", ""), CommandRoute::NotServed);
        // The plugin family has no daemon request surface.
        assert_eq!(route_for("plugin_install", ""), CommandRoute::NotServed);
    }

    #[test]
    fn an_unknown_command_or_form_is_not_served() {
        assert_eq!(route_for("not_a_command", ""), CommandRoute::NotServed);
        assert_eq!(route_for("reload", "not_a_form"), CommandRoute::NotServed);
    }

    #[test]
    fn every_frontend_effect_form_routes_to_a_declared_action() {
        let catalog = codegg_core::command_catalog::the_catalog();
        for command in catalog.commands {
            for form in command.forms {
                if form.kind != CommandKind::FrontendEffect {
                    continue;
                }
                assert!(
                    matches!(
                        route_for(command.command_id, form.form),
                        CommandRoute::FrontendEffect { .. }
                    ),
                    "{} must route to a declared action",
                    command.command_id
                );
            }
        }
    }

    #[test]
    fn the_command_execute_request_carries_the_scope_and_the_argument() {
        let request = command_execute(
            CommandCatalogScopeDto {
                project_id: "project".to_string(),
                workspace_id: "workspace".to_string(),
                session_id: Some("session".to_string()),
            },
            "memory_search",
            "",
            "needle",
        );
        match request {
            CoreRequest::CommandExecute { request } => {
                assert_eq!(request.command_id, "memory_search");
                assert_eq!(request.argument, "needle");
                assert_eq!(request.scope.session_id.as_deref(), Some("session"));
                assert_eq!(
                    request.catalog_version,
                    codegg_core::command_catalog::catalog_version()
                );
            }
            other => panic!("expected a command execution, got {other:?}"),
        }
    }

    #[test]
    fn an_inner_response_is_unwrapped_and_a_refusal_is_not() {
        let inner = CoreResponse::Error {
            code: "inner".to_string(),
            message: "the inner response".to_string(),
        };
        let wrapped = CoreResponse::CommandExecute {
            result: codegg_protocol::core::CommandExecuteResultDto {
                run_id: "run".to_string(),
                command_id: "policy".to_string(),
                status: codegg_protocol::core::CommandOutcomeStatusDto::Completed,
                summary: "ok".to_string(),
                lines: Vec::new(),
                report: None,
                started_at_ms: 0,
                completed_at_ms: 1,
                response: serde_json::to_value(&inner).ok(),
            },
        };
        let unwrapped = unwrap_inner_response(wrapped);
        assert_eq!(
            serde_json::to_value(&unwrapped).ok(),
            serde_json::to_value(&inner).ok(),
            "the inner response must come back verbatim"
        );

        // A refusal carries no inner response and is returned unchanged.
        let refusal = CoreResponse::Error {
            code: "unknown_command".to_string(),
            message: "no such command".to_string(),
        };
        let unchanged = unwrap_inner_response(refusal.clone());
        assert_eq!(
            serde_json::to_value(&unchanged).ok(),
            serde_json::to_value(&refusal).ok(),
            "a refusal must come back unchanged"
        );
    }
}
