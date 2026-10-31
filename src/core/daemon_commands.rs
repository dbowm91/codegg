//! M007-D: the `commands` request family for `CoreDaemon`.
//!
//! ADR-0052 (accepted 2026-10-24) moved command ownership into the daemon:
//! [`CoreRequest::CommandCatalog`] serves the daemon's catalog, the single
//! definition of what commands exist, and
//! [`CoreRequest::CommandExecute`] executes one **daemon-effect** form
//! under the capability the catalog entry names, audited as a canonical
//! `Run`. A refusal is typed and produces **no** `Run`; each code is a
//! `REFUSAL_*` constant.
//!
//! A command's effect is served by the daemon's handler for the equivalent
//! request. That inner request is authorized **again**, on its own
//! operation and capability, rather than inheriting the command's decision:
//! `command.execute` is the authority to *run* a command, never a widening
//! of what it may touch.

use crate::error::AppError;
use crate::protocol::core::{
    CommandCatalogDto, CommandDescriptorDto, CommandExecuteRequestDto, CommandExecuteResultDto,
    CommandFormDto, CommandKindDto, CommandOutcomeStatusDto, CommandReportDto, CoreRequest,
    CoreResponse,
};

use super::daemon::CoreDaemon;

use codegg_core::command_catalog::{
    self, bounds, catalog_version, find_by_command_id, the_catalog, CommandCapability, CommandKind,
};
use serde_json::json;

use codegg_core::run_store::{
    ActualBackend, BackendRecord, PlannedBackend, RiskRecord, RunCompletion, RunDraft,
    RunInvocation, RunKind, RunOwnership, RunStatus, RunStore,
};

/// The command identity is not in the catalog.
pub const REFUSAL_UNKNOWN_COMMAND: &str = "unknown_command";
/// The command has no such argument form.
pub const REFUSAL_UNKNOWN_FORM: &str = "unknown_form";
/// The form is a frontend effect; the daemon does not execute it.
pub const REFUSAL_NOT_DAEMON_EFFECT: &str = "not_daemon_effect";
/// The request names a catalog version the daemon no longer serves.
pub const REFUSAL_STALE_CATALOG: &str = "stale_catalog";
/// The argument exceeds its bound.
pub const REFUSAL_ARGUMENT_TOO_LONG: &str = "argument_too_long";
/// The catalog entry names no `command.execute` capability.
pub const REFUSAL_CAPABILITY_REQUIRED: &str = "capability_required";
/// The daemon cannot record the run, so it refuses to execute.
pub const REFUSAL_RUN_STORE_UNAVAILABLE: &str = "run_store_unavailable";
/// The daemon's own catalog exceeds its bounds.
pub const REFUSAL_CATALOG_OVER_BOUND: &str = "catalog_over_bound";
/// The produced result exceeds its bounds.
pub const REFUSAL_RESULT_OVER_BOUND: &str = "result_over_bound";
/// The daemon does not serve this command's effect yet.
pub const REFUSAL_COMMAND_NOT_SERVED: &str = "command_not_served";
/// The command acts on a session and the caller named none.
pub const REFUSAL_NO_SESSION: &str = "no_session";
/// The daemon serves the form, but the argument is not one it can read.
///
/// This is deliberately not `command_not_served`: that would say the
/// daemon does not serve the command, which is false.
pub const REFUSAL_ARGUMENT_UNREADABLE: &str = "argument_unreadable";

/// The daemon-side context a command's effect is built from: the project,
/// the workspace, and the session the caller named.
pub struct CommandContext<'a> {
    /// The project the command acts in.
    pub project_id: &'a str,
    /// The workspace the command acts in.
    pub workspace_id: &'a str,
    /// The session the command acts on, when the caller named one.
    pub session_id: Option<&'a str>,
    /// The turn the effect acts on, when the daemon resolved one.
    pub turn_id: Option<&'a str>,
    /// The command's argument.
    pub argument: &'a str,
    /// The authority the effect resolves for an identity the caller named.
    pub authority: Option<CommandAuthority<'a>>,
}

/// The binding the daemon resolved for an identity the caller named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandAuthority<'a> {
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub session_id: Option<&'a str>,
}

/// The identifier the argument names: its first whitespace-separated token.
fn argument_identifier(argument: &str) -> &str {
    let trimmed = argument.trim_start();
    trimmed.split_whitespace().next().unwrap_or_default()
}

/// The number in the argument's second token.
///
/// `Ok(None)` — the argument carries no second token, so the row reads an
/// absent value. `Ok(Some(n))` — the second token is the number.
/// `Err(())` — the argument carries a second token that is not a number,
/// so the row cannot read it and the call is refused as
/// `argument_unreadable` rather than as an unserved command.
fn argument_number(rest: &str) -> Result<Option<u64>, ()> {
    if rest.is_empty() {
        return Ok(None);
    }
    rest.parse::<u64>().map(Some).map_err(|_| ())
}

/// The listing bound in the argument's first token.
///
/// `Ok(None)` — the argument names no bound, so the row reads an absent
/// value and the daemon's own default applies. `Ok(Some(n))` — the first
/// token is the bound. `Err(())` — the first token is not a number, so
/// the row cannot read it and the call is refused as
/// `argument_unreadable` rather than as an unserved command.
fn argument_bound(first: &str) -> Result<Option<u32>, ()> {
    if first.is_empty() {
        return Ok(None);
    }
    first.parse::<u32>().map(Some).map_err(|_| ())
}

/// Build a daemon request from its wire shape.
///
/// The requests are built from JSON rather than from typed constructors so
/// that one table row is one line, and so that a row which names a shape the
/// protocol does not have fails *closed* — the deserialisation fails, the
/// entry yields nothing, and the command is refused with
/// `command_not_served` instead of dispatching a half-built request.
fn json_request(shape: serde_json::Value) -> Option<CoreRequest> {
    match serde_json::from_value(shape.clone()) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!(
                "PROBE-SHAPE-FAIL {} :: {}",
                shape.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
                e
            );
            None
        }
    }
}

/// What the daemon's effect table yields for one command form.
#[derive(Debug)]
pub enum CommandEffect {
    /// The request that serves the form.
    ///
    /// The request is boxed: it is far larger than the other variant, and
    /// an unboxed pair would make every match on this enum copy a
    /// `CoreRequest`.
    Ready(Box<CoreRequest>),
    /// The form acts on a session and the caller named none.
    ///
    /// The daemon resolves the project and the workspace from its own durable
    /// binding, but the session is the caller's selection: the daemon cannot
    /// guess which session a user is looking at.
    NeedsSession,
    /// The form has a row, and the argument the caller gave is not one the
    /// row can read.
    ///
    /// This is its own outcome rather than a missing row. The daemon *does*
    /// serve the form, so refusing it as `command_not_served` would
    /// misreport the daemon's capability — and the catalog publishes
    /// serve-ability from the row's presence, so the form stays dispatchable.
    UnreadableArgument,
}

/// The command's effect table.
///
/// Each row names the daemon request that serves the form, built from the
/// scope and the argument. A form with no row is refused with
/// `command_not_served`: the daemon does not guess what a command means.
pub fn build_inner_request(
    command_id: &str,
    form: &str,
    ctx: &CommandContext<'_>,
) -> Option<CommandEffect> {
    let project_id = ctx.project_id;
    let workspace_id = ctx.workspace_id;
    let session = ctx.session_id;
    let argument = ctx.argument;
    let trimmed = argument.trim_start();
    let first = argument_identifier(argument);
    let rest = trimmed[first.len()..].trim_start();
    let shape = match (command_id, form) {
        // ── assets and the project ──
        ("reload", "") => json!({
            "type": "asset_refresh",
            "request": {
                "scope": { "project_id": project_id, "workspace_id": workspace_id },
                "reason": "reload",
                "session_id": session,
            }
        }),
        ("project_init", "") => json!({
            "type": "project_init_draft_get",
            "project_id": project_id,
            "workspace_id": workspace_id,
        }),

        // ── the effective policy and the preferences ──
        //
        // M007-D, decision shape one: the `policy` row names **no** session.
        // The effective runtime policy is a global reading, so a
        // session-scoped read would answer a different question.
        ("policy", "") => json!({
            "type": "execution_policy_get",
            "session_id": null,
        }),
        ("approval", "") => json!({ "type": "approval_preference_get" }),
        ("reasoning", "") => json!({ "type": "reasoning_preference_get" }),
        // The network intent travels as the argument's first token and
        // the revision the caller holds as its second. The revision is
        // the compare-and-swap the policy read published, so dropping it
        // would turn a checked write into an unchecked one.
        ("network", "") => {
            let expected_revision = match argument_number(rest) {
                Ok(revision) => revision,
                Err(()) => return Some(CommandEffect::UnreadableArgument),
            };
            let expected_revision = match expected_revision {
                Some(revision) => serde_json::json!(revision),
                None => serde_json::Value::Null,
            };
            json!({
                "type": "network_intent_set",
                "network_intent": first,
                "expected_revision": expected_revision,
            })
        }

        // ── the daemon's own catalogues ──
        //
        // M007-017: `skills`, `worktree` and `tool_contracts` are frontend effects.
        ("models_refresh", "") => json!({ "type": "models_refresh" }),
        ("nodes", "") => json!({ "type": "node_telemetry", "node_id": null }),

        // ── sessions ──
        //
        // M007-D, decision shape two: the session a share, an unshare or
        // an export acts on is the one the **caller** named, so the session id
        // travels as the argument — the same place the turn id travels for
        // `stop`. A call that names no session is refused with `no_session`.
        ("share", "") => {
            if first.is_empty() {
                return Some(CommandEffect::NeedsSession);
            }
            json!({ "type": "session_share", "session_id": first })
        }
        ("unshare", "") => {
            if first.is_empty() {
                return Some(CommandEffect::NeedsSession);
            }
            json!({ "type": "session_unshare", "session_id": first })
        }
        ("export", "") => {
            if first.is_empty() {
                return Some(CommandEffect::NeedsSession);
            }
            json!({ "type": "session_export", "session_id": first })
        }
        // C002 (ADR-0060): the compaction request acts on the session the
        // **caller** selected, so the session travels in the scope and a
        // call that names none is refused with `no_session`.
        ("compact", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "session_compact",
                "session_id": session_id,
            })
        }
        // M007-017: `status` is a frontend effect now, so its row is gone.

        // ── goals, tasks and schedules ──
        //
        // The task listing's bound travels as the argument's first token.
        // The bound is a *reading* bound — how many work orders the
        // caller wants to see — and it belongs to the caller, so the
        // daemon does not pick one for it.
        ("tasks", "") | ("task", "") => {
            let limit = match argument_bound(first) {
                Ok(Some(limit)) => serde_json::json!(limit),
                // A caller that names no bound gets the daemon's own
                // reading bound, which is the one the row carried before
                // the bound became the caller's to choose.
                Ok(None) => serde_json::json!(20),
                Err(()) => return Some(CommandEffect::UnreadableArgument),
            };
            json!({
                "type": "work_order_list",
                "project_id": project_id,
                "state_filter": null,
                "cursor": null,
                "limit": limit,
            })
        }
        ("schedules", "") => json!({
            "type": "schedule_list",
            "workspace_id": workspace_id,
            "include_archived": false,
        }),
        ("goal", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "goal_show", "session_id": session_id })
        }
        ("state", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "active_goal_load", "session_id": session_id })
        }

        // ── memory ──
        ("memory", "") | ("memory_list", "") => json!({
            "type": "memory_list",
            "namespace": if argument.is_empty() { "user/preferences".to_string() } else { argument.to_string() },
        }),
        ("memory_search", "") => json!({ "type": "memory_search", "query": argument }),
        ("memory_remember", "") => json!({
            "type": "memory_remember",
            "text": argument,
            "namespace": Some("user/preferences".to_string()),
        }),
        ("memory_forget", "") => json!({ "type": "memory_forget", "id": argument }),

        // ── collaboration ──
        //
        // M007-017: the presence panel's own effect gets its own form. The
        // panel refreshes a **named** project, so the project travels in the
        // argument rather than in the scope.
        ("collaborators", "presence") => {
            if first.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            json!({ "type": "presence_snapshot_get", "project_id": first })
        }
        ("collaborators", "") => json!({
            "type": "team_principal_list",
            "limit": null,
        }),
        ("team", "") => json!({
            "type": "team_membership_list",
            "project_id": project_id,
            "limit": null,
        }),
        ("control", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({ "type": "session_control_get", "session_id": session_id })
        }
        // The observation the user asked for names the session it
        // follows. The scope's session is the *attached* session, and
        // `/observe <session-id>` names another one, so the session id
        // travels as the argument — the same place the share rows carry
        // theirs.
        ("observe", "") => {
            if first.is_empty() {
                return Some(CommandEffect::NeedsSession);
            }
            json!({
                "type": "projection_subscribe",
                "request": {
                    "scope": "session",
                    "scope_id": first,
                    "cursor": null,
                    "projection_version": 1,
                }
            })
        }
        // The turn the user wants stopped. The session travels in the scope
        // and the turn id travels as the argument: the daemon refuses a
        // turn that does not belong to the session.
        ("stop", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "turn_cancel",
                "session_id": session_id,
                "turn_id": first,
            })
        }
        // The structured actions of one channel. The argument carries the
        // channel and the optional message id, in that order; a channel the
        // daemon does not know fails closed.
        ("chat_action_list", "") => {
            let message_id = if rest.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(rest.to_string())
            };
            json!({
                "type": "chat_action_list",
                "channel_id": first,
                "message_id": message_id,
                "limit": null,
            })
        }
        // The projection subscription the user is following. The daemon
        // issued the id and the frontend holds the one it owns, so the id
        // travels as the argument.
        ("stop_observing", "") => json!({
            "type": "projection_unsubscribe",
            "subscription_id": first,
        }),
        // The background schedule the user named. The frontend resolves a
        // short id to the full one first, so the full id travels as the
        // argument.
        ("task_del", "") => json!({
            "type": "schedule_delete",
            "schedule_id": first,
        }),
        // The newest window of one channel. The channel travels as the
        // argument and the cursor is unanchored, which is the "newest N"
        // query the chat window wants.
        ("chat_history", "") => json!({
            "type": "chat_history_older",
            "channel_id": first,
            "before_seq": null,
            "limit": null,
        }),
        // The incremental sync of one channel. The argument carries the
        // channel and the cursor the frontend holds. The cursor is
        // required: an argument the daemon cannot read is refused as
        // `argument_unreadable`, not as an unserved command.
        ("chat_sync", "") => {
            let Some(from_seq) = argument_number(rest).ok().flatten() else {
                return Some(CommandEffect::UnreadableArgument);
            };
            json!({
                "type": "chat_sync",
                "channel_id": first,
                "from_seq": from_seq,
                "limit": null,
            })
        }
        // The read marker of one channel. The argument carries the channel
        // and the sequence the frontend marked. The sequence is required.
        ("chat_read", "") => {
            let Some(last_read_seq) = argument_number(rest).ok().flatten() else {
                return Some(CommandEffect::UnreadableArgument);
            };
            json!({
                "type": "chat_read_set",
                "channel_id": first,
                "last_read_seq": last_read_seq,
            })
        }
        // The attachment of one interactive process. The argument carries
        // the handle and, when the frontend resumes from a retained
        // cursor, that cursor. The cursor is optional; a cursor the daemon
        // cannot read is refused.
        ("terminal_attach", "") => {
            let from_seq = match argument_number(rest) {
                Ok(seq) => seq,
                Err(()) => return Some(CommandEffect::UnreadableArgument),
            };
            let from_seq = match from_seq {
                Some(seq) => serde_json::json!(seq),
                None => serde_json::Value::Null,
            };
            json!({
                "type": "interactive_process_attach",
                "handle": first,
                "from_seq": from_seq,
                "max_bytes": null,
            })
        }
        // The resume of one interactive process. The argument carries the
        // attachment and the cursor the frontend resumes from. The cursor
        // is required.
        ("terminal_resume", "") => {
            let Some(from_seq) = argument_number(rest).ok().flatten() else {
                return Some(CommandEffect::UnreadableArgument);
            };
            json!({
                "type": "interactive_process_resume",
                "attachment_id": first,
                "from_seq": from_seq,
                "max_bytes": null,
            })
        }
        // The wait for a submitted job. The argument carries the job id
        // and, when the frontend asked for a bound, that bound in
        // milliseconds. The bound is optional; a bound the daemon cannot
        // read is refused.
        ("test", "") => {
            let timeout_ms = match argument_number(rest) {
                Ok(ms) => ms,
                Err(()) => return Some(CommandEffect::UnreadableArgument),
            };
            let timeout_ms = match timeout_ms {
                Some(ms) => serde_json::json!(ms),
                None => serde_json::Value::Null,
            };
            json!({
                "type": "job_wait",
                "job_id": first,
                "timeout_ms": timeout_ms,
            })
        }

        // ── the editor's checkpoints ──
        ("edit_undo", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "edit_checkpoint_undo_latest",
                "workspace_id": workspace_id,
                "session_id": session_id,
            })
        }
        ("edit_reapply", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "edit_checkpoint_reapply_latest",
                "workspace_id": workspace_id,
                "session_id": session_id,
            })
        }
        ("edit_checkpoints", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "edit_checkpoint_list",
                "workspace_id": workspace_id,
                "session_id": session_id,
                "limit": 20,
            })
        }

        // ── the editor and the subagents ──
        //
        // M007-H: `/open` opens a document; the argument is the path.
        ("open_document", "") => {
            let path = argument.trim();
            if path.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            json!({
                "type": "document_open",
                "project_id": project_id,
                "workspace_id": workspace_id,
                "relative_path": path,
            })
        }
        // M007-H: `/subagent-send` steers the **parent** turn, so the row
        // names the turn the session is running. A session with no active
        // turn names none, and the turn handler refuses it — exactly as the
        // `stop` row refuses a turn it cannot name.
        ("subagent_send", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "turn_steer",
                "session_id": session_id,
                "turn_id": ctx.turn_id.unwrap_or(""),
                "text": argument,
            })
        }
        ("review", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            if first.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            json!({
                "type": "lsp_preview_read",
                "request": {
                    "preview_id": first,
                    "workspace_id": workspace_id,
                    "session_id": session_id,
                }
            })
        }
        ("security_review", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "security_review_start",
                "request": {
                    "protocol_version": codegg_protocol::security_review::SECURITY_REVIEW_PROTOCOL_VERSION,
                    "project_id": project_id,
                    "workspace_id": workspace_id,
                    "session_id": session_id,
                    "argument": argument.trim(),
                }
            })
        }
        ("security_review_show", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "security_review_read",
                "request": {
                    "protocol_version": codegg_protocol::security_review::SECURITY_REVIEW_PROTOCOL_VERSION,
                    "project_id": project_id,
                    "workspace_id": workspace_id,
                    "session_id": session_id,
                    "run_id": if first.is_empty() { serde_json::Value::Null } else { json!(first) },
                }
            })
        }
        ("security_review_cancel", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "turn_cancel",
                "session_id": session_id,
                "turn_id": ctx.turn_id.unwrap_or(""),
            })
        }
        ("shell_rerun", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "human_shell_run_list",
                "request": {
                    "protocol_version": codegg_protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
                    "project_id": project_id,
                    "workspace_id": workspace_id,
                    "session_id": session_id,
                    "limit": 20,
                }
            })
        }
        // M007-I (work package A): `/diff` reads the working tree against
        // `HEAD`. The daemon owns the read: it resolves the checkout from
        // its own durable binding and reads the bytes through the read-only
        // git fact layer. The argument is the repository-relative path; the
        // **empty** argument is the bare branch, which diffs the whole tree.
        ("diff", "") => {
            let path = argument.trim();
            let path = if path.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::json!(path)
            };
            json!({
                "type": "git_diff",
                "request": {
                    "project_id": project_id,
                    "workspace_id": workspace_id,
                    "session_id": session,
                    "path": path,
                }
            })
        }
        // M007-H (work package C): `/revert` restores paths in the
        // project's checkout. The daemon owns the write: it resolves the
        // checkout from its own durable binding, contains every path inside
        // it, and takes the checkout's writer permit before the first
        // effect. The argument is the repository-relative path.
        ("revert", "") => {
            let path = argument.trim();
            if path.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            json!({
                "type": "git_revert",
                "request": {
                    "project_id": project_id,
                    "workspace_id": workspace_id,
                    "session_id": session,
                    "paths": [path],
                }
            })
        }

        // ── the human shell ──
        //
        // M007-017: `shell_list` is a frontend effect now.
        ("shell_show", "") | ("shell_expand", "") => json!({
            "type": "human_shell_run_read",
            "request": {
                "run_id": first,
                "project_id": project_id,
                "workspace_id": workspace_id,
                "session_id": session,
                "from_sequence": 0,
                "max_bytes": 65536,
            }
        }),
        // The cancel's authority is the **run's own** binding, not the
        // caller's scope: the daemon resolved it before this row was built.
        ("shell_kill", "") => {
            if first.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            let Some(authority) = ctx.authority else {
                return Some(CommandEffect::UnreadableArgument);
            };
            json!({
                "type": "human_shell_run_cancel",
                "request": {
                    "run_id": first,
                    "project_id": authority.project_id,
                    "workspace_id": authority.workspace_id,
                    "session_id": authority.session_id,
                }
            })
        }

        // ── the interactive terminal ──
        //
        // The listing carries no bound of its own: the daemon's own
        // default is the bound the caller asked for, and a bound written
        // into the row would silently change how many terminals it shows.
        //
        // M007-017: `terminal_show` is a frontend effect now.
        ("terminal_list", "") => json!({
            "type": "interactive_process_list",
            "workspace_id": workspace_id,
            "limit": null,
        }),
        ("terminal_send", "") => json!({
            "type": "interactive_process_input",
            "attachment_id": first,
            "data_b64": rest,
        }),

        // ── the language server ──
        ("lsp_status", "")
        | ("lsp_servers", "")
        | ("lsp_capabilities", "")
        | ("lsp_errors", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            let selected_server = matches!(command_id, "lsp_capabilities" | "lsp_errors");
            if selected_server && ctx.argument.trim().is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            json!({
                "type": "lsp_server_status_get",
                "request": {
                    "session_id": session_id,
                    "server_id": if selected_server { Some(ctx.argument.trim()) } else { None },
                },
            })
        }
        ("lsp_restart", "") | ("lsp_stop", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            if argument.trim().is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            json!({
                "type": "lsp_control",
                "request": {
                    "session_id": session_id,
                    "server_id": argument.trim(),
                    "action": if command_id == "lsp_stop" { "stop" } else { "restart" },
                },
            })
        }
        ("lsp_cache_status", "") | ("lsp_cache_clear", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            json!({
                "type": "lsp_control",
                "request": {
                    "session_id": session_id,
                    "server_id": "",
                    "action": if command_id == "lsp_cache_clear" { "cache_clear" } else { "cache_status" },
                },
            })
        }
        ("lsp_root", "")
        | ("lsp_repair_local", "")
        | ("lsp_review_file", "")
        | ("lsp_context_diagnostics", "")
        | ("lsp_impact", "")
        | ("lsp_test_repair", "")
        | ("lsp_interface", "")
        | ("lsp_cross_repair", "")
        | ("lsp_call_neighbors", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            let argument = argument.trim();
            if argument.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            let mut request = serde_json::Map::new();
            request.insert("session_id".into(), json!(session_id));
            let mut primary_path = argument;
            let mut related_paths: Vec<&str> = Vec::new();
            let mut line = None;
            let mut column = None;
            let mut symbol = None;
            let mut direction = None;
            let mut failure_text = None;
            let workflow = match command_id {
                "lsp_root" => "root_diagnosis",
                "lsp_repair_local" => {
                    let parts: Vec<&str> = argument.splitn(3, ':').collect();
                    primary_path = parts[0];
                    line = parts.get(1).and_then(|value| value.parse::<u32>().ok());
                    column = parts.get(2).and_then(|value| value.parse::<u32>().ok());
                    if (parts.get(1).is_some() && line.is_none())
                        || (parts.get(2).is_some() && column.is_none())
                    {
                        return Some(CommandEffect::UnreadableArgument);
                    }
                    "repair_local"
                }
                "lsp_review_file" | "lsp_context_diagnostics" => "review_file",
                "lsp_impact" => {
                    let parts: Vec<&str> = argument.splitn(3, ':').collect();
                    if parts.len() != 3 {
                        return Some(CommandEffect::UnreadableArgument);
                    }
                    let (Ok(parsed_line), Ok(parsed_column)) =
                        (parts[1].parse::<u32>(), parts[2].parse::<u32>())
                    else {
                        return Some(CommandEffect::UnreadableArgument);
                    };
                    primary_path = parts[0];
                    line = Some(parsed_line);
                    column = Some(parsed_column);
                    "impact_analysis"
                }
                "lsp_test_repair" => {
                    let (path, detail) = argument.split_once(' ').unwrap_or((argument, ""));
                    primary_path = path;
                    failure_text = (!detail.is_empty()).then_some(detail);
                    "test_failure_repair"
                }
                "lsp_interface" => {
                    let (path, name) = argument.split_once(':').unwrap_or((argument, ""));
                    primary_path = path;
                    symbol = (!name.is_empty()).then_some(name);
                    "interface_boundary"
                }
                "lsp_cross_repair" => {
                    let paths: Vec<&str> = argument.split_whitespace().collect();
                    if paths.is_empty() || paths.len() > 9 {
                        return Some(CommandEffect::UnreadableArgument);
                    }
                    primary_path = paths[0];
                    related_paths = paths[1..].to_vec();
                    "cross_file_repair"
                }
                "lsp_call_neighbors" => {
                    let mut parts = argument.split_whitespace();
                    let position = parts.next().unwrap_or_default();
                    let position_parts: Vec<&str> = position.splitn(3, ':').collect();
                    if position_parts.len() != 3 {
                        return Some(CommandEffect::UnreadableArgument);
                    }
                    let (Ok(parsed_line), Ok(parsed_column)) = (
                        position_parts[1].parse::<u32>(),
                        position_parts[2].parse::<u32>(),
                    ) else {
                        return Some(CommandEffect::UnreadableArgument);
                    };
                    primary_path = position_parts[0];
                    line = Some(parsed_line);
                    column = Some(parsed_column);
                    direction = parts.next();
                    if parts.next().is_some() {
                        return Some(CommandEffect::UnreadableArgument);
                    }
                    "call_neighborhood"
                }
                _ => unreachable!("matched LSP workflow command above"),
            };
            request.insert("workflow".into(), json!(workflow));
            request.insert("primary_path".into(), json!(primary_path));
            request.insert("related_paths".into(), json!(related_paths));
            request.insert("line".into(), json!(line));
            request.insert("column".into(), json!(column));
            request.insert("symbol".into(), json!(symbol));
            request.insert("direction".into(), json!(direction));
            request.insert("failure_text".into(), json!(failure_text));
            json!({ "type": "lsp_workflow_execute", "request": request })
        }

        // ── M007-G (ADR-0057): the plugin family ──
        //
        // The three read-only plugin forms are frontend effects over the
        // daemon's published catalogue, so they have no row here. The four
        // mutating forms are daemon effects, each acting on a session:
        // the `plugin.manage` grant is checked against that session's
        // project.
        ("plugin_enable", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            if first.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            plugin_manage_shape(session_id, "enable", first)
        }
        ("plugin_disable", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            if first.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            plugin_manage_shape(session_id, "disable", first)
        }
        ("plugin_remove", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            if first.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            plugin_manage_shape(session_id, "remove", first)
        }
        // The install target is a **path**, and a path may contain
        // spaces, so the whole trimmed argument is the target here rather
        // than the argument's first token.
        ("plugin_install", "") => {
            let Some(session_id) = session else {
                return Some(CommandEffect::NeedsSession);
            };
            let target = argument.trim();
            if target.is_empty() {
                return Some(CommandEffect::UnreadableArgument);
            }
            plugin_manage_shape(session_id, "install", target)
        }

        _ => return None,
    };
    json_request(shape).map(|request| CommandEffect::Ready(Box::new(request)))
}

/// The effect shape of one mutating plugin form.
fn plugin_manage_shape(session_id: &str, action: &str, target: &str) -> serde_json::Value {
    json!({
        "type": "plugin_manage",
        "request": {
            "session_id": session_id,
            "action": action,
            "target": target,
        }
    })
}

/// Whether the daemon serves this command's effect.
///
/// The check asks whether the effect table has a row for the form at all,
/// independently of the caller's scope and argument. A row whose effect
/// needs a session still counts as served — the daemon refuses *that*
/// call, with `no_session` or `argument_unreadable`.
pub fn serves_form(command_id: &str, form: &str) -> bool {
    let ctx = CommandContext {
        project_id: "probe",
        workspace_id: "probe",
        session_id: Some("probe"),
        argument: "",

        authority: None,
        turn_id: None,
    };
    build_inner_request(command_id, form, &ctx).is_some()
}

/// The outcome of one command execution, before it becomes a DTO.
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    /// How the execution ended.
    pub status: CommandOutcomeStatusDto,
    /// A one-line summary.
    pub summary: String,
    /// The structured output lines.
    pub lines: Vec<String>,
    /// The structured report, when the command produces one.
    pub report: Option<CommandReportDto>,
}

impl CoreDaemon {
    /// M007-D: the `commands` request family.
    pub(crate) async fn handle_command_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::CommandCatalog { scope } => Ok(CoreResponse::CommandCatalog {
                catalog: self.command_catalog_dto(&scope)?,
            }),
            CoreRequest::CommandExecute { request } => {
                let result = self
                    .execute_command(
                        &request,
                        request_id,
                        trusted_client_id,
                        &authority,
                        &authz_decision,
                    )
                    .await?;
                Ok(CoreResponse::CommandExecute { result })
            }
            _ => {
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }

    /// Build the catalog DTO.
    ///
    /// An over-bound catalog is **refused**, never truncated: the daemon
    /// will not hand a frontend a silently shortened command list.
    fn command_catalog_dto(
        &self,
        _scope: &crate::protocol::core::CommandCatalogScopeDto,
    ) -> Result<CommandCatalogDto, AppError> {
        let catalog = the_catalog();
        if let Err(error) = command_catalog::check_catalog_bounds(catalog) {
            return Err(AppError::Other(anyhow::anyhow!(
                "{REFUSAL_CATALOG_OVER_BOUND}: {error}"
            )));
        }
        let commands = catalog
            .commands
            .iter()
            .map(|command| CommandDescriptorDto {
                command_id: command.command_id.to_string(),
                name: command.name.to_string(),
                aliases: command
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                description: command.description.to_string(),
                domain: command.domain.to_string(),
                kind: command_kind_dto(command.kind),
                capability: command
                    .capability
                    .map(|capability| capability.as_str().to_string()),
                forms: command
                    .forms
                    .iter()
                    .map(|form| CommandFormDto {
                        form: form.form.to_string(),
                        kind: command_kind_dto(form.kind),
                        argument_hint: form.argument_hint.to_string(),
                        argument_required: form.argument_required,
                        // The daemon serves exactly the forms its effect
                        // table has a row for. A frontend-effect form has no
                        // row: the frontend owns it.
                        daemon_served: serves_form(command.command_id, form.form),
                        // The action the frontend performs.
                        action: codegg_core::frontend_action::action_for_form(
                            command.command_id,
                            form.form,
                        )
                        .map(|action| action.as_str().to_string()),
                    })
                    .collect(),
            })
            .collect();
        Ok(CommandCatalogDto {
            catalog_version: catalog.version,
            commands,
        })
    }

    /// Resolve the authority an effect is authorized against, when the effect
    /// acts on an identity the caller named.
    async fn resolve_command_authority(
        &self,
        command_id: &str,
        form: &str,
        argument: &str,
    ) -> Option<crate::shell::daemon::HumanShellRunScope> {
        if command_id != "shell_kill" || !form.is_empty() {
            return None;
        }
        let run_id = argument_identifier(argument);
        if run_id.is_empty() {
            return None;
        }
        self.human_shell_runs.scope_for(run_id).await
    }

    /// Execute one daemon-effect command form.
    async fn execute_command(
        &self,
        request: &CommandExecuteRequestDto,
        request_id: &str,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        _authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CommandExecuteResultDto, AppError> {
        let started_at_ms = chrono::Utc::now().timestamp_millis();

        // The catalog version is checked first: a stale request is refused
        // with the current version so the frontend refetches rather than
        // guessing which command it is running.
        if request.catalog_version != catalog_version() {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_STALE_CATALOG,
                format!(
                    "catalog version {} is current; refetch and re-issue",
                    catalog_version()
                ),
                started_at_ms,
            ));
        }

        let Some(command) = find_by_command_id(&request.command_id) else {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_UNKNOWN_COMMAND,
                format!("no command named {}", request.command_id),
                started_at_ms,
            ));
        };

        let Some(form) = command.forms.iter().find(|form| form.form == request.form) else {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_UNKNOWN_FORM,
                format!("{} has no form named {}", command.name, request.form),
                started_at_ms,
            ));
        };

        // A frontend effect never reaches the daemon: the frontend owns it,
        // and this refusal produces no run.
        if form.kind != CommandKind::DaemonEffect {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_NOT_DAEMON_EFFECT,
                format!("the form named {} is a frontend effect", request.form),
                started_at_ms,
            ));
        }

        if request.argument.len() > bounds::MAX_ARGUMENT_BYTES {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_ARGUMENT_TOO_LONG,
                format!(
                    "argument is {} bytes, over the {} byte bound",
                    request.argument.len(),
                    bounds::MAX_ARGUMENT_BYTES
                ),
                started_at_ms,
            ));
        }

        // The capability is a property of the catalog entry, never a
        // client-supplied widening. The request DTO carries no capability
        // field at all, so there is nothing for a client to widen.
        match command.capability {
            Some(CommandCapability::CommandExecute) => {}
            _ => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_CAPABILITY_REQUIRED,
                    format!("{} names no command.execute capability", command.name),
                    started_at_ms,
                ));
            }
        }

        // The effect table is the daemon's own map from a command form to the
        // request that serves it. A form with no row is refused: the daemon
        // does not guess what a command means.
        //
        // The daemon resolves the binding before the row is built.
        let resolved_authority = self
            .resolve_command_authority(command.command_id, form.form, &request.argument)
            .await;
        let command_authority = resolved_authority.as_ref().map(|scope| CommandAuthority {
            project_id: scope.project_id.as_str(),
            workspace_id: scope.workspace_id.as_str(),
            session_id: scope.session_id.as_deref(),
        });
        // M007-H: a row that steers a turn names the turn the session runs.
        let active_turn_id = match request.scope.session_id.as_deref() {
            Some(session_id) => match self.sessions.get(session_id) {
                Some(runtime) => runtime
                    .active_turn
                    .read()
                    .await
                    .as_ref()
                    .map(|handle| handle.turn_id.clone()),
                None => None,
            },
            None => None,
        };
        // M007-H: the cancel names the review run's turn.
        let turn_id = match request.command_id.as_str() {
            "security_review_cancel" => match request.scope.session_id.as_deref() {
                Some(session_id) => self.security_reviews.running_turn_id(session_id),
                None => None,
            },
            _ => active_turn_id,
        };
        let ctx = CommandContext {
            project_id: &request.scope.project_id,
            workspace_id: &request.scope.workspace_id,
            session_id: request.scope.session_id.as_deref(),
            turn_id: turn_id.as_deref(),
            argument: &request.argument,
            authority: command_authority,
        };
        match build_inner_request(command.command_id, form.form, &ctx) {
            None => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_COMMAND_NOT_SERVED,
                    format!("the daemon does not serve {} yet", command.name),
                    started_at_ms,
                ))
            }
            Some(CommandEffect::NeedsSession) => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_NO_SESSION,
                    format!(
                        "{} acts on a session and the request named none",
                        command.name
                    ),
                    started_at_ms,
                ))
            }
            Some(CommandEffect::UnreadableArgument) => {
                // The daemon serves this form. The caller's argument is
                // what it cannot read, and saying so is the difference
                // between a fixable call and a missing capability.
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_ARGUMENT_UNREADABLE,
                    format!("{} takes an argument the daemon cannot read", command.name),
                    started_at_ms,
                ));
            }
            Some(CommandEffect::Ready(_)) => {}
        }

        // M007-D: the daemon audits every daemon-effect command as a
        // canonical `Run` (ADR-0052), and the run lands in the
        // **workspace's own** run store — `<workspace_root>/.codegg/runs`.
        //
        // Fail closed: without a store the execution would be unaudited,
        // and an unaudited daemon effect is not permitted.
        let workspace_root = self.command_workspace_root(&request.scope).await;
        let workspace_store;
        let store: &dyn RunStore = match self.run_store.as_ref() {
            Some(store) => store.as_ref(),
            None => {
                if workspace_root.as_os_str().is_empty() {
                    return Ok(refusal(
                        &request.command_id,
                        REFUSAL_RUN_STORE_UNAVAILABLE,
                        "the command's workspace could not be resolved, so its run cannot be \
                         recorded"
                            .to_string(),
                        started_at_ms,
                    ));
                }
                workspace_store = codegg_core::run_store::FsRunStore::new(
                    crate::work_plan_repository_binding::run_store_root(&workspace_root),
                );
                &workspace_store
            }
        };

        let draft = RunDraft {
            kind: RunKind::Command,
            invocation: RunInvocation {
                command: format!("/{}", command.command_id),
                argv: Some(vec![
                    command.command_id.to_string(),
                    form.form.to_string(),
                    request.argument.clone(),
                ]),
                script_hash: None,
            },
            session_id: None,
            parent_run_id: None,
            workspace_root: workspace_root.clone(),
            cwd: workspace_root,
            backend: BackendRecord {
                family: "command".to_string(),
                detail: Some(command.command_id.to_string()),
            },
            risk: RiskRecord {
                level: "low".to_string(),
                has_subprocess: false,
                has_git_mutation: false,
                has_destructive_mutation: false,
            },
            planned_backend: Some(PlannedBackend::NativeTool),
            actual_backend: Some(ActualBackend::NativeTool),
            ownership: RunOwnership::Caller,
            asset_provenance: None,
        };

        let handle = match store.begin_run(draft).await {
            Ok(handle) => handle,
            Err(error) => {
                return Ok(refusal(
                    &request.command_id,
                    REFUSAL_RUN_STORE_UNAVAILABLE,
                    format!("the run could not be opened: {error}"),
                    started_at_ms,
                ));
            }
        };
        let run_id = handle.run_id.as_str().to_string();

        let (outcome, inner) = self
            .dispatch_command_effect(
                command.command_id,
                form.form,
                &ctx,
                request_id,
                trusted_client_id,
                authority,
            )
            .await;

        let completed_at_ms = chrono::Utc::now().timestamp_millis();
        let run_status = match outcome.status {
            CommandOutcomeStatusDto::Completed => RunStatus::Complete,
            CommandOutcomeStatusDto::Refused => RunStatus::Failed,
            CommandOutcomeStatusDto::Failed => RunStatus::Failed,
            CommandOutcomeStatusDto::Cancelled => RunStatus::Cancelled,
        };
        let completion = RunCompletion {
            status: run_status,
            completed_at: chrono::Utc::now(),
            permissions: Vec::new(),
            sandbox: None,
            projection: None,
            changes: Vec::new(),
            rerun: None,
            actual_backend: Some(ActualBackend::NativeTool),
            fallback: None,
            remote_resources: None,
        };
        if let Err(error) = store.complete_run(handle, completion).await {
            tracing::warn!(
                run_id = %run_id,
                error = %error,
                "M007-D: the command run could not be closed"
            );
        }

        if outcome.lines.len() > bounds::MAX_RESULT_LINES {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_RESULT_OVER_BOUND,
                format!(
                    "the result carries {} lines, over the {} line bound",
                    outcome.lines.len(),
                    bounds::MAX_RESULT_LINES
                ),
                started_at_ms,
            ));
        }
        let serialized = serde_json::to_string(&outcome.lines).unwrap_or_default();
        if serialized.len() > bounds::MAX_RESULT_BYTES {
            return Ok(refusal(
                &request.command_id,
                REFUSAL_RESULT_OVER_BOUND,
                format!(
                    "the result serializes to {} bytes, over the {} byte bound",
                    serialized.len(),
                    bounds::MAX_RESULT_BYTES
                ),
                started_at_ms,
            ));
        }

        Ok(CommandExecuteResultDto {
            run_id,
            command_id: request.command_id.clone(),
            status: outcome.status,
            summary: outcome.summary,
            lines: outcome.lines,
            report: outcome.report,
            started_at_ms,
            completed_at_ms,
            // The inner response travels verbatim: the daemon owns the
            // effect and the authoritative data, and the frontend keeps the
            // formatting by rendering the response it already knows how to
            // render.
            response: inner.and_then(|response| serde_json::to_value(response).ok()),
        })
    }

    /// Resolve the workspace root the command ran in.
    ///
    /// The daemon resolves the root from its own durable binding. When it
    /// cannot, the run records an empty root rather than guessing: the
    /// daemon never consults the process working directory.
    async fn command_workspace_root(
        &self,
        scope: &crate::protocol::core::CommandCatalogScopeDto,
    ) -> std::path::PathBuf {
        let Some(resolver) = self.context_resolver.as_ref() else {
            return std::path::PathBuf::new();
        };
        let Ok(project_id) = codegg_core::identity::ProjectId::parse(&scope.project_id) else {
            return std::path::PathBuf::new();
        };
        let Ok(workspace_id) = codegg_core::workspace::WorkspaceId::parse(&scope.workspace_id)
        else {
            return std::path::PathBuf::new();
        };
        match resolver
            .resolve(codegg_core::context::ProjectContextRequest::new(
                project_id,
                workspace_id,
            ))
            .await
        {
            Ok(context) => context.workspace_root,
            Err(_) => std::path::PathBuf::new(),
        }
    }

    /// Serve one command effect by running the daemon's own handler for the
    /// equivalent request.
    #[allow(clippy::too_many_arguments)]
    async fn dispatch_command_effect(
        &self,
        command_id: &str,
        form: &str,
        ctx: &CommandContext<'_>,
        request_id: &str,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
    ) -> (CommandOutcome, Option<CoreResponse>) {
        let inner = match build_inner_request(command_id, form, ctx) {
            Some(CommandEffect::Ready(inner)) => *inner,
            _ => {
                return (
                    CommandOutcome {
                        status: CommandOutcomeStatusDto::Failed,
                        summary: "the command's effect could not be prepared".to_string(),
                        lines: Vec::new(),
                        report: None,
                    },
                    None,
                )
            }
        };

        // The inner request is authorized on its own operation and
        // capability. `command.execute` is the authority to run a command,
        // never a widening of what the command may touch.
        let decision = match self.authorize_request(authority, request_id, &inner).await {
            Ok(decision) => decision,
            Err(error) => {
                return (
                    CommandOutcome {
                        status: CommandOutcomeStatusDto::Refused,
                        summary: format!("the command's effect was refused: {error}"),
                        lines: vec![format!("refused: {error}")],
                        report: None,
                    },
                    None,
                )
            }
        };

        let family = super::daemon_family::DaemonRequestFamily::of(&inner);
        let response = match self
            .dispatch_family_request(
                family,
                inner,
                request_id,
                trusted_client_id,
                authority.clone(),
                decision,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return (
                    CommandOutcome {
                        status: CommandOutcomeStatusDto::Failed,
                        summary: format!("the command's effect failed: {error}"),
                        lines: vec![format!("failed: {error}")],
                        report: None,
                    },
                    None,
                )
            }
        };

        let outcome = summarize(&response);
        (outcome, Some(response))
    }
}

/// Summarize the inner response into the command's bounded result.
///
/// The renderer is variant-agnostic on purpose: it reads the response's
/// wire shape, so one function covers every command family and a new
/// family needs no new renderer. A frontend that wants richer text reads
/// the inner response itself and renders it with the presentation code it
/// already had.
fn summarize(response: &CoreResponse) -> CommandOutcome {
    if let CoreResponse::Error { code, message } = response {
        return CommandOutcome {
            status: CommandOutcomeStatusDto::Refused,
            summary: message.clone(),
            lines: vec![format!("{code}: {message}")],
            report: None,
        };
    }
    let value = serde_json::to_value(response).ok();
    let kind = value
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or("result")
        .to_string();
    let lines = render_lines(value.as_ref(), &kind);
    CommandOutcome {
        status: CommandOutcomeStatusDto::Completed,
        summary: format!("{kind} completed"),
        lines,
        report: None,
    }
}

/// The bounded lines a response renders to.
///
/// A list of records renders as one line per record's naming field; every
/// other shape renders as its kind. The renderer never renders a payload
/// that is not already bounded by the protocol.
fn render_lines(value: Option<&serde_json::Value>, kind: &str) -> Vec<String> {
    let Some(object) = value.and_then(|value| value.as_object()) else {
        return vec![kind.to_string()];
    };
    if kind == "lsp_workflow_result" {
        if let Some(rendered) = object
            .get("result")
            .and_then(|result| result.get("rendered"))
            .and_then(serde_json::Value::as_str)
        {
            return rendered
                .lines()
                .take(bounds::MAX_RESULT_LINES)
                .map(|line| line.chars().take(2_000).collect())
                .collect();
        }
    }
    if kind == "plugin_manage_result" {
        if let Some(result) = object.get("result").and_then(serde_json::Value::as_object) {
            let plugin_id = result
                .get("plugin_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let message = result
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("the plugin request completed");
            if !plugin_id.is_empty() {
                return vec![format!("{plugin_id}: {message}")];
            }
            return vec![message.to_string()];
        }
    }
    if kind == "lsp_control_result" {
        if let Some(result) = object.get("result").and_then(serde_json::Value::as_object) {
            let server_id = result
                .get("server_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let state = result
                .get("state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            if !server_id.is_empty() {
                return vec![format!("{server_id}: {state}")];
            }
            let entries = result
                .get("cache_entries")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let bytes = result
                .get("cache_bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            return vec![format!(
                "LSP semantic cache: {entries} entries, {bytes} bytes ({state})"
            )];
        }
    }
    for field in ["items", "sessions", "entries", "skills", "records", "rows"] {
        let Some(serde_json::Value::Array(items)) = object.get(field) else {
            continue;
        };
        if items.is_empty() {
            return vec![format!("{kind}: 0")];
        }
        return items
            .iter()
            .take(64)
            .map(|item| {
                item.get("name")
                    .or_else(|| item.get("title"))
                    .or_else(|| item.get("id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("entry")
                    .to_string()
            })
            .collect();
    }
    vec![kind.to_string()]
}

/// Build a typed refusal. A refusal produces **no** run.
fn refusal(
    command_id: &str,
    code: &str,
    reason: String,
    started_at_ms: i64,
) -> CommandExecuteResultDto {
    CommandExecuteResultDto {
        run_id: String::new(),
        command_id: command_id.to_string(),
        status: CommandOutcomeStatusDto::Refused,
        summary: reason.clone(),
        lines: vec![format!("{code}: {reason}")],
        report: None,
        started_at_ms,
        completed_at_ms: started_at_ms,
        response: None,
    }
}

/// Map the domain kind onto the wire kind.
fn command_kind_dto(kind: CommandKind) -> CommandKindDto {
    match kind {
        CommandKind::DaemonEffect => CommandKindDto::DaemonEffect,
        CommandKind::FrontendEffect => CommandKindDto::FrontendEffect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::command_catalog::{the_catalog, CommandKind};

    /// Every row of the effect table.
    ///
    /// The list is the table's key set. The `the_effect_table_is_closed` test
    /// proves it agrees with the match arms in both directions, so a row
    /// cannot be added to one and forgotten in the other.
    const EFFECT_TABLE: &[(&str, &str, &str)] = &[
        ("reload", "", ""),
        ("project_init", "", ""),
        ("policy", "", ""),
        ("approval", "", ""),
        ("reasoning", "", ""),
        ("network", "", "offline 7"),
        ("models_refresh", "", ""),
        ("nodes", "", ""),
        ("share", "", "session-1"),
        ("unshare", "", "session-1"),
        ("export", "", "session-1"),
        ("compact", "", ""),
        ("tasks", "", "200"),
        ("task", "", "200"),
        ("schedules", "", ""),
        ("goal", "", ""),
        ("state", "", ""),
        ("memory", "", ""),
        ("memory_list", "", ""),
        ("memory_search", "", ""),
        ("memory_remember", "", ""),
        ("memory_forget", "", ""),
        ("collaborators", "", ""),
        ("collaborators", "presence", "probe-project"),
        ("team", "", ""),
        ("control", "", ""),
        ("observe", "", "session-1"),
        ("stop", "", ""),
        ("chat_action_list", "", ""),
        ("stop_observing", "", ""),
        ("task_del", "", ""),
        ("chat_history", "", ""),
        ("chat_sync", "", "probe 1"),
        ("chat_read", "", "probe 1"),
        ("terminal_attach", "", "probe 1"),
        ("terminal_resume", "", "probe 1"),
        ("test", "", "probe 1"),
        ("edit_undo", "", ""),
        ("edit_reapply", "", ""),
        ("edit_checkpoints", "", ""),
        // M007-H: the editor, the subagents, the review, the shell.
        ("open_document", "", "src/lib.rs"),
        ("subagent_send", "", "probe-agent"),
        ("review", "", "preview-1"),
        ("security_review", "", "--changed"),
        ("security_review_show", "", ""),
        ("security_review_cancel", "", ""),
        ("shell_rerun", "", "run-1"),
        // M007-H (work package C): the daemon's own `/revert`.
        ("revert", "", "src/lib.rs"),
        ("diff", "", "src/lib.rs"),
        ("shell_show", "", "probe"),
        ("shell_expand", "", "probe"),
        ("shell_kill", "", "probe"),
        ("terminal_list", "", ""),
        ("terminal_send", "", "probe ZGF0YQ=="),
        ("lsp_status", "", ""),
        ("lsp_servers", "", ""),
        ("lsp_capabilities", "", "rust-analyzer"),
        ("lsp_errors", "", "rust-analyzer"),
        ("lsp_restart", "", "rust-analyzer"),
        ("lsp_stop", "", "rust-analyzer"),
        ("lsp_cache_status", "", ""),
        ("lsp_cache_clear", "", ""),
        ("lsp_review_file", "", "src/lib.rs"),
        ("lsp_impact", "", "src/lib.rs:1:1"),
        ("lsp_root", "", "src/lib.rs"),
        ("lsp_context_diagnostics", "", "src/lib.rs"),
        ("lsp_repair_local", "", "src/lib.rs:1"),
        ("lsp_test_repair", "", "tests/foo.rs failed"),
        ("lsp_interface", "", "src/lib.rs:Thing"),
        ("lsp_cross_repair", "", "src/lib.rs tests/foo.rs"),
        ("lsp_call_neighbors", "", "src/lib.rs:1:1 incoming"),
        // M007-G (ADR-0057): the four mutating plugin forms.
        ("plugin_enable", "", "probe-plugin"),
        ("plugin_disable", "", "probe-plugin"),
        ("plugin_remove", "", "probe-plugin"),
        ("plugin_install", "", "/tmp/probe-plugin"),
    ];

    fn daemon_effect_forms() -> Vec<(&'static str, &'static str)> {
        the_catalog()
            .commands
            .iter()
            .flat_map(|command| {
                command
                    .forms
                    .iter()
                    .filter(|form| form.kind == CommandKind::DaemonEffect)
                    .map(move |form| (command.command_id, form.form))
            })
            .collect()
    }

    /// Every row of the effect table names a real daemon-effect form, and
    /// every daemon-effect form the table serves is listed.
    ///
    /// The two directions matter: a row naming a form the catalog does not
    /// have is dead, and a listed row whose match arm is missing is a
    /// command the daemon claims to serve and then refuses.
    #[test]
    fn the_effect_table_is_closed() {
        let daemon_forms = daemon_effect_forms();
        for (command_id, form, _) in EFFECT_TABLE {
            assert!(
                daemon_forms.contains(&(*command_id, *form)),
                "{command_id}/{form} is not a daemon-effect form in the catalog"
            );
            assert!(
                serves_form(command_id, form),
                "{command_id}/{form} is listed but the daemon does not build a request for it"
            );
        }
        let served = daemon_forms
            .iter()
            .filter(|(command_id, form)| serves_form(command_id, form))
            .count();
        assert_eq!(
            served,
            EFFECT_TABLE.len(),
            "the served daemon-effect forms and the listed rows must agree"
        );
    }

    /// M007-G (ADR-0057): the seven plugin forms split across the daemon's
    /// two surfaces exactly as the accepted ADR says they do.
    ///
    /// The three read-only forms are frontend effects over the daemon's
    /// published catalogue and have no effect-table row. The four mutating
    /// forms each build a `PluginManage` request that carries the action
    /// and the target, and each refuses a call that names no session.
    #[test]
    fn the_plugin_forms_split_between_the_daemons_two_surfaces() {
        for command_id in ["plugins", "plugin_info", "plugin_doctor"] {
            assert!(
                !serves_form(command_id, ""),
                "{command_id} is a read-only plugin form and the daemon must not serve it"
            );
            let ctx = CommandContext {
                project_id: "project",
                workspace_id: "workspace",
                session_id: Some("session"),
                argument: "",
                authority: None,
                turn_id: None,
            };
            assert!(
                build_inner_request(command_id, "", &ctx).is_none(),
                "{command_id} must have no effect-table row"
            );
        }

        let expected = [
            ("plugin_enable", "enable", "probe-plugin"),
            ("plugin_disable", "disable", "probe-plugin"),
            ("plugin_remove", "remove", "probe-plugin"),
            ("plugin_install", "install", "/tmp/probe-plugin"),
        ];
        for (command_id, action, target) in expected {
            let ctx = CommandContext {
                project_id: "project",
                workspace_id: "workspace",
                session_id: Some("session-1"),
                argument: target,
                authority: None,
                turn_id: None,
            };
            let effect = build_inner_request(command_id, "", &ctx)
                .unwrap_or_else(|| panic!("{command_id} must have a row"));
            let CommandEffect::Ready(inner) = effect else {
                panic!("{command_id} must build a request");
            };
            let CoreRequest::PluginManage { request } = *inner else {
                panic!("{command_id} must build a plugin management request");
            };
            assert_eq!(
                request.action.as_str(),
                action,
                "{command_id} must name its own action"
            );
            assert_eq!(request.target, target);
            assert_eq!(request.session_id, "session-1");

            let without_session = CommandContext {
                project_id: "project",
                workspace_id: "workspace",
                session_id: None,
                argument: target,
                authority: None,
                turn_id: None,
            };
            let refused = build_inner_request(command_id, "", &without_session)
                .unwrap_or_else(|| panic!("{command_id} must have a row"));
            assert!(
                matches!(refused, CommandEffect::NeedsSession),
                "{command_id} must refuse a call that names no session"
            );
        }
    }

    /// Every row builds a request the protocol actually has.
    ///
    /// The table's rows are built from JSON so that a wrong shape fails
    /// *closed*. This test is what keeps that from becoming a silent
    /// refusal: a row whose shape the protocol does not have fails here
    /// rather than turning into `command_not_served` in production.
    ///
    /// Each row is probed with the argument **it declares**, so a row that
    /// reads more than an identifier is proved with an argument that
    /// carries what it reads.
    #[test]
    fn every_row_builds() {
        for (command_id, form, argument) in EFFECT_TABLE {
            let ctx = CommandContext {
                project_id: "project",
                workspace_id: "workspace",
                session_id: Some("session"),
                argument,
                // A row that acts on a named identity carries its binding.
                authority: Some(CommandAuthority {
                    project_id: "project",
                    workspace_id: "workspace",
                    session_id: Some("session"),
                }),
                turn_id: Some("turn-1"),
            };
            let effect = build_inner_request(command_id, form, &ctx)
                .unwrap_or_else(|| panic!("{command_id}/{form} must have a row"));
            assert!(
                matches!(effect, CommandEffect::Ready(_)),
                "{command_id}/{form} must build a request for the argument it declares"
            );
        }
    }

    /// The `subagent_send` row steers the turn the **session** runs, and never
    /// the subagent's task id.
    ///
    /// M007-H moved `/subagent-send` behind the command executor, so this row
    /// is what names the turn. The frontend holds the subagent's `task_id` — a
    /// daemon-local `u64` — and `TurnSteer` is keyed on a *session turn*, so a
    /// steer naming the task id would resolve to `turn_id_mismatch` every
    /// time. The row takes the turn the daemon resolved for the session.
    ///
    /// A session that runs no turn names none: the daemon's own `TurnSteer`
    /// handler refuses the empty turn, which is the answer the user sees.
    #[test]
    fn the_subagent_send_row_steers_the_resolved_session_turn() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            turn_id: Some("turn-live"),
            argument: "try the second retry first",
            authority: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("subagent_send", "", &ctx)
        else {
            panic!("the addressed reply must build a request");
        };
        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value["type"], "turn_steer");
        assert_eq!(value["session_id"], "session");
        assert_eq!(
            value["turn_id"], "turn-live",
            "the steer names the turn the session runs"
        );
        assert_eq!(value["text"], "try the second retry first");

        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            turn_id: None,
            argument: "try the second retry first",
            authority: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("subagent_send", "", &ctx)
        else {
            panic!("the addressed reply must build a request");
        };
        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(
            value["turn_id"], "",
            "a session that runs no turn names no turn, and never the task id"
        );
    }

    /// C002 (ADR-0060): the `compact` row asks the daemon to compact the
    /// **caller's** session, and refuses a call that names none.
    #[test]
    fn the_compact_row_asks_for_the_callers_session() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session-1"),
            argument: "",
            authority: None,
            turn_id: None,
        };
        let effect = build_inner_request("compact", "", &ctx)
            .unwrap_or_else(|| panic!("compact must have a row"));
        let CommandEffect::Ready(inner) = effect else {
            panic!("compact must build a request");
        };
        assert!(
            matches!(
                inner.as_ref(),
                CoreRequest::SessionCompact { session_id } if session_id == "session-1"
            ),
            "the row must name the session it compacts"
        );

        let without_session = CommandContext {
            session_id: None,
            ..ctx
        };
        let refused = build_inner_request("compact", "", &without_session)
            .unwrap_or_else(|| panic!("compact must have a row"));
        assert!(
            matches!(refused, CommandEffect::NeedsSession),
            "compact must refuse a call that names no session"
        );
    }

    #[test]
    fn lsp_workflow_forms_build_typed_session_scoped_requests() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "src/lib.rs:5:7 outgoing",
            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) =
            build_inner_request("lsp_call_neighbors", "", &ctx)
        else {
            panic!("call-neighborhood command must build its typed request");
        };
        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value["type"], "lsp_workflow_execute");
        assert_eq!(value["request"]["session_id"], "session");
        assert_eq!(value["request"]["workflow"], "call_neighborhood");
        assert_eq!(value["request"]["primary_path"], "src/lib.rs");
        assert_eq!(value["request"]["line"], 5);
        assert_eq!(value["request"]["column"], 7);
        assert_eq!(value["request"]["direction"], "outgoing");
    }

    #[test]
    fn lsp_workflow_rejects_malformed_position_and_excess_related_paths() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "src/lib.rs:not-a-position",
            authority: None,
            turn_id: None,
        };
        assert!(matches!(
            build_inner_request("lsp_impact", "", &ctx),
            Some(CommandEffect::UnreadableArgument)
        ));
        let ctx = CommandContext {
            argument: "primary.rs a.rs b.rs c.rs d.rs e.rs f.rs g.rs h.rs i.rs",
            ..ctx
        };
        assert!(matches!(
            build_inner_request("lsp_cross_repair", "", &ctx),
            Some(CommandEffect::UnreadableArgument)
        ));
    }

    /// An unreadable argument is refused as such, and the form stays
    /// served.
    ///
    /// The two facts are one invariant: serve-ability is a property of the
    /// form, so a caller that got the argument wrong must be told the
    /// argument was wrong — not that the daemon does not serve the
    /// command.
    #[test]
    fn an_unreadable_argument_is_refused_and_the_form_stays_served() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "channel-1 not-a-number",

            authority: None,
            turn_id: None,
        };
        let effect = build_inner_request("chat_sync", "", &ctx);
        assert!(
            matches!(effect, Some(CommandEffect::UnreadableArgument)),
            "a cursor the daemon cannot read must be its own outcome"
        );
        assert!(
            serves_form("chat_sync", ""),
            "a form whose argument was wrong is still served"
        );
        // The same argument with a readable cursor builds.
        let ctx = CommandContext {
            argument: "channel-1 42",
            ..ctx
        };
        assert!(
            matches!(
                build_inner_request("chat_sync", "", &ctx),
                Some(CommandEffect::Ready(_))
            ),
            "a readable cursor must build the row"
        );
    }

    /// A session-scoped row refuses when the caller named no session.
    ///
    /// The refusal is distinct from `command_not_served`: the daemon serves
    /// the form, it just cannot guess which session the caller meant.
    #[test]
    fn a_session_scoped_row_refuses_without_a_session() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: None,
            argument: "",

            authority: None,
            turn_id: None,
        };
        let effect = build_inner_request("share", "", &ctx).expect("share has a row");
        assert!(
            matches!(effect, CommandEffect::NeedsSession),
            "share acts on a session and must refuse without one"
        );
        // A row that does not act on a session still builds.
        let effect = build_inner_request("policy", "", &ctx).expect("policy has a row");
        assert!(
            matches!(effect, CommandEffect::Ready(_)),
            "policy does not act on a session and must build without one"
        );
    }

    /// A form with no row is not served, and the daemon says so.
    #[test]
    fn a_form_with_no_row_is_not_served() {
        assert!(!serves_form("not_a_command", ""));
        assert!(!serves_form("reload", "not_a_form"));
    }

    /// The cancel's authority is the **run's own** binding, not the caller's.
    #[test]
    fn the_cancel_is_authorized_against_the_runs_own_binding() {
        let ctx = CommandContext {
            project_id: "caller-project",
            workspace_id: "caller-workspace",
            session_id: Some("caller-session"),
            argument: "run-1",
            authority: Some(CommandAuthority {
                project_id: "run-project",
                workspace_id: "run-workspace",
                session_id: Some("run-session"),
            }),
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("shell_kill", "", &ctx)
        else {
            panic!("shell_kill must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::HumanShellRunCancel { request }
                    if request.run_id == "run-1"
                        && request.project_id == "run-project"
                        && request.workspace_id == "run-workspace"
                        && request.session_id.as_deref() == Some("run-session")
            ),
            "the cancel must name the run's own binding, not the caller's scope"
        );
    }

    /// A run the daemon does not hold has no binding, so the row refuses.
    #[test]
    fn a_cancel_with_no_resolved_run_refuses_the_call_and_stays_served() {
        let ctx = CommandContext {
            project_id: "caller-project",
            workspace_id: "caller-workspace",
            session_id: Some("caller-session"),
            argument: "run-1",
            authority: None,
            turn_id: None,
        };
        assert!(
            matches!(
                build_inner_request("shell_kill", "", &ctx),
                Some(CommandEffect::UnreadableArgument)
            ),
            "a cancel the daemon cannot resolve must refuse the call"
        );
        assert!(
            serves_form("shell_kill", ""),
            "a cancel whose run is unknown is still a served form"
        );
        let ctx = CommandContext {
            argument: "",
            ..ctx
        };
        assert!(
            matches!(
                build_inner_request("shell_kill", "", &ctx),
                Some(CommandEffect::UnreadableArgument)
            ),
            "a cancel that names no run must refuse the call"
        );
    }

    /// The presence panel's project travels in the argument.
    #[test]
    fn the_presence_row_carries_its_project_in_the_argument() {
        let ctx = CommandContext {
            project_id: "scope-project",
            workspace_id: "scope-workspace",
            session_id: Some("scope-session"),
            argument: "panel-project",
            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) =
            build_inner_request("collaborators", "presence", &ctx)
        else {
            panic!("the presence form must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::PresenceSnapshotGet { project_id } if project_id == "panel-project"
            ),
            "the presence row must read the project the argument names"
        );
        let ctx = CommandContext {
            argument: "",
            ..ctx
        };
        assert!(
            matches!(
                build_inner_request("collaborators", "presence", &ctx),
                Some(CommandEffect::UnreadableArgument)
            ),
            "a presence read with no project must refuse the call"
        );
        assert!(
            serves_form("collaborators", "presence"),
            "a presence read with no project is still a served form"
        );
    }

    /// The Collaboration rows carry their identifiers in the argument.
    ///
    /// The turn id and the channel are not in the scope, so the argument is
    /// where they travel. The test pins the encoding, because a frontend
    /// that encoded them differently would address the wrong turn or the
    /// wrong channel.
    #[test]
    fn the_collaboration_rows_carry_their_identifiers_in_the_argument() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "turn-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("stop", "", &ctx) else {
            panic!("stop must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::TurnCancel { session_id, turn_id }
                    if session_id == "session" && turn_id == "turn-1"
            ),
            "the stop row must cancel the named turn of the named session"
        );

        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "channel-1 message-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("chat_action_list", "", &ctx)
        else {
            panic!("chat_action_list must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ChatActionList { channel_id, message_id, limit }
                    if channel_id == "channel-1"
                        && message_id.as_deref() == Some("message-1")
                        && limit.is_none()
            ),
            "the action-list row must address the channel and the message"
        );

        // A channel with no message id lists the channel's own actions.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "channel-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("chat_action_list", "", &ctx)
        else {
            panic!("chat_action_list must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ChatActionList { channel_id, message_id, .. }
                    if channel_id == "channel-1" && message_id.is_none()
            ),
            "an action listing with no message id must list the channel's actions"
        );
    }

    /// The rows whose whole identifier travels as the argument.
    ///
    /// The projection subscription and the background schedule are named by
    /// ids the frontend holds, and nothing else. The test pins the encoding
    /// because a frontend that encoded them differently would unsubscribe
    /// the wrong subscription or delete the wrong schedule.
    #[test]
    fn the_identifier_rows_carry_their_identifier_in_the_argument() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "subscription-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("stop_observing", "", &ctx)
        else {
            panic!("stop_observing must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ProjectionUnsubscribe { subscription_id }
                    if subscription_id.as_str() == "subscription-1"
            ),
            "the unsubscribe row must name the subscription the daemon issued"
        );

        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "schedule-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("task_del", "", &ctx) else {
            panic!("task_del must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ScheduleDelete { schedule_id } if schedule_id == "schedule-1"
            ),
            "the schedule-delete row must name the schedule the frontend resolved"
        );
    }

    /// The policy rows name no session, and the preference rows cannot.
    ///
    /// M007-D, decision shape one. The effective runtime policy is a
    /// **global** reading, so the `policy` row names no session even when
    /// the scope carries one. `approval` and `reasoning` are unit variants:
    /// their requests carry no session field at all.
    #[test]
    fn the_policy_rows_name_no_session() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("policy", "", &ctx) else {
            panic!("policy must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ExecutionPolicyGet { session_id } if session_id.is_none()
            ),
            "the policy row must read the global effective policy, not a session's"
        );

        let Some(CommandEffect::Ready(request)) = build_inner_request("approval", "", &ctx) else {
            panic!("approval must build a request");
        };
        assert!(
            matches!(request.as_ref(), CoreRequest::ApprovalPreferenceGet),
            "the approval row must read the approval preference"
        );

        let Some(CommandEffect::Ready(request)) = build_inner_request("reasoning", "", &ctx) else {
            panic!("reasoning must build a request");
        };
        assert!(
            matches!(request.as_ref(), CoreRequest::ReasoningPreferenceGet),
            "the reasoning row must read the reasoning preference"
        );
    }

    /// The rows that act on a session the **caller** named carry that
    /// session in the argument.
    ///
    /// M007-D, decision shape two. The scope's session is the *attached*
    /// session; the share surface and `/observe` name another one. So the
    /// session id travels as the argument, and a call that names none is
    /// refused with `no_session` rather than silently retargeted at the
    /// attached session.
    #[test]
    fn the_session_named_rows_carry_their_session_in_the_argument() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("attached-session"),
            argument: "named-session",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("share", "", &ctx) else {
            panic!("share must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::SessionShare { session_id } if session_id == "named-session"
            ),
            "the share row must act on the session the caller named"
        );

        let Some(CommandEffect::Ready(request)) = build_inner_request("unshare", "", &ctx) else {
            panic!("unshare must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::SessionUnshare { session_id } if session_id == "named-session"
            ),
            "the unshare row must act on the session the caller named"
        );

        let Some(CommandEffect::Ready(request)) = build_inner_request("export", "", &ctx) else {
            panic!("export must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::SessionExport { session_id } if session_id == "named-session"
            ),
            "the export row must act on the session the caller named"
        );

        let Some(CommandEffect::Ready(request)) = build_inner_request("observe", "", &ctx) else {
            panic!("observe must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ProjectionSubscribe { request }
                    if request.scope_id == "named-session"
            ),
            "the observe row must follow the session the caller named"
        );

        // A call that names no session is refused as such.
        let ctx = CommandContext {
            argument: "",
            ..ctx
        };
        for command_id in ["share", "unshare", "export", "observe"] {
            assert!(
                matches!(
                    build_inner_request(command_id, "", &ctx),
                    Some(CommandEffect::NeedsSession)
                ),
                "{command_id} acts on a session and must refuse when the caller names none"
            );
        }
    }

    /// The network row carries the intent and the revision the caller
    /// holds.
    ///
    /// The revision is the compare-and-swap the policy read published.
    /// Dropping it would turn a checked write into an unchecked one, so
    /// the encoding is pinned: the intent first, the revision second.
    #[test]
    fn the_network_row_carries_the_intent_and_the_revision() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "offline 7",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("network", "", &ctx) else {
            panic!("network must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::NetworkIntentSet { network_intent, expected_revision }
                    if network_intent == "offline" && *expected_revision == Some(7)
            ),
            "the network row must set the named intent against the revision the caller holds"
        );

        // An intent with no revision is still a write; it just carries no
        // compare-and-swap.
        let ctx = CommandContext {
            argument: "offline",
            ..ctx
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("network", "", &ctx) else {
            panic!("network must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::NetworkIntentSet { network_intent, expected_revision }
                    if network_intent == "offline" && expected_revision.is_none()
            ),
            "an intent with no revision must carry no compare-and-swap"
        );

        // A revision that is not a number is the argument's own refusal.
        let ctx = CommandContext {
            argument: "offline later",
            ..ctx
        };
        assert!(
            matches!(
                build_inner_request("network", "", &ctx),
                Some(CommandEffect::UnreadableArgument)
            ),
            "a revision the daemon cannot read must be its own outcome"
        );
    }

    /// The task rows carry the listing bound in the argument.
    ///
    /// The bound is how many work orders the caller wants to see, so it
    /// belongs to the caller; the daemon does not pick one for it.
    #[test]
    fn the_task_rows_carry_their_bound_in_the_argument() {
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "200",

            authority: None,
            turn_id: None,
        };
        for command_id in ["tasks", "task"] {
            let Some(CommandEffect::Ready(request)) = build_inner_request(command_id, "", &ctx)
            else {
                panic!("{command_id} must build a request");
            };
            assert!(
                matches!(
                    request.as_ref(),
                    CoreRequest::WorkOrderList { limit, .. } if *limit == Some(200)
                ),
                "the {command_id} row must list as many work orders as the caller asked for"
            );
        }

        // A bound that is not a number is the argument's own refusal.
        let ctx = CommandContext {
            argument: "many",
            ..ctx
        };
        for command_id in ["tasks", "task"] {
            assert!(
                matches!(
                    build_inner_request(command_id, "", &ctx),
                    Some(CommandEffect::UnreadableArgument)
                ),
                "a bound the daemon cannot read must be its own outcome"
            );
        }
    }

    /// The rows whose argument carries an identifier and a cursor.
    ///
    /// Each encoding is pinned: a frontend that encoded the cursor
    /// differently would sync from the wrong place, mark the wrong sequence
    /// read, or resume the wrong attachment.
    #[test]
    fn the_cursor_rows_carry_their_identifier_and_cursor_in_the_argument() {
        // The newest window of one channel: the channel alone.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "channel-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("chat_history", "", &ctx)
        else {
            panic!("chat_history must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ChatHistoryOlder { channel_id, before_seq, limit }
                    if channel_id == "channel-1" && before_seq.is_none() && limit.is_none()
            ),
            "the chat-history row must ask for the unanchored newest window of the channel"
        );

        // The incremental sync: the channel and the cursor.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "channel-1 42",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("chat_sync", "", &ctx) else {
            panic!("chat_sync must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ChatSync { channel_id, from_seq, limit }
                    if channel_id == "channel-1" && *from_seq == 42 && limit.is_none()
            ),
            "the chat-sync row must sync the channel from the cursor the frontend holds"
        );

        // The read marker: the channel and the sequence.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "channel-1 42",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("chat_read", "", &ctx) else {
            panic!("chat_read must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::ChatReadSet { channel_id, last_read_seq }
                    if channel_id == "channel-1" && *last_read_seq == 42
            ),
            "the chat-read row must mark the named sequence of the channel read"
        );

        // The attach: the handle, and the cursor when the frontend has one.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "handle-1 42",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("terminal_attach", "", &ctx)
        else {
            panic!("terminal_attach must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::InteractiveProcessAttach { handle, from_seq, max_bytes }
                    if handle == "handle-1" && *from_seq == Some(42) && max_bytes.is_none()
            ),
            "the attach row must resume the handle from the cursor the frontend holds"
        );

        // An attach with no cursor starts at the oldest retained output.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "handle-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("terminal_attach", "", &ctx)
        else {
            panic!("terminal_attach must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::InteractiveProcessAttach { handle, from_seq, .. }
                    if handle == "handle-1" && from_seq.is_none()
            ),
            "an attach with no cursor must start at the oldest retained output"
        );

        // The resume: the attachment and the cursor.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "attachment-1 42",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("terminal_resume", "", &ctx)
        else {
            panic!("terminal_resume must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::InteractiveProcessResume { attachment_id, from_seq, max_bytes }
                    if attachment_id == "attachment-1" && *from_seq == 42 && max_bytes.is_none()
            ),
            "the resume row must resume the attachment from the cursor the frontend holds"
        );

        // The job wait: the job id, and the bound when the frontend asked
        // for one.
        let ctx = CommandContext {
            project_id: "project",
            workspace_id: "workspace",
            session_id: Some("session"),
            argument: "job-1",

            authority: None,
            turn_id: None,
        };
        let Some(CommandEffect::Ready(request)) = build_inner_request("test", "", &ctx) else {
            panic!("test must build a request");
        };
        assert!(
            matches!(
                request.as_ref(),
                CoreRequest::JobWait { job_id, timeout_ms }
                    if job_id == "job-1" && timeout_ms.is_none()
            ),
            "the job-wait row must wait for the named job"
        );
    }
}
