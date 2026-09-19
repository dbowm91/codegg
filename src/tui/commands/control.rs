//! Shared-session controller commands (Team Collaboration Corrective M004).
//!
//! The TUI owns no control truth. Every operation performs an explicit
//! `session_control.v1` round-trip through the daemon-owned
//! `CoreClient` for the active session: get (lease plus inert
//! requests), request (inert), transfer (controller-only, CAS),
//! release (controller-only, CAS), and takeover (Maintainer/Owner,
//! bounded reason, audited). Results land back on the event loop as
//! `TuiCommand::Control*` completions guarded by the control request
//! id + active session + reconnect epoch, so stale completions are
//! dropped at apply time.
//!
//! Observer mode stays hard-blocked: `/control` is absent from the
//! observer allowlist, so the central `execute_command` gate rejects
//! every subcommand while observing (including read-only `get` —
//! observers already see controller identity through the session
//! projection). Permission/question answers stay blocked by
//! `ObserverState` exactly as before.
//!
//! Secrecy: control DTOs carry principal/turn/session ids, revisions,
//! and bounded reasons only — never credentials or device secrets.

use crate::protocol::core::{
    CoreRequest, CoreResponse, SessionControlRequestDto, SessionControllerDto,
};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

fn epoch_of(app: &App) -> u64 {
    app.presence.reconnect_epoch
}

fn unauthorized_of(code: &str) -> bool {
    matches!(
        code,
        "project_not_found" | "authorization_denied" | "authorization_scope_required"
    )
}

/// Entry point for `/control` and its subcommands. `args` is the raw
/// text after `/control` (possibly empty for the bare lease view).
pub(crate) fn dispatch_control_command(app: &mut App, args: &str) {
    let Some(session_id) = app.active_session_id().map(str::to_string) else {
        app.messages_state
            .toasts
            .info("No active session — open a session tab first");
        return;
    };
    let mut tokens = args.split_whitespace();
    let sub = tokens.next().unwrap_or("").to_ascii_lowercase();
    match sub.as_str() {
        "" | "get" | "show" | "status" => {
            start_control_get(app, session_id);
        }
        "request" => {
            let rest = args
                .split_once(' ')
                .map(|(_, rest)| rest.trim())
                .unwrap_or("");
            let message = if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            };
            start_control_request(app, session_id, message);
        }
        "transfer" => {
            let (recipient, revision, reason) = parse_transfer_args(args);
            let (Some(recipient), Some(revision)) = (recipient, revision) else {
                app.messages_state.toasts.warning(
                    "Usage: /control transfer <principal-id> <revision> [--reason <text>] (see /control for revisions)",
                );
                return;
            };
            start_control_transfer(app, session_id, recipient, revision, reason);
        }
        "release" => {
            let revision = tokens.next().unwrap_or("");
            if revision.is_empty() {
                app.messages_state
                    .toasts
                    .warning("Usage: /control release <revision> (see /control for the revision)");
                return;
            }
            match revision.parse::<u64>() {
                Ok(revision) => start_control_release(app, session_id, revision),
                Err(_) => {
                    app.messages_state
                        .toasts
                        .warning("Revision must be a non-negative integer");
                }
            }
        }
        "takeover" => {
            let (revision, reason) = parse_takeover_args(args);
            let Some(revision) = revision else {
                app.messages_state.toasts.warning(
                    "Usage: /control takeover <revision> <reason> (Maintainer/Owner only)",
                );
                return;
            };
            if reason.trim().is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /control takeover <revision> <reason> (Maintainer/Owner only)",
                );
                return;
            }
            start_control_takeover(app, session_id, revision, reason);
        }
        _ => {
            app.messages_state.toasts.warning(
                "Usage: /control, /control request [message], /control transfer <principal-id> <revision> [--reason <text>], /control release <revision>, /control takeover <revision> <reason>",
            );
        }
    }
}

/// Parse `/control transfer <principal> <revision> [--reason <text>]`.
fn parse_transfer_args(args: &str) -> (Option<String>, Option<u64>, Option<String>) {
    // Strip the leading `transfer` token, then split off `--reason`.
    let rest = args
        .split_once(' ')
        .map(|(_, rest)| rest.trim())
        .unwrap_or("");
    let (head, reason) = match rest.split_once("--reason") {
        Some((head, reason)) => (head.trim(), {
            let reason = reason.trim().trim_start_matches('=').trim();
            if reason.is_empty() {
                None
            } else {
                Some(reason.to_string())
            }
        }),
        None => (rest, None),
    };
    let mut parts = head.split_whitespace();
    let recipient = parts.next().map(str::to_string);
    let revision = parts.next().and_then(|raw| raw.parse::<u64>().ok());
    (recipient, revision, reason)
}

/// Parse `/control takeover <revision> <reason...>`.
fn parse_takeover_args(args: &str) -> (Option<u64>, String) {
    let rest = args
        .split_once(' ')
        .map(|(_, rest)| rest.trim())
        .unwrap_or("");
    let (revision_raw, reason) = match rest.split_once(' ') {
        Some((revision_raw, reason)) => (revision_raw, reason.trim().to_string()),
        None => (rest, String::new()),
    };
    (revision_raw.parse::<u64>().ok(), reason)
}

fn start_control_get(app: &mut App, session_id: String) {
    let request_id = app.dialog_state.control_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "control_get",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ControlLoaded {
                    request_id,
                    session_id,
                    controller: None,
                    requests: Vec::new(),
                    truncated: false,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("control-get-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionControlGet {
                    session_id: session_id.clone(),
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::SessionControl {
                    controller,
                    requests,
                    truncated,
                }) => Some(TuiCommand::ControlLoaded {
                    request_id,
                    session_id,
                    controller,
                    requests,
                    truncated,
                    error: None,
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::ControlLoaded {
                    request_id,
                    session_id,
                    controller: None,
                    requests: Vec::new(),
                    truncated: false,
                    error: Some(message),
                    unauthorized: unauthorized_of(&code),
                    unsupported: code == "unimplemented",
                    reconnect_epoch,
                }),
                Ok(other) => Some(TuiCommand::ControlLoaded {
                    request_id,
                    session_id,
                    controller: None,
                    requests: Vec::new(),
                    truncated: false,
                    error: Some(format!("unexpected response: {other:?}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ControlLoaded {
                    request_id,
                    session_id,
                    controller: None,
                    requests: Vec::new(),
                    truncated: false,
                    error: Some(e.to_string()),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

fn start_control_request(app: &mut App, session_id: String, message: Option<String>) {
    let request_id = app.dialog_state.control_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "control_request",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("control-request-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionControlRequest {
                    session_id: session_id.clone(),
                    message,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::SessionControl { .. }) => {
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: Some(
                            "Control request recorded (inert: no lease change)".to_string(),
                        ),
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: None,
                        error: Some(message),
                        unauthorized: unauthorized_of(&code),
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(format!("unexpected response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(e.to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

fn start_control_transfer(
    app: &mut App,
    session_id: String,
    recipient: String,
    revision: u64,
    reason: Option<String>,
) {
    let request_id = app.dialog_state.control_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "control_transfer",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("control-transfer-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionControlTransfer {
                    session_id: session_id.clone(),
                    recipient_principal: recipient.clone(),
                    expected_revision: revision,
                    reason,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::SessionControlUpdated { controller }) => {
                    let summary = match controller {
                        Some(dto) => format!(
                            "Control transferred to {} (revision {})",
                            dto.controller_principal, dto.revision
                        ),
                        None => "Control released".to_string(),
                    };
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: Some(summary),
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: None,
                        error: Some(message),
                        unauthorized: unauthorized_of(&code),
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(format!("unexpected response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(e.to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

fn start_control_release(app: &mut App, session_id: String, revision: u64) {
    let request_id = app.dialog_state.control_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "control_release",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("control-release-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionControlRelease {
                    session_id: session_id.clone(),
                    expected_revision: revision,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::SessionControlUpdated { .. }) => {
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: Some("Control released".to_string()),
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: None,
                        error: Some(message),
                        unauthorized: unauthorized_of(&code),
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(format!("unexpected response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(e.to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

fn start_control_takeover(app: &mut App, session_id: String, revision: u64, reason: String) {
    let request_id = app.dialog_state.control_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "control_takeover",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("control-takeover-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionControlTakeover {
                    session_id: session_id.clone(),
                    expected_revision: revision,
                    reason,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::SessionControlUpdated { controller }) => {
                    let summary = match controller {
                        Some(dto) => format!(
                            "Control taken over (revision {}, recovery audited)",
                            dto.revision
                        ),
                        None => "Control released".to_string(),
                    };
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: Some(summary),
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ControlMutationFinished {
                        request_id,
                        session_id: Some(session_id),
                        message: None,
                        error: Some(message),
                        unauthorized: unauthorized_of(&code),
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(format!("unexpected response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ControlMutationFinished {
                    request_id,
                    session_id: Some(session_id),
                    message: None,
                    error: Some(e.to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `ControlLoaded` completion. Success caches the lease for
/// the session indicator and opens the bounded lease view; stale
/// completions are dropped.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_control_loaded(
    app: &mut App,
    request_id: u64,
    session_id: String,
    controller: Option<SessionControllerDto>,
    requests: Vec<SessionControlRequestDto>,
    truncated: bool,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.control_request.finish(request_id) {
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.messages_state.toasts.warning(
                "Session control is unavailable for this session (not found or not authorized)",
            );
        } else if unsupported {
            app.messages_state.toasts.warning(
                "Session control requires a newer daemon (session_control.v1 unsupported)",
            );
        } else {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    app.dialog_state.control_last = Some((session_id.clone(), controller.clone()));
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Control,
        control_lines(&session_id, controller.as_ref(), &requests, truncated),
    );
}

/// Apply a `ControlMutationFinished` completion. Success refreshes the
/// lease view so the indicator and revision stay coherent.
pub(crate) fn apply_control_mutation(
    app: &mut App,
    request_id: u64,
    session_id: Option<String>,
    message: Option<String>,
    error: Option<String>,
    unauthorized: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.control_request.finish(request_id) {
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.messages_state.toasts.warning(
                "Session control is unavailable for this session (not found or not authorized)",
            );
        } else {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if let Some(message) = message {
        app.messages_state.toasts.info(&message);
    }
    // Refresh the cached lease so the indicator follows handoff.
    if let Some(session_id) = session_id {
        if app.active_session_id() == Some(session_id.as_str()) {
            start_control_get(app, session_id);
        }
    }
}

/// Pure line renderer for the lease view (ids/revisions/reasons only;
/// control DTOs never carry secrets).
fn control_lines(
    session_id: &str,
    controller: Option<&SessionControllerDto>,
    requests: &[SessionControlRequestDto],
    truncated: bool,
) -> Vec<String> {
    let mut lines = vec![
        format!("Session control — session {session_id}"),
        String::new(),
    ];
    match controller {
        Some(lease) => {
            lines.push(format!(
                "Controller: {} (turn {}, revision {})",
                lease.controller_principal, lease.turn_id, lease.revision
            ));
            lines.push(format!("Last action: {}", lease.last_action));
            if let Some(actor) = lease.last_actor.as_deref() {
                lines.push(format!("Last actor: {actor}"));
            }
            if let Some(reason) = lease.last_reason.as_deref() {
                lines.push(format!("Last reason: {reason}"));
            }
        }
        None => {
            lines.push("Controller: none (idle or released)".to_string());
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "Requests: {} shown{} (inert: never change the lease)",
        requests.len(),
        if truncated { " (truncated)" } else { "" }
    ));
    for request in requests {
        let message = request.message.as_deref().unwrap_or("");
        lines.push(format!(
            "  {} by {} {}",
            request.request_id, request.requester_principal, message
        ));
    }
    lines.push(String::new());
    lines.push("Request: /control request [message]".to_string());
    lines.push(
        "Transfer: /control transfer <principal-id> <revision> [--reason <text>]".to_string(),
    );
    lines.push("Release: /control release <revision>".to_string());
    lines.push(
        "Takeover: /control takeover <revision> <reason> (Maintainer/Owner only)".to_string(),
    );
    lines
}

/// Short controller indicator for the status bar (`None` when no
/// cached lease matches the active session).
pub(crate) fn control_indicator_for(app: &App, session_id: Option<&str>) -> Option<String> {
    let (cached_session, controller) = app.dialog_state.control_last.as_ref()?;
    let session_id = session_id?;
    if cached_session != session_id {
        return None;
    }
    let lease = controller.as_ref()?;
    Some(format!(
        "control:{}",
        short_principal(&lease.controller_principal)
    ))
}

fn short_principal(principal: &str) -> String {
    if principal.len() <= 18 {
        return principal.to_string();
    }
    let mut cut = 18;
    while cut > 0 && !principal.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &principal[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_args_parse_recipient_revision_and_reason() {
        let (recipient, revision, reason) =
            parse_transfer_args("transfer alice 3 --reason handoff for lunch");
        assert_eq!(recipient.as_deref(), Some("alice"));
        assert_eq!(revision, Some(3));
        assert_eq!(reason.as_deref(), Some("handoff for lunch"));
    }

    #[test]
    fn transfer_args_without_reason_parses() {
        let (recipient, revision, reason) = parse_transfer_args("transfer bob 1");
        assert_eq!(recipient.as_deref(), Some("bob"));
        assert_eq!(revision, Some(1));
        assert_eq!(reason, None);
    }

    #[test]
    fn takeover_args_require_revision_and_reason() {
        let (revision, reason) = parse_takeover_args("takeover 2 recovery after disconnect");
        assert_eq!(revision, Some(2));
        assert_eq!(reason, "recovery after disconnect");
    }

    #[test]
    fn control_lines_carry_no_secrets() {
        let lease = SessionControllerDto {
            session_id: "s1".to_string(),
            turn_id: "t1".to_string(),
            controller_principal: "alice".to_string(),
            origin_client: Some("c1".to_string()),
            revision: 2,
            created_at_ms: 1,
            updated_at_ms: 2,
            last_action: "transferred".to_string(),
            last_actor: Some("alice".to_string()),
            last_reason: Some("handoff".to_string()),
        };
        let lines = control_lines("s1", Some(&lease), &[], false);
        let joined = lines.join("\n");
        assert!(joined.contains("alice"));
        assert!(joined.contains("revision"));
        assert!(!joined.contains("cggt_"));
        assert!(!joined.contains("secret"));
    }
}
