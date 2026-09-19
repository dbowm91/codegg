//! Project team administration commands (Team Collaboration Corrective M003).
//!
//! The TUI owns no team truth. Every operation performs an explicit
//! `team.v1` round-trip through the daemon-owned `CoreClient`:
//! capability negotiation, then membership list/add/update/revoke,
//! principal list/create/status, token list/create/revoke, and the M002
//! chat-policy get/set operations for the same project. Results land back
//! on the event loop as `TuiCommand::Team*` completions guarded by the
//! team request id + active project + reconnect epoch, so stale
//! completions (after a tab switch, a newer request, or a reconnect) are
//! dropped at apply time.
//!
//! `/collaborators` remains the ephemeral presence/observe chooser; this
//! module owns the durable administration surface (`/team`).
//!
//! Secrecy: the one-time `cggt_...` plaintext is rendered exactly once in
//! the secret-safe [`DeviceSecretDialog`](crate::tui::components::dialogs::device_secret::DeviceSecretDialog)
//! and never enters prompt/transcript/notification/audit/event/chat
//! state. Closing the dialog drops the credential; token metadata stays
//! listable but the plaintext is never re-readable.

use crate::protocol::core::{
    ChatChannelModeDto, ChatPolicyDecisionDto, ChatProjectPolicyDto, CoreRequest, CoreResponse,
    TeamMembershipDto, TeamPrincipalDto, TeamTokenDto,
};
use crate::tui::app::state::{OneTimeBearer, OneTimeDeviceToken};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

fn unauthorized_of(code: &str) -> bool {
    // Single-project denials use the privacy-safe not-found shape;
    // LocalOwner-only denials carry the typed scope denial. Both render
    // as the generic unavailable/admin-only panel, never as an oracle.
    matches!(
        code,
        "project_not_found" | "authorization_denied" | "authorization_scope_required"
    )
}

fn epoch_of(app: &App) -> u64 {
    app.presence.reconnect_epoch
}

/// Entry point for `/team` and its subcommands. `args` is the raw text
/// after `/team` (possibly empty for the bare administration view).
pub(crate) fn dispatch_team_command(app: &mut App, args: &str) {
    let Some(project_id) = app.active_project_id().map(str::to_string) else {
        app.messages_state
            .toasts
            .info("No active project — open a project tab first");
        return;
    };
    let mut tokens = args.split_whitespace();
    let sub = tokens.next().unwrap_or("").to_ascii_lowercase();
    match sub.as_str() {
        "" | "members" | "list" | "show" => {
            start_show_team(app, project_id);
        }
        "add" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let role = tokens.next().unwrap_or("").to_string();
            if principal.is_empty() || role.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team add <principal-id> <viewer|contributor|maintainer|owner>",
                );
                return;
            }
            start_membership_add(app, project_id, principal, role);
        }
        "role" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let role = tokens.next().unwrap_or("").to_string();
            let revision = tokens.next().unwrap_or("");
            if principal.is_empty() || role.is_empty() || revision.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team role <principal-id> <role> <revision> (see /team for revisions)",
                );
                return;
            }
            match revision.parse::<u64>() {
                Ok(revision) => {
                    start_membership_update(app, project_id, principal, Some(role), None, revision);
                }
                Err(_) => {
                    app.messages_state
                        .toasts
                        .warning("Revision must be a non-negative integer");
                }
            }
        }
        "suspend" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let revision = tokens.next().unwrap_or("");
            if principal.is_empty() || revision.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team suspend <principal-id> <revision> (see /team for revisions)",
                );
                return;
            }
            match revision.parse::<u64>() {
                Ok(revision) => start_membership_update(
                    app,
                    project_id,
                    principal,
                    None,
                    Some("suspended".to_string()),
                    revision,
                ),
                Err(_) => {
                    app.messages_state
                        .toasts
                        .warning("Revision must be a non-negative integer");
                }
            }
        }
        "reactivate" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let revision = tokens.next().unwrap_or("");
            if principal.is_empty() || revision.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team reactivate <principal-id> <revision> (see /team for revisions)",
                );
                return;
            }
            match revision.parse::<u64>() {
                Ok(revision) => start_membership_update(
                    app,
                    project_id,
                    principal,
                    None,
                    Some("active".to_string()),
                    revision,
                ),
                Err(_) => {
                    app.messages_state
                        .toasts
                        .warning("Revision must be a non-negative integer");
                }
            }
        }
        "revoke" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let revision = tokens.next().unwrap_or("");
            if principal.is_empty() || revision.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team revoke <principal-id> <revision> (see /team for revisions; revocation is monotonic)",
                );
                return;
            }
            match revision.parse::<u64>() {
                Ok(revision) => start_membership_revoke(app, project_id, principal, revision),
                Err(_) => {
                    app.messages_state
                        .toasts
                        .warning("Revision must be a non-negative integer");
                }
            }
        }
        "principals" => {
            start_principals_list(app);
        }
        "principal-create" => {
            let rest: Vec<&str> = tokens.collect();
            if rest.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team principal-create <display-name> [--service] (LocalOwner only)",
                );
                return;
            }
            let service = rest.contains(&"--service");
            let name = rest
                .iter()
                .filter(|t| **t != "--service")
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            if name.trim().is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team principal-create <display-name> [--service] (LocalOwner only)",
                );
                return;
            }
            start_principal_create(app, name, service);
        }
        "principal-disable" | "principal-enable" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let revision = tokens.next().unwrap_or("");
            if principal.is_empty() || revision.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team principal-disable|principal-enable <principal-id> <revision> (LocalOwner only)",
                );
                return;
            }
            match revision.parse::<u64>() {
                Ok(revision) => {
                    let status = if sub == "principal-disable" {
                        "disabled"
                    } else {
                        "active"
                    };
                    start_principal_status(app, principal, status.to_string(), revision);
                }
                Err(_) => {
                    app.messages_state
                        .toasts
                        .warning("Revision must be a non-negative integer");
                }
            }
        }
        "tokens" => {
            let principal = tokens.next().unwrap_or("").to_string();
            if principal.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team tokens <principal-id> (LocalOwner only; metadata only, never secrets)",
                );
                return;
            }
            start_tokens_list(app, principal);
        }
        "token-create" => {
            let principal = tokens.next().unwrap_or("").to_string();
            let label = tokens.next().unwrap_or("").to_string();
            if principal.is_empty() || label.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team token-create <principal-id> <label> (LocalOwner only; credential shown once)",
                );
                return;
            }
            start_token_create(app, project_id, principal, label);
        }
        "token-revoke" => {
            let token_id = tokens.next().unwrap_or("").to_string();
            if token_id.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team token-revoke <token-id> (LocalOwner only; revocation is monotonic)",
                );
                return;
            }
            start_token_revoke(app, token_id);
        }
        "chat" => {
            start_chat_policy_view(app, project_id);
        }
        "chat-allow" | "chat-deny" | "chat-clear" => {
            let principal = tokens.next().unwrap_or("").to_string();
            if principal.is_empty() {
                app.messages_state.toasts.warning(
                    "Usage: /team chat-allow|chat-deny|chat-clear <principal-id> [--channel <channel-id>]",
                );
                return;
            }
            let rest: Vec<&str> = tokens.collect();
            let channel = channel_flag(&rest);
            let decision = match sub.as_str() {
                "chat-allow" => Some(ChatPolicyDecisionDto::Allow),
                "chat-deny" => Some(ChatPolicyDecisionDto::Deny),
                _ => None,
            };
            start_chat_override(app, project_id, principal, channel, decision);
        }
        "chat-restrict" | "chat-inherit" => {
            let channel = tokens.next().unwrap_or("").to_string();
            if channel.is_empty() {
                app.messages_state
                    .toasts
                    .warning("Usage: /team chat-restrict|chat-inherit <channel-id>");
                return;
            }
            let mode = if sub == "chat-restrict" {
                ChatChannelModeDto::Restricted
            } else {
                ChatChannelModeDto::InheritProject
            };
            start_channel_mode(app, project_id, channel, mode);
        }
        _ => {
            app.messages_state.toasts.warning(
                "Usage: /team [members|add|role|suspend|reactivate|revoke|principals|principal-create|tokens|token-create|token-revoke|chat|chat-allow|chat-deny|chat-clear|chat-restrict|chat-inherit]",
            );
        }
    }
}

fn channel_flag(rest: &[&str]) -> Option<String> {
    let mut iter = rest.iter().peekable();
    while let Some(token) = iter.next() {
        if *token == "--channel" {
            if let Some(id) = iter.next() {
                return Some((*id).to_string());
            }
            return None;
        }
    }
    None
}

// ── Membership view (bare /team) ───────────────────────────────────────────

/// Fetch the bounded member list plus the M002 chat-policy summary for
/// the active project, then render the FocusManager-backed `/team`
/// administration dialog. `/collaborators` stays the presence surface.
fn start_show_team(app: &mut App, project_id: String) {
    if app.core_client.is_none() {
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::Team,
            vec![
                "Team administration unavailable — no daemon connection.".to_string(),
                "Check daemon status with /doctor.".to_string(),
            ],
        );
        return;
    }
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_membership_list",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TeamMembershipLoaded {
                    request_id,
                    project_id,
                    memberships: None,
                    policy: None,
                    truncated: false,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            };
            if !negotiate_team(&core_client).await {
                return Some(TuiCommand::TeamMembershipLoaded {
                    request_id,
                    project_id,
                    memberships: None,
                    policy: None,
                    truncated: false,
                    error: None,
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            }
            let list_request = crate::core::new_request(
                format!("team-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamMembershipList {
                    project_id: project_id.clone(),
                    limit: None,
                },
            );
            let (memberships, truncated, list_error, unauthorized) =
                match core_client.request(list_request).await {
                    Ok(CoreResponse::TeamMembershipList {
                        memberships,
                        truncated,
                    }) => (Some(memberships), truncated, None, false),
                    Ok(CoreResponse::Error { code, message }) => (
                        None,
                        false,
                        Some(format!("{code}: {message}")),
                        unauthorized_of(&code),
                    ),
                    Ok(other) => (
                        None,
                        false,
                        Some(format!("Unexpected core response: {other:?}")),
                        false,
                    ),
                    Err(e) => (None, false, Some(format!("Team list failed: {e}")), false),
                };
            // M002 chat-policy summary rides along when the caller holds
            // `member.manage`; denials render as the generic unavailable
            // line rather than a second error panel.
            let policy_request = crate::core::new_request(
                format!("team-chat-policy-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatPolicyGet {
                    project_id: project_id.clone(),
                    channel_id: None,
                },
            );
            let policy = match core_client.request(policy_request).await {
                Ok(CoreResponse::ChatPolicy { project, .. }) => Some(project),
                Ok(_) | Err(_) => None,
            };
            Some(TuiCommand::TeamMembershipLoaded {
                request_id,
                project_id,
                memberships,
                policy,
                truncated,
                error: list_error,
                unauthorized,
                unsupported: false,
                reconnect_epoch,
            })
        },
    );
}

/// Capability negotiation shared by every team task. Returns `true`
/// when the daemon advertises `team.v1`.
async fn negotiate_team(core_client: &std::sync::Arc<dyn crate::core::CoreClient>) -> bool {
    let cap_request = crate::core::new_request(
        format!("team-capabilities-{}", uuid::Uuid::new_v4()),
        CoreRequest::TeamCapabilities,
    );
    match core_client.request(cap_request).await {
        Ok(CoreResponse::TeamCapabilities { capabilities }) => capabilities.supported,
        _ => false,
    }
}

/// Apply a `TeamMembershipLoaded` completion. Drops stale completions
/// (superseded request, tab switch, reconnect) and renders the
/// FocusManager-backed administration dialog for the captured project.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_team_membership_loaded(
    app: &mut App,
    request_id: u64,
    project_id: String,
    memberships: Option<Vec<TeamMembershipDto>>,
    policy: Option<ChatProjectPolicyDto>,
    truncated: bool,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.team_request.finish(request_id) {
        return;
    }
    if app.active_project_id() != Some(project_id.as_str()) {
        return;
    }
    if unsupported {
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::Team,
            vec![
                "Team administration unavailable on this daemon.".to_string(),
                "Update the daemon to a build with the team.v1 surface.".to_string(),
            ],
        );
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::Team,
                vec![
                    "Team administration unavailable for this project.".to_string(),
                    "Requires project Owner (member.manage); outsiders see no membership detail."
                        .to_string(),
                ],
            );
        } else {
            app.messages_state
                .toasts
                .warning(&format!("Team list failed: {error}"));
        }
        return;
    }
    let Some(memberships) = memberships else {
        app.messages_state
            .toasts
            .warning("Team list returned no membership data");
        return;
    };
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Team,
        membership_lines(&project_id, &memberships, policy.as_ref(), truncated),
    );
}

/// Pure line renderer for the `/team` administration dialog (unit
/// tested; carries ids/roles/states/revisions only, never secrets).
fn membership_lines(
    project_id: &str,
    memberships: &[TeamMembershipDto],
    policy: Option<&ChatProjectPolicyDto>,
    truncated: bool,
) -> Vec<String> {
    let mut lines = vec![
        format!("Team — project {project_id} (durable administration)"),
        format!(
            "Members: {} shown{}",
            memberships.len(),
            if truncated { " (truncated at 200)" } else { "" }
        ),
        String::new(),
    ];
    if memberships.is_empty() {
        lines.push("No memberships recorded for this project.".to_string());
    }
    for member in memberships {
        lines.push(format!(
            "  {}  role={} state={} rev={}",
            member.principal_id, member.role, member.state, member.revision
        ));
    }
    lines.push(String::new());
    match policy {
        Some(policy) => {
            lines.push(format!(
                "Chat policy: project rev={} overrides={}",
                policy.revision,
                policy.overrides.len()
            ));
            for override_row in &policy.overrides {
                lines.push(format!(
                    "  {} chat={}",
                    override_row.principal_id,
                    override_row.decision.as_str()
                ));
            }
        }
        None => {
            lines.push("Chat policy: unavailable (requires member.manage).".to_string());
        }
    }
    lines.push(String::new());
    lines.push(
        "Manage: /team add <principal> <role> | /team role|suspend|reactivate|revoke <principal> <rev>"
            .to_string(),
    );
    lines.push(
        "Chat: /team chat-allow|chat-deny|chat-clear <principal> [--channel <id>]".to_string(),
    );
    lines.push("LocalOwner: /team principals | /team token-create <principal> <label>".to_string());
    lines.push("/collaborators remains the live presence view.".to_string());
    lines
}

// ── Membership mutations ───────────────────────────────────────────────────

fn start_membership_add(app: &mut App, project_id: String, principal_id: String, role: String) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_membership_add",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-add-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamMembershipAdd {
                    project_id: project_id.clone(),
                    principal_id,
                    role,
                },
            );
            Some(mutation_result(
                request_id,
                Some(project_id),
                core_client.request(req).await,
                reconnect_epoch,
                "member added",
            ))
        },
    );
}

fn start_membership_update(
    app: &mut App,
    project_id: String,
    principal_id: String,
    role: Option<String>,
    state: Option<String>,
    expected_revision: u64,
) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_membership_update",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-update-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamMembershipUpdate {
                    project_id: project_id.clone(),
                    principal_id,
                    expected_revision,
                    role,
                    state,
                },
            );
            Some(mutation_result(
                request_id,
                Some(project_id),
                core_client.request(req).await,
                reconnect_epoch,
                "membership updated",
            ))
        },
    );
}

fn start_membership_revoke(
    app: &mut App,
    project_id: String,
    principal_id: String,
    expected_revision: u64,
) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_membership_revoke",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-revoke-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamMembershipRevoke {
                    project_id: project_id.clone(),
                    principal_id,
                    expected_revision,
                },
            );
            Some(mutation_result(
                request_id,
                Some(project_id),
                core_client.request(req).await,
                reconnect_epoch,
                "membership revoked (takes effect at the next authorization boundary)",
            ))
        },
    );
}

fn mutation_result(
    request_id: u64,
    project_id: Option<String>,
    response: Result<CoreResponse, crate::error::AppError>,
    reconnect_epoch: u64,
    ok_summary: &str,
) -> TuiCommand {
    match response {
        Ok(CoreResponse::TeamMembership { membership }) => TuiCommand::TeamMutationFinished {
            request_id,
            project_id,
            message: Some(format!(
                "{} {} role={} state={} rev={} ({ok_summary})",
                membership.principal_id,
                membership.project_id,
                membership.role,
                membership.state,
                membership.revision
            )),
            error: None,
            unauthorized: false,
            reconnect_epoch,
        },
        Ok(CoreResponse::TeamPrincipal { principal }) => TuiCommand::TeamMutationFinished {
            request_id,
            project_id,
            message: Some(format!(
                "principal {} kind={} status={} rev={}",
                principal.principal_id, principal.kind, principal.status, principal.revision
            )),
            error: None,
            unauthorized: false,
            reconnect_epoch,
        },
        Ok(CoreResponse::TeamToken { token }) => TuiCommand::TeamMutationFinished {
            request_id,
            project_id,
            message: Some(format!(
                "token {} for {} revoked at {:?} (new authentications rejected)",
                token.token_id, token.principal_id, token.revoked_at_ms
            )),
            error: None,
            unauthorized: false,
            reconnect_epoch,
        },
        Ok(CoreResponse::ChatPolicy { project, .. }) => TuiCommand::TeamMutationFinished {
            request_id,
            project_id,
            message: Some(format!(
                "chat policy rev={} overrides={}",
                project.revision,
                project.overrides.len()
            )),
            error: None,
            unauthorized: false,
            reconnect_epoch,
        },
        Ok(CoreResponse::Error { code, message }) => {
            let unauthorized = unauthorized_of(&code);
            let hint = if code == "team_revision_conflict" {
                " — stale revision; run /team to reload and retry with the current rev"
            } else if code == "team_membership_conflict" {
                " — pair already exists (including revoked rows); use /team role|reactivate with the current rev"
            } else {
                ""
            };
            TuiCommand::TeamMutationFinished {
                request_id,
                project_id,
                message: None,
                error: Some(format!("{code}: {message}{hint}")),
                unauthorized,
                reconnect_epoch,
            }
        }
        Ok(other) => TuiCommand::TeamMutationFinished {
            request_id,
            project_id,
            message: None,
            error: Some(format!("Unexpected core response: {other:?}")),
            unauthorized: false,
            reconnect_epoch,
        },
        Err(e) => TuiCommand::TeamMutationFinished {
            request_id,
            project_id,
            message: None,
            error: Some(format!(
                "Team request failed: {e} — reconcile with /team before retrying (no silent retry)"
            )),
            unauthorized: false,
            reconnect_epoch,
        },
    }
}

fn failed_mutation(
    request_id: u64,
    project_id: Option<String>,
    error: &str,
    unauthorized: bool,
    reconnect_epoch: u64,
) -> TuiCommand {
    TuiCommand::TeamMutationFinished {
        request_id,
        project_id,
        message: None,
        error: Some(error.to_string()),
        unauthorized,
        reconnect_epoch,
    }
}

/// Apply a `TeamMutationFinished` completion. Success toasts the
/// structural summary and refreshes the `/team` view for the captured
/// project; stale completions drop.
pub(crate) fn apply_team_mutation_finished(
    app: &mut App,
    request_id: u64,
    project_id: Option<String>,
    message: Option<String>,
    error: Option<String>,
    unauthorized: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.team_request.finish(request_id) {
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.messages_state.toasts.warning(
                "Not authorized for team administration (project Owner or LocalOwner required)",
            );
        } else {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    if let Some(message) = message {
        app.messages_state.toasts.info(&message);
    }
    if let Some(project_id) = project_id {
        if app.active_project_id() == Some(project_id.as_str()) {
            start_show_team(app, project_id);
        }
    }
}

// ── Principal administration (LocalOwner-only) ─────────────────────────────

fn start_principals_list(app: &mut App) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_principal_list",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TeamPrincipalsLoaded {
                    request_id,
                    principals: None,
                    truncated: false,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            if !negotiate_team(&core_client).await {
                return Some(TuiCommand::TeamPrincipalsLoaded {
                    request_id,
                    principals: None,
                    truncated: false,
                    error: None,
                    unauthorized: false,
                    reconnect_epoch,
                });
            }
            let req = crate::core::new_request(
                format!("team-principals-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamPrincipalList { limit: None },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::TeamPrincipalList {
                    principals,
                    truncated,
                }) => Some(TuiCommand::TeamPrincipalsLoaded {
                    request_id,
                    principals: Some(principals),
                    truncated,
                    error: None,
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::TeamPrincipalsLoaded {
                        request_id,
                        principals: None,
                        truncated: false,
                        error: Some(format!("{code}: {message}")),
                        unauthorized: unauthorized_of(&code),
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::TeamPrincipalsLoaded {
                    request_id,
                    principals: None,
                    truncated: false,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::TeamPrincipalsLoaded {
                    request_id,
                    principals: None,
                    truncated: false,
                    error: Some(format!("Principal list failed: {e}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `TeamPrincipalsLoaded` completion (metadata only, never
/// secrets). Stale completions drop.
pub(crate) fn apply_team_principals_loaded(
    app: &mut App,
    request_id: u64,
    principals: Option<Vec<TeamPrincipalDto>>,
    truncated: bool,
    error: Option<String>,
    unauthorized: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.team_request.finish(request_id) {
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.messages_state.toasts.warning(
                "Principal administration requires LocalOwner (ordinary team principals fail closed)",
            );
        } else {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    // Unsupported (older daemon) arrives as principals=None + no error.
    let Some(principals) = principals else {
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::Team,
            vec![
                "Principal administration unavailable on this daemon.".to_string(),
                "Update the daemon to a build with the team.v1 surface.".to_string(),
            ],
        );
        return;
    };
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Team,
        principal_lines(&principals, truncated),
    );
}

/// Pure line renderer for the principal list (metadata only).
fn principal_lines(principals: &[TeamPrincipalDto], truncated: bool) -> Vec<String> {
    let mut lines = vec![
        "Principals — deployment scope (LocalOwner only)".to_string(),
        format!(
            "{} shown{}",
            principals.len(),
            if truncated { " (truncated at 200)" } else { "" }
        ),
        String::new(),
    ];
    for principal in principals {
        lines.push(format!(
            "  {}  kind={} status={} rev={} name={}",
            principal.principal_id,
            principal.kind,
            principal.status,
            principal.revision,
            principal.display_name
        ));
    }
    lines.push(String::new());
    lines.push("Create: /team principal-create <display-name> [--service]".to_string());
    lines.push("Status: /team principal-disable|principal-enable <principal-id> <rev>".to_string());
    lines.push(
        "Tokens: /team tokens <principal-id> | /team token-create <principal-id> <label>"
            .to_string(),
    );
    lines
}

fn start_principal_create(app: &mut App, display_name: String, service: bool) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_principal_create",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-principal-create-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamPrincipalCreate {
                    kind: Some(if service {
                        "service_account".to_string()
                    } else {
                        "human".to_string()
                    }),
                    display_name,
                },
            );
            Some(mutation_result(
                request_id,
                None,
                core_client.request(req).await,
                reconnect_epoch,
                "principal created",
            ))
        },
    );
}

fn start_principal_status(
    app: &mut App,
    principal_id: String,
    status: String,
    expected_revision: u64,
) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_principal_status",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-principal-status-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamPrincipalStatusSet {
                    principal_id,
                    status,
                    expected_revision,
                },
            );
            Some(mutation_result(
                request_id,
                None,
                core_client.request(req).await,
                reconnect_epoch,
                "principal status updated",
            ))
        },
    );
}

// ── Device-token administration (LocalOwner-only) ──────────────────────────

fn start_tokens_list(app: &mut App, principal_id: String) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_token_list",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TeamTokensLoaded {
                    request_id,
                    principal_id,
                    tokens: None,
                    truncated: false,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("team-tokens-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamTokenList {
                    principal_id: principal_id.clone(),
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::TeamTokenList { tokens, truncated }) => {
                    Some(TuiCommand::TeamTokensLoaded {
                        request_id,
                        principal_id,
                        tokens: Some(tokens),
                        truncated,
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::TeamTokensLoaded {
                    request_id,
                    principal_id,
                    tokens: None,
                    truncated: false,
                    error: Some(format!("{code}: {message}")),
                    unauthorized: unauthorized_of(&code),
                    reconnect_epoch,
                }),
                Ok(other) => Some(TuiCommand::TeamTokensLoaded {
                    request_id,
                    principal_id,
                    tokens: None,
                    truncated: false,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::TeamTokensLoaded {
                    request_id,
                    principal_id,
                    tokens: None,
                    truncated: false,
                    error: Some(format!("Token list failed: {e}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `TeamTokensLoaded` completion (metadata only, never
/// secrets). Stale completions drop.
pub(crate) fn apply_team_tokens_loaded(
    app: &mut App,
    request_id: u64,
    principal_id: String,
    tokens: Option<Vec<TeamTokenDto>>,
    truncated: bool,
    error: Option<String>,
    unauthorized: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.team_request.finish(request_id) {
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.messages_state.toasts.warning(
                "Token administration requires LocalOwner (ordinary team principals fail closed)",
            );
        } else {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    let Some(tokens) = tokens else {
        app.messages_state
            .toasts
            .warning("Token list returned no data");
        return;
    };
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Team,
        token_lines(&principal_id, &tokens, truncated),
    );
}

/// Pure line renderer for token metadata (never secrets: prefix +
/// label + lifecycle timestamps only).
fn token_lines(principal_id: &str, tokens: &[TeamTokenDto], truncated: bool) -> Vec<String> {
    let mut lines = vec![
        format!("Device tokens — principal {principal_id} (LocalOwner only; metadata only)"),
        format!(
            "{} shown{}",
            tokens.len(),
            if truncated { " (truncated at 200)" } else { "" }
        ),
        String::new(),
    ];
    if tokens.is_empty() {
        lines.push("No device tokens issued for this principal.".to_string());
    }
    for token in tokens {
        let status = if token.revoked_at_ms.is_some() {
            "revoked"
        } else {
            "live"
        };
        lines.push(format!(
            "  {}  prefix={} label={} status={} created={}",
            token.token_id, token.token_prefix, token.label, status, token.created_at_ms
        ));
    }
    lines.push(String::new());
    lines.push(
        "Issue: /team token-create <principal-id> <label> (credential shown once)".to_string(),
    );
    lines.push("Revoke: /team token-revoke <token-id> (monotonic)".to_string());
    lines
}

/// Mint one device token. The plaintext is returned exactly once and
/// rendered in the secret-safe modal; closing the modal destroys the
/// frontend copy. Each explicit `/team token-create` mints one
/// credential: after an ambiguous transport failure reconcile through
/// `/team tokens <principal>` and require explicit user confirmation
/// before re-issuing (never silent auto-retry).
fn start_token_create(app: &mut App, project_id: String, principal_id: String, label: String) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_token_create",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TeamTokenCreated {
                    request_id,
                    project_id: Some(project_id),
                    principal_id,
                    token: None,
                    plaintext: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("team-token-create-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamTokenCreate {
                    principal_id: principal_id.clone(),
                    label,
                    expires_at_ms: None,
                    idempotency_key: None,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::TeamTokenCreated { token, plaintext }) => {
                    Some(TuiCommand::TeamTokenCreated {
                        request_id,
                        project_id: Some(project_id),
                        principal_id,
                        token: Some(token),
                        plaintext: Some(plaintext),
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::TeamTokenCreated {
                        request_id,
                        project_id: Some(project_id),
                        principal_id,
                        token: None,
                        plaintext: None,
                        error: Some(format!("{code}: {message}")),
                        unauthorized: unauthorized_of(&code),
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::TeamTokenCreated {
                    request_id,
                    project_id: Some(project_id),
                    principal_id,
                    token: None,
                    plaintext: None,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::TeamTokenCreated {
                    request_id,
                    project_id: Some(project_id),
                    principal_id,
                    token: None,
                    plaintext: None,
                    error: Some(format!(
                        "Token create failed: {e} — reconcile with /team tokens before retrying (no silent retry)"
                    )),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `TeamTokenCreated` completion. Success mounts the one-time
/// secret-safe surface exactly once when the captured project is still
/// active; stale routes drop foreground display without logging the
/// credential and leave metadata for rotation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_team_token_created(
    app: &mut App,
    request_id: u64,
    project_id: Option<String>,
    principal_id: String,
    token: Option<crate::protocol::core::TeamTokenDto>,
    plaintext: Option<String>,
    error: Option<String>,
    unauthorized: bool,
    reconnect_epoch: u64,
) {
    if reconnect_epoch != epoch_of(app) {
        return;
    }
    if !app.dialog_state.team_request.finish(request_id) {
        return;
    }
    if let Some(error) = error {
        if unauthorized {
            app.messages_state.toasts.warning(
                "Token issuance requires LocalOwner (ordinary team principals fail closed)",
            );
        } else {
            app.messages_state.toasts.warning(&error);
        }
        return;
    }
    let (Some(token), Some(plaintext)) = (token, plaintext) else {
        app.messages_state.toasts.warning(
            "Token created but the one-time response was unusable; reconcile with /team tokens",
        );
        return;
    };
    // Stale route: the credential may have minted. Never log it to
    // make it recoverable; drop foreground display and leave metadata
    // plus explicit revoke/re-issue.
    if let Some(ref project_id) = project_id {
        if app.active_project_id() != Some(project_id.as_str()) {
            app.messages_state.toasts.info(&format!(
                "Token {} minted while another project is active; credential withheld — reconcile with /team tokens {} (revoke + re-issue for a new credential)",
                token.token_id, token.principal_id
            ));
            return;
        }
    }
    let bearer = match OneTimeBearer::new_device_token(plaintext) {
        Ok(bearer) => bearer,
        Err(error) => {
            app.messages_state.toasts.warning(&format!(
                "Token created but the one-time response was unusable ({error}); reconcile with /team tokens"
            ));
            return;
        }
    };
    // Mount the transient secret-safe surface exactly once.
    let secret_state = OneTimeDeviceToken {
        principal_id: principal_id.clone(),
        token_id: token.token_id.clone(),
        label: token.label.clone(),
        token: bearer,
        project_id,
    };
    app.dialog_state.team_token_secret = Some(secret_state);
    if let Some(secret_ref) = app.dialog_state.team_token_secret.as_ref() {
        use crate::tui::components::dialogs::device_secret::DeviceSecretDialog;
        let dialog = DeviceSecretDialog::from_secret(secret_ref);
        app.push_dialog(crate::tui::app::Dialog::TeamTokenSecret, Box::new(dialog));
    }
}

/// Close the one-time device-token dialog and forget the plaintext.
/// Token metadata stays listable; the credential is never re-readable.
pub(crate) fn close_team_token_secret(app: &mut App) {
    app.dialog_state.team_token_secret = None;
    // Pop only the secret dialog; leave the underlying view.
    app.focus_manager
        .pop_dialog(crate::tui::components::component::DialogType::TeamTokenSecret);
    let active = app.focus_manager.active_dialog_type();
    app.ui_state.dialog = crate::tui::app::Dialog::from(active);
}

fn start_token_revoke(app: &mut App, token_id: String) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_token_revoke",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-token-revoke-{}", uuid::Uuid::new_v4()),
                CoreRequest::TeamTokenRevoke { token_id },
            );
            Some(mutation_result(
                request_id,
                None,
                core_client.request(req).await,
                reconnect_epoch,
                "device token revoked",
            ))
        },
    );
}

// ── M002 chat-policy integration ───────────────────────────────────────────

/// Render the project chat-policy summary inside the `/team` surface
/// (revision-safe administration lives in the override subcommands).
fn start_chat_policy_view(app: &mut App, project_id: String) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_chat_policy_view",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TeamMembershipLoaded {
                    request_id,
                    project_id,
                    memberships: None,
                    policy: None,
                    truncated: false,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            };
            let req = crate::core::new_request(
                format!("team-chat-view-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatPolicyGet {
                    project_id: project_id.clone(),
                    channel_id: None,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatPolicy { project, .. }) => {
                    Some(TuiCommand::TeamMembershipLoaded {
                        request_id,
                        project_id,
                        memberships: Some(Vec::new()),
                        policy: Some(project),
                        truncated: false,
                        error: None,
                        unauthorized: false,
                        unsupported: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::TeamMembershipLoaded {
                        request_id,
                        project_id,
                        memberships: None,
                        policy: None,
                        truncated: false,
                        error: Some(format!("{code}: {message}")),
                        unauthorized: unauthorized_of(&code),
                        unsupported: false,
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::TeamMembershipLoaded {
                    request_id,
                    project_id,
                    memberships: None,
                    policy: None,
                    truncated: false,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::TeamMembershipLoaded {
                    request_id,
                    project_id,
                    memberships: None,
                    policy: None,
                    truncated: false,
                    error: Some(format!("Chat policy fetch failed: {e}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Set or clear one project/channel chat override (M002 operations
/// surfaced through `/team`). Channel overrides resolve the owning
/// project server-side; unknown channels fail closed.
fn start_chat_override(
    app: &mut App,
    project_id: String,
    principal_id: String,
    channel_id: Option<String>,
    decision: Option<ChatPolicyDecisionDto>,
) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let request = match channel_id {
        None => CoreRequest::ChatProjectPolicySet {
            project_id: project_id.clone(),
            principal_id,
            decision,
            expected_revision: None,
        },
        Some(channel_id) => CoreRequest::ChatChannelPolicySet {
            channel_id,
            mode: None,
            principal_id: Some(principal_id),
            decision,
            expected_revision: None,
        },
    };
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_chat_override",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-chat-override-{}", uuid::Uuid::new_v4()),
                request,
            );
            Some(mutation_result(
                request_id,
                Some(project_id),
                core_client.request(req).await,
                reconnect_epoch,
                "chat override recorded",
            ))
        },
    );
}

/// Set one channel's default mode (`restricted` or `inherit_project`).
fn start_channel_mode(
    app: &mut App,
    project_id: String,
    channel_id: String,
    mode: ChatChannelModeDto,
) {
    let request_id = app.dialog_state.team_request.begin();
    let reconnect_epoch = epoch_of(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "team_channel_mode",
        async move {
            let Some(core_client) = core_client else {
                return Some(failed_mutation(
                    request_id,
                    None,
                    "Core unavailable",
                    false,
                    reconnect_epoch,
                ));
            };
            let req = crate::core::new_request(
                format!("team-channel-mode-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatChannelPolicySet {
                    channel_id,
                    mode: Some(mode),
                    principal_id: None,
                    decision: None,
                    expected_revision: None,
                },
            );
            Some(mutation_result(
                request_id,
                Some(project_id),
                core_client.request(req).await,
                reconnect_epoch,
                "channel mode recorded",
            ))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, role: &str, state: &str, revision: u64) -> TeamMembershipDto {
        TeamMembershipDto {
            project_id: "project-1".to_string(),
            principal_id: id.to_string(),
            role: role.to_string(),
            state: state.to_string(),
            revision,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    #[test]
    fn membership_lines_carry_revisions_and_chat_status() {
        let members = vec![
            member("alice", "owner", "active", 3),
            member("bob", "viewer", "suspended", 7),
        ];
        let policy = ChatProjectPolicyDto {
            project_id: "project-1".to_string(),
            revision: 5,
            overrides: vec![],
        };
        let lines = membership_lines("project-1", &members, Some(&policy), false);
        let joined = lines.join("\n");
        assert!(joined.contains("alice"));
        assert!(joined.contains("rev=3"));
        assert!(joined.contains("rev=7"));
        assert!(joined.contains("project rev=5"));
        assert!(joined.contains("/collaborators remains the live presence view"));
        // No secret-bearing field names in the admin surface.
        assert!(!joined.contains("cggt_"));
        assert!(!joined.contains("digest"));
        assert!(!joined.contains("plaintext"));
    }

    #[test]
    fn membership_lines_without_policy_render_unavailable() {
        let lines = membership_lines("project-1", &[], None, false);
        let joined = lines.join("\n");
        assert!(joined.contains("No memberships"));
        assert!(joined.contains("unavailable (requires member.manage)"));
    }

    #[test]
    fn principal_lines_carry_no_secrets() {
        let principals = vec![TeamPrincipalDto {
            principal_id: "p-1".to_string(),
            kind: "human".to_string(),
            display_name: "Alice".to_string(),
            status: "active".to_string(),
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 2,
        }];
        let joined = principal_lines(&principals, false).join("\n");
        assert!(joined.contains("p-1"));
        assert!(!joined.contains("cggt_"));
        assert!(!joined.contains("digest"));
        assert!(!joined.contains("plaintext"));
    }

    #[test]
    fn token_lines_carry_prefix_never_plaintext() {
        let tokens = vec![TeamTokenDto {
            token_id: "token-1".to_string(),
            principal_id: "p-1".to_string(),
            token_prefix: "token-12".to_string(),
            label: "device".to_string(),
            created_at_ms: 1,
            expires_at_ms: None,
            revoked_at_ms: None,
        }];
        let joined = token_lines("p-1", &tokens, false).join("\n");
        assert!(joined.contains("token-1"));
        assert!(joined.contains("prefix=token-12"));
        assert!(!joined.contains("cggt_"));
        assert!(!joined.contains("digest"));
        assert!(!joined.contains("plaintext"));
    }

    #[test]
    fn channel_flag_parses_explicit_locator() {
        assert_eq!(
            channel_flag(&["--channel", "ch-1"]),
            Some("ch-1".to_string())
        );
        assert_eq!(channel_flag(&[]), None);
        assert_eq!(channel_flag(&["--channel"]), None);
    }

    #[test]
    fn unauthorized_covers_privacy_safe_shapes() {
        assert!(unauthorized_of("project_not_found"));
        assert!(unauthorized_of("authorization_denied"));
        assert!(unauthorized_of("authorization_scope_required"));
        assert!(!unauthorized_of("team_revision_conflict"));
    }
}
