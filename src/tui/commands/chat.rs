//! Project chat commands (Project Collaboration M002).
//!
//! The TUI owns no chat truth. Every operation performs an explicit
//! `chat.v1` round-trip through the daemon-owned `CoreClient` (M001
//! contract): capability negotiation, then channel ensure / history /
//! sync / send / edit / redact / read-marker / composing. Results land
//! back on the event loop as `TuiCommand::Chat*` completions guarded by
//! per-project request id + reconnect epoch, so stale completions (after
//! a tab switch, stop, or reconnect) are dropped at apply time.
//!
//! Concurrency: `start_*` bumps the reducer generation via
//! `ChatState::begin_ensure` / `begin_history` and `apply_*` drops stale
//! completions. Sends carry an idempotency key and merge by `message_id`,
//! so duplicate deliveries converge without duplication. Hints
//! (`ChatMessageCommitted` / `ChatComposingUpdated`) only flag
//! `needs_resync`; they never spawn a task per update — the next explicit
//! refresh (panel open, user action, or reconnect) performs the single
//! re-fetch. Live full-message events apply directly when they target the
//! cached active channel.
//!
//! Security: principals come from transport authority daemon-side; this
//! module never invents authorship and never infers permission
//! client-side. Daemon denials (`project_not_found`) render cleanly as
//! the generic unavailable panel. Free text sent here has no execution
//! semantics (M003 owns structured actions separately).

use crate::protocol::core::{ChatChannelDto, ChatMessageDto, CoreRequest, CoreResponse};
use crate::tui::app::state::chat::extract_mentions;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Maximum body bytes accepted by the composer (matches the daemon
/// bound; oversized input fails fast with a typed error and the draft
/// retained instead of a wasted round-trip).
pub const CHAT_COMPOSER_MAX_BYTES: usize = 8192;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn unauthorized_of(code: &str) -> bool {
    code == "project_not_found"
}

/// Capability negotiation shared by every chat task. Returns `true` when
/// the daemon advertises `chat.v1`.
async fn negotiate_chat(core_client: &std::sync::Arc<dyn crate::core::CoreClient>) -> bool {
    let cap_request = crate::core::new_request(
        format!("chat-capabilities-{}", uuid::Uuid::new_v4()),
        CoreRequest::ChatCapabilities,
    );
    match core_client.request(cap_request).await {
        Ok(CoreResponse::ChatCapabilities { capabilities }) => capabilities.supported,
        _ => false,
    }
}

async fn ensure_default_channel(
    core_client: &std::sync::Arc<dyn crate::core::CoreClient>,
    project_id: &str,
) -> Result<ChatChannelDto, (Option<String>, bool)> {
    let req = crate::core::new_request(
        format!("chat-ensure-{}", uuid::Uuid::new_v4()),
        CoreRequest::ChatChannelEnsure {
            project_id: project_id.to_string(),
            name: None,
        },
    );
    match core_client.request(req).await {
        Ok(CoreResponse::ChatChannel { channel }) => Ok(channel),
        Ok(CoreResponse::Error { code, message }) => {
            Err((Some(format!("{code}: {message}")), unauthorized_of(&code)))
        }
        Ok(other) => Err((Some(format!("Unexpected core response: {other:?}")), false)),
        Err(e) => Err((Some(format!("Chat channel ensure failed: {e}")), false)),
    }
}

/// Resolve the send/history channel for `project_id`: the cached active
/// channel when present.
fn active_channel_for(app: &App, project_id: &str) -> Option<String> {
    app.chat
        .get(project_id)
        .and_then(|e| e.active_channel_id.clone())
}

/// Refresh the open chat panel (when one is showing) from the reducer so
/// completions and live events update the visible window without a new
/// fetch.
fn refresh_chat_panel(app: &mut App) {
    let showing_chat = app
        .dialog_state
        .info_dialog
        .as_ref()
        .map(|d| d.info_type() == crate::tui::components::dialogs::info::InfoType::ProjectChat)
        .unwrap_or(false);
    if !showing_chat {
        return;
    }
    if let Some(project_id) = app.chat_panel_project.clone() {
        let lines = app.chat.panel_lines(&project_id, now_ms());
        if let Some(dialog) = app.dialog_state.info_dialog.as_mut() {
            dialog.set_content(lines);
        }
    }
}

// ── History / sync ───────────────────────────────────────────────────────

/// Fetch the bounded history window for `project_id`. Resolves the
/// channel from the cache, ensuring the default channel first when none
/// is known (single task, no polling loop).
pub(crate) fn start_chat_history(app: &mut App, project_id: String) {
    if project_id.is_empty() {
        return;
    }
    if app.core_client.is_none() {
        let Some(request_id) = app.chat.begin_ensure(&project_id) else {
            return;
        };
        let epoch = app.chat.reconnect_epoch;
        apply_chat_history_loaded(
            app,
            request_id,
            project_id,
            String::new(),
            Vec::new(),
            0,
            false,
            0,
            Some("Core unavailable — check daemon status with /doctor".to_string()),
            false,
            true,
            epoch,
        );
        return;
    }
    // begin_history needs a channel; use a placeholder binding when the
    // channel is not cached yet — the task ensures the channel first and
    // the completion carries the real channel id. Bind the request to a
    // dummy channel only to mark loading; the apply path re-keys on the
    // completion's channel id.
    let known_channel = active_channel_for(app, &project_id);
    let bind_channel = known_channel.clone().unwrap_or_default();
    let request_id = if bind_channel.is_empty() {
        app.chat.begin_ensure(&project_id)
    } else {
        app.chat.begin_history(&project_id, &bind_channel)
    };
    let Some(request_id) = request_id else {
        app.messages_state
            .toasts
            .warning("Invalid project locator — chat history not loaded");
        return;
    };
    let reconnect_epoch = app.chat.reconnect_epoch;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let pid = project_id.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_history",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ChatHistoryLoaded {
                    request_id,
                    project_id: pid,
                    channel_id: String::new(),
                    messages: Vec::new(),
                    next_cursor: 0,
                    truncated: false,
                    retention_floor_seq: 0,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            };
            if !negotiate_chat(&core_client).await {
                return Some(TuiCommand::ChatHistoryLoaded {
                    request_id,
                    project_id: pid,
                    channel_id: String::new(),
                    messages: Vec::new(),
                    next_cursor: 0,
                    truncated: false,
                    retention_floor_seq: 0,
                    error: None,
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            }
            let channel_id = match known_channel {
                Some(id) => id,
                None => match ensure_default_channel(&core_client, &pid).await {
                    Ok(channel) => channel.channel_id,
                    Err((error, unauthorized)) => {
                        return Some(TuiCommand::ChatHistoryLoaded {
                            request_id,
                            project_id: pid,
                            channel_id: String::new(),
                            messages: Vec::new(),
                            next_cursor: 0,
                            truncated: false,
                            retention_floor_seq: 0,
                            error,
                            unauthorized,
                            unsupported: false,
                            reconnect_epoch,
                        });
                    }
                },
            };
            let req = crate::core::new_request(
                format!("chat-history-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatHistory {
                    channel_id: channel_id.clone(),
                    from_seq: None,
                    limit: None,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatHistory {
                    messages,
                    next_cursor,
                    truncated,
                    retention_floor_seq,
                    ..
                }) => Some(TuiCommand::ChatHistoryLoaded {
                    request_id,
                    project_id: pid,
                    channel_id,
                    messages,
                    next_cursor,
                    truncated,
                    retention_floor_seq,
                    error: None,
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::ChatHistoryLoaded {
                    request_id,
                    project_id: pid,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: 0,
                    truncated: false,
                    retention_floor_seq: 0,
                    error: Some(format!("{code}: {message}")),
                    unauthorized: unauthorized_of(&code),
                    unsupported: false,
                    reconnect_epoch,
                }),
                Ok(other) => Some(TuiCommand::ChatHistoryLoaded {
                    request_id,
                    project_id: pid,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: 0,
                    truncated: false,
                    retention_floor_seq: 0,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ChatHistoryLoaded {
                    request_id,
                    project_id: pid,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: 0,
                    truncated: false,
                    retention_floor_seq: 0,
                    error: Some(format!("Chat history request failed: {e}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `ChatHistoryLoaded` completion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chat_history_loaded(
    app: &mut App,
    request_id: u64,
    project_id: String,
    channel_id: String,
    messages: Vec<ChatMessageDto>,
    next_cursor: u64,
    truncated: bool,
    retention_floor_seq: u64,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if unsupported {
        app.chat.set_capability(false);
        let _ = app.chat.apply_error(
            request_id,
            &project_id,
            String::new(),
            false,
            true,
            reconnect_epoch,
        );
        refresh_chat_panel(app);
        return;
    }
    if unauthorized {
        let _ = app.chat.apply_error(
            request_id,
            &project_id,
            "project_not_found: denied".to_string(),
            true,
            false,
            reconnect_epoch,
        );
        refresh_chat_panel(app);
        return;
    }
    if !channel_id.is_empty() && error.is_none() {
        if app.chat.capability_supported.is_none() {
            app.chat.set_capability(true);
        }
        let _ = app.chat.apply_history(
            request_id,
            &project_id,
            &channel_id,
            messages,
            next_cursor,
            truncated,
            retention_floor_seq,
            reconnect_epoch,
        );
        refresh_chat_panel(app);
        return;
    }
    if let Some(err) = error {
        // Empty-channel ensure failures carry the channel error already.
        if !channel_id.is_empty() || !err.is_empty() {
            let _ = app.chat.apply_error(
                request_id,
                &project_id,
                err.clone(),
                false,
                false,
                reconnect_epoch,
            );
            app.messages_state
                .toasts
                .warning(&format!("Chat history failed: {err}"));
        }
        refresh_chat_panel(app);
    }
}

/// Incremental sync from the cached cursor (`next_cursor`). Resumes the
/// M001 cursor on reconnect; an expired cursor receives a bounded resync
/// page with `resync_required` set.
pub(crate) fn start_chat_sync(app: &mut App, project_id: String) {
    let Some(channel_id) = active_channel_for(app, &project_id) else {
        // No channel cached yet: a full history fetch (with ensure)
        // converges instead of failing.
        start_chat_history(app, project_id);
        return;
    };
    let from_seq = app
        .chat
        .get(&project_id)
        .map(|e| e.next_cursor)
        .unwrap_or(0);
    if app.core_client.is_none() {
        return;
    }
    // Coalesce while a fetch is in flight.
    if !app.chat.needs_refresh(&project_id) {
        // Explicit user sync still proceeds: bump a fresh request.
        let _ = from_seq;
    }
    let Some(request_id) = app.chat.begin_history(&project_id, &channel_id) else {
        return;
    };
    let reconnect_epoch = app.chat.reconnect_epoch;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_sync",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ChatSyncLoaded {
                    request_id,
                    project_id,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: from_seq,
                    resync_required: false,
                    retention_floor_seq: 0,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            };
            if !negotiate_chat(&core_client).await {
                return Some(TuiCommand::ChatSyncLoaded {
                    request_id,
                    project_id,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: from_seq,
                    resync_required: false,
                    retention_floor_seq: 0,
                    error: None,
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            }
            let req = crate::core::new_request(
                format!("chat-sync-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatSync {
                    channel_id: channel_id.clone(),
                    from_seq,
                    limit: None,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatSync {
                    messages,
                    next_cursor,
                    resync_required,
                    retention_floor_seq,
                    ..
                }) => Some(TuiCommand::ChatSyncLoaded {
                    request_id,
                    project_id,
                    channel_id,
                    messages,
                    next_cursor,
                    resync_required,
                    retention_floor_seq,
                    error: None,
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::ChatSyncLoaded {
                    request_id,
                    project_id,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: from_seq,
                    resync_required: false,
                    retention_floor_seq: 0,
                    error: Some(format!("{code}: {message}")),
                    unauthorized: unauthorized_of(&code),
                    unsupported: false,
                    reconnect_epoch,
                }),
                Ok(other) => Some(TuiCommand::ChatSyncLoaded {
                    request_id,
                    project_id,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: from_seq,
                    resync_required: false,
                    retention_floor_seq: 0,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ChatSyncLoaded {
                    request_id,
                    project_id,
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: from_seq,
                    resync_required: false,
                    retention_floor_seq: 0,
                    error: Some(format!("Chat sync request failed: {e}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `ChatSyncLoaded` completion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chat_sync_loaded(
    app: &mut App,
    request_id: u64,
    project_id: String,
    channel_id: String,
    messages: Vec<ChatMessageDto>,
    next_cursor: u64,
    resync_required: bool,
    retention_floor_seq: u64,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if unsupported {
        app.chat.set_capability(false);
        let _ = app.chat.apply_error(
            request_id,
            &project_id,
            String::new(),
            false,
            true,
            reconnect_epoch,
        );
        refresh_chat_panel(app);
        return;
    }
    if unauthorized {
        let _ = app.chat.apply_error(
            request_id,
            &project_id,
            "project_not_found: denied".to_string(),
            true,
            false,
            reconnect_epoch,
        );
        refresh_chat_panel(app);
        return;
    }
    if error.is_none() {
        if app.chat.capability_supported.is_none() {
            app.chat.set_capability(true);
        }
        let _ = app.chat.apply_sync(
            request_id,
            &project_id,
            &channel_id,
            messages,
            next_cursor,
            resync_required,
            retention_floor_seq,
            reconnect_epoch,
        );
        refresh_chat_panel(app);
        return;
    }
    if let Some(err) = error {
        let _ = app.chat.apply_error(
            request_id,
            &project_id,
            err.clone(),
            false,
            false,
            reconnect_epoch,
        );
        app.messages_state
            .toasts
            .warning(&format!("Chat sync failed: {err}"));
        refresh_chat_panel(app);
    }
}

// ── Send ─────────────────────────────────────────────────────────────────

/// Send one message to the project's active channel (ensuring the default
/// channel first when none is cached). Mentions are extracted from the
/// body as a client-side convenience; the daemon re-validates. Free text
/// has no execution semantics — this issues only `Chat*` requests.
pub(crate) fn start_chat_send(
    app: &mut App,
    project_id: String,
    body: String,
    reply_to: Option<String>,
) {
    let trimmed = body.trim().to_string();
    if trimmed.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /chat-send <text> — message body is empty");
        return;
    }
    if trimmed.len() > CHAT_COMPOSER_MAX_BYTES {
        app.chat.note_failed_send(
            &project_id,
            trimmed.clone(),
            "chat_body_too_large: message exceeds 8 KiB".to_string(),
        );
        app.messages_state
            .toasts
            .warning("Message exceeds 8 KiB and was not sent — draft retained; shorten and retry");
        refresh_chat_panel(app);
        return;
    }
    if let Some(reply) = reply_to.as_deref() {
        if reply.is_empty()
            || reply.len() > crate::tui::app::state::chat::MAX_CHAT_LOCATOR_LEN
            || reply.bytes().any(|b| b == 0 || b.is_ascii_control())
        {
            app.messages_state
                .toasts
                .warning("Invalid reply target — message not sent");
            return;
        }
    }
    let mentions = extract_mentions(&trimmed);
    let known_channel = active_channel_for(app, &project_id);
    let reconnect_epoch = app.chat.reconnect_epoch;
    // Sends do not mark the window loading (no task-per-message spinner);
    // stale protection comes from the reconnect epoch plus idempotent
    // merge by message id. Use a request id only for tracing.
    let request_id = reconnect_epoch.saturating_add(1);
    let idempotency_key = uuid::Uuid::new_v4().to_string();
    if app.core_client.is_none() {
        app.chat.note_failed_send(
            &project_id,
            trimmed.clone(),
            "Core unavailable — check daemon status with /doctor".to_string(),
        );
        app.messages_state
            .toasts
            .warning("Chat send failed: core unavailable — draft retained");
        refresh_chat_panel(app);
        return;
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let pid = project_id.clone();
    let draft = trimmed.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_send",
        async move {
            let finish = |message: Option<ChatMessageDto>,
                          duplicate: bool,
                          channel_id: String,
                          error: Option<String>,
                          unauthorized: bool,
                          unsupported: bool| {
                TuiCommand::ChatMessageSent {
                    request_id,
                    project_id: pid.clone(),
                    channel_id,
                    message,
                    duplicate,
                    draft: draft.clone(),
                    error,
                    unauthorized,
                    unsupported,
                    reconnect_epoch,
                }
            };
            let Some(core_client) = core_client else {
                return Some(finish(
                    None,
                    false,
                    String::new(),
                    Some("Core unavailable".to_string()),
                    false,
                    true,
                ));
            };
            if !negotiate_chat(&core_client).await {
                return Some(finish(None, false, String::new(), None, false, true));
            }
            let channel_id = match known_channel {
                Some(id) => id,
                None => match ensure_default_channel(&core_client, &pid).await {
                    Ok(channel) => channel.channel_id,
                    Err((error, unauthorized)) => {
                        return Some(finish(
                            None,
                            false,
                            String::new(),
                            error,
                            unauthorized,
                            false,
                        ));
                    }
                },
            };
            let req = crate::core::new_request(
                format!("chat-send-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatSend {
                    channel_id: channel_id.clone(),
                    body: trimmed.clone(),
                    reply_to: reply_to.clone(),
                    thread_root: None,
                    mentions,
                    references: Vec::new(),
                    idempotency_key: Some(idempotency_key),
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatMessage { message, duplicate }) => Some(finish(
                    Some(message),
                    duplicate,
                    channel_id,
                    None,
                    false,
                    false,
                )),
                Ok(CoreResponse::Error { code, message }) => Some(finish(
                    None,
                    false,
                    channel_id,
                    Some(format!("{code}: {message}")),
                    unauthorized_of(&code),
                    false,
                )),
                Ok(other) => Some(finish(
                    None,
                    false,
                    channel_id,
                    Some(format!("Unexpected core response: {other:?}")),
                    false,
                    false,
                )),
                Err(e) => Some(finish(
                    None,
                    false,
                    channel_id,
                    Some(format!("Chat send failed: {e}")),
                    false,
                    false,
                )),
            }
        },
    );
}

/// Apply a `ChatMessageSent` completion. Failed sends retain the editable
/// draft with the typed error and fabricate nothing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chat_message_sent(
    app: &mut App,
    project_id: String,
    channel_id: String,
    message: Option<ChatMessageDto>,
    duplicate: bool,
    draft: String,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if unsupported {
        app.chat.set_capability(false);
        app.chat.note_failed_send(
            &project_id,
            draft,
            "Chat unavailable — daemon does not support project chat".to_string(),
        );
        app.messages_state
            .toasts
            .warning("Chat unavailable — daemon does not support project chat; draft retained");
        refresh_chat_panel(app);
        return;
    }
    if unauthorized {
        app.chat.note_failed_send(
            &project_id,
            draft,
            "project_not_found: not authorized for project chat".to_string(),
        );
        // Privacy-preserving: identical message for denied vs absent.
        app.messages_state
            .toasts
            .warning("Chat unavailable for this project — draft retained");
        refresh_chat_panel(app);
        return;
    }
    if let Some(message) = message {
        if app.chat.capability_supported.is_none() {
            app.chat.set_capability(true);
        }
        let channel = if channel_id.is_empty() {
            message.channel_id.clone()
        } else {
            channel_id.clone()
        };
        if app
            .chat
            .apply_sent(&project_id, &channel, &message, reconnect_epoch)
            && !duplicate
        {
            app.messages_state
                .toasts
                .info(&format!("Sent to chat (#{})", message.seq));
        }
        refresh_chat_panel(app);
        return;
    }
    if let Some(err) = error {
        app.chat.note_failed_send(&project_id, draft, err.clone());
        app.messages_state
            .toasts
            .warning(&format!("Chat send failed: {err} — draft retained"));
        refresh_chat_panel(app);
    }
}

// ── Edit / redact ────────────────────────────────────────────────────────

/// Edit one message body. The expected revision comes from the local
/// window (authoritative conflict detection stays daemon-side); unknown
/// messages fail fast with guidance instead of guessing a revision.
pub(crate) fn start_chat_edit(
    app: &mut App,
    project_id: String,
    message_id: String,
    new_body: String,
) {
    let trimmed = new_body.trim().to_string();
    if trimmed.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /chat-edit <message-id> <new-text>");
        return;
    }
    if trimmed.len() > CHAT_COMPOSER_MAX_BYTES {
        app.messages_state
            .toasts
            .warning("Edited body exceeds 8 KiB — edit not sent");
        return;
    }
    let Some(channel_id) = active_channel_for(app, &project_id) else {
        app.messages_state
            .toasts
            .warning("No chat channel cached — open /chat first so the message window loads");
        return;
    };
    let Some(expected_revision) = app.chat.get(&project_id).and_then(|e| {
        e.messages
            .iter()
            .find(|m| m.message_id == message_id)
            .map(|m| m.revision)
    }) else {
        app.messages_state
            .toasts
            .warning("Message not in the local window — run /chat-history first, then retry");
        return;
    };
    if app.core_client.is_none() {
        app.messages_state
            .toasts
            .warning("Chat edit failed: core unavailable");
        return;
    }
    let reconnect_epoch = app.chat.reconnect_epoch;
    let request_id = reconnect_epoch.saturating_add(1);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_edit",
        async move {
            let finish = |message: Option<ChatMessageDto>,
                          error: Option<String>,
                          unauthorized: bool,
                          unsupported: bool| {
                TuiCommand::ChatEditFinished {
                    request_id,
                    project_id: project_id.clone(),
                    channel_id: channel_id.clone(),
                    message,
                    message_id: message_id.clone(),
                    error,
                    unauthorized,
                    unsupported,
                    reconnect_epoch,
                }
            };
            let Some(core_client) = core_client else {
                return Some(finish(
                    None,
                    Some("Core unavailable".to_string()),
                    false,
                    true,
                ));
            };
            if !negotiate_chat(&core_client).await {
                return Some(finish(None, None, false, true));
            }
            let req = crate::core::new_request(
                format!("chat-edit-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatEdit {
                    channel_id: channel_id.clone(),
                    message_id: message_id.clone(),
                    expected_revision,
                    new_body: trimmed.clone(),
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatMessage { message, .. }) => {
                    Some(finish(Some(message), None, false, false))
                }
                Ok(CoreResponse::Error { code, message }) => Some(finish(
                    None,
                    Some(format!("{code}: {message}")),
                    unauthorized_of(&code),
                    false,
                )),
                Ok(other) => Some(finish(
                    None,
                    Some(format!("Unexpected core response: {other:?}")),
                    false,
                    false,
                )),
                Err(e) => Some(finish(
                    None,
                    Some(format!("Chat edit failed: {e}")),
                    false,
                    false,
                )),
            }
        },
    );
}

/// Apply a `ChatEditFinished` completion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chat_edit_finished(
    app: &mut App,
    project_id: String,
    channel_id: String,
    message: Option<ChatMessageDto>,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    _reconnect_epoch: u64,
) {
    if unsupported || unauthorized {
        app.messages_state
            .toasts
            .warning("Chat edit unavailable for this project");
        return;
    }
    if let Some(message) = message {
        if app.chat.apply_event_edited(&message) {
            app.messages_state
                .toasts
                .info(&format!("Edited message (r{})", message.revision));
        }
        refresh_chat_panel(app);
        let _ = (project_id, channel_id);
        return;
    }
    if let Some(err) = error {
        app.messages_state
            .toasts
            .warning(&format!("Chat edit failed: {err}"));
    }
}

/// Redact one message body to `[REDACTED]` (author-only daemon-side;
/// prior revisions survive as append-only history).
pub(crate) fn start_chat_redact(
    app: &mut App,
    project_id: String,
    message_id: String,
    reason: Option<String>,
) {
    let Some(channel_id) = active_channel_for(app, &project_id) else {
        app.messages_state
            .toasts
            .warning("No chat channel cached — open /chat first so the message window loads");
        return;
    };
    let expected_revision = app.chat.get(&project_id).and_then(|e| {
        e.messages
            .iter()
            .find(|m| m.message_id == message_id)
            .map(|m| m.revision)
    });
    if app.core_client.is_none() {
        app.messages_state
            .toasts
            .warning("Chat redact failed: core unavailable");
        return;
    }
    let reconnect_epoch = app.chat.reconnect_epoch;
    let request_id = reconnect_epoch.saturating_add(1);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_redact",
        async move {
            let finish =
                |revision: u64, error: Option<String>, unauthorized: bool, unsupported: bool| {
                    TuiCommand::ChatRedactFinished {
                        request_id,
                        project_id: project_id.clone(),
                        channel_id: channel_id.clone(),
                        message_id: message_id.clone(),
                        revision,
                        error,
                        unauthorized,
                        unsupported,
                        reconnect_epoch,
                    }
                };
            let Some(core_client) = core_client else {
                return Some(finish(0, Some("Core unavailable".to_string()), false, true));
            };
            if !negotiate_chat(&core_client).await {
                return Some(finish(0, None, false, true));
            }
            let req = crate::core::new_request(
                format!("chat-redact-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatRedact {
                    channel_id: channel_id.clone(),
                    message_id: message_id.clone(),
                    expected_revision,
                    reason: reason.clone(),
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatMessage { message, .. }) => {
                    Some(finish(message.revision, None, false, false))
                }
                Ok(CoreResponse::Error { code, message }) => Some(finish(
                    0,
                    Some(format!("{code}: {message}")),
                    unauthorized_of(&code),
                    false,
                )),
                Ok(other) => Some(finish(
                    0,
                    Some(format!("Unexpected core response: {other:?}")),
                    false,
                    false,
                )),
                Err(e) => Some(finish(
                    0,
                    Some(format!("Chat redact failed: {e}")),
                    false,
                    false,
                )),
            }
        },
    );
}

/// Apply a `ChatRedactFinished` completion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chat_redact_finished(
    app: &mut App,
    project_id: String,
    channel_id: String,
    message_id: String,
    revision: u64,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    _reconnect_epoch: u64,
) {
    if unsupported || unauthorized {
        app.messages_state
            .toasts
            .warning("Chat redact unavailable for this project");
        return;
    }
    if error.is_none() {
        if app
            .chat
            .apply_event_redacted(&project_id, &channel_id, &message_id, revision)
        {
            app.messages_state.toasts.info("Message redacted");
        }
        refresh_chat_panel(app);
        return;
    }
    if let Some(err) = error {
        app.messages_state
            .toasts
            .warning(&format!("Chat redact failed: {err}"));
    }
}

// ── Read markers / composing ─────────────────────────────────────────────

/// Advance the caller's read marker to the newest retained message
/// (forward-only daemon-side).
pub(crate) fn start_chat_read(app: &mut App, project_id: String) {
    let (channel_id, last_seq) = match app.chat.get(&project_id) {
        Some(entry) => match entry.active_channel_id.clone() {
            Some(channel) => {
                let newest = entry.messages.back().map(|m| m.seq).unwrap_or(0);
                (channel, newest)
            }
            None => {
                app.messages_state
                    .toasts
                    .info("No chat channel cached — open /chat first");
                return;
            }
        },
        None => {
            app.messages_state
                .toasts
                .info("No chat loaded for this project — open /chat first");
            return;
        }
    };
    if app.core_client.is_none() {
        // Local-only fallback still clears the badge optimistically;
        // the daemon marker converges on the next authorized fetch.
        app.chat
            .apply_read_marker(&project_id, &channel_id, last_seq);
        refresh_chat_panel(app);
        return;
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let pid = project_id.clone();
    let cid = channel_id.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_read",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ChatReadMarkerSet {
                    project_id: pid,
                    channel_id: cid,
                    last_read_seq: last_seq,
                    error: Some("Core unavailable".to_string()),
                });
            };
            let req = crate::core::new_request(
                format!("chat-read-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatReadSet {
                    channel_id: cid.clone(),
                    last_read_seq: last_seq,
                },
            );
            match core_client.request(req).await {
                Ok(CoreResponse::ChatReadMarker { last_read_seq, .. }) => {
                    Some(TuiCommand::ChatReadMarkerSet {
                        project_id: pid,
                        channel_id: cid,
                        last_read_seq,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::ChatReadMarkerSet {
                    project_id: pid,
                    channel_id: cid,
                    last_read_seq: last_seq,
                    error: Some(format!("{code}: {message}")),
                }),
                Ok(_) => None,
                Err(e) => Some(TuiCommand::ChatReadMarkerSet {
                    project_id: pid,
                    channel_id: cid,
                    last_read_seq: last_seq,
                    error: Some(format!("Chat read marker failed: {e}")),
                }),
            }
        },
    );
}

/// Apply a `ChatReadMarkerSet` completion (forward-only; failures keep
/// the local badge so unread state never silently clears).
pub(crate) fn apply_chat_read_marker_set(
    app: &mut App,
    project_id: String,
    channel_id: String,
    last_read_seq: u64,
    error: Option<String>,
) {
    if error.is_none() {
        app.chat
            .apply_read_marker(&project_id, &channel_id, last_read_seq);
        refresh_chat_panel(app);
    } else if let Some(err) = error {
        // Privacy-preserving: denials and transport errors share one
        // shape; the badge is retained either way.
        app.messages_state
            .toasts
            .warning(&format!("Chat read marker not saved: {err}"));
    }
}

/// Set or clear the caller's ephemeral composing lease (content-free).
pub(crate) fn start_chat_composing(app: &mut App, project_id: String, composing: bool) {
    let Some(channel_id) = active_channel_for(app, &project_id) else {
        app.messages_state
            .toasts
            .warning("No chat channel cached — open /chat first so the channel resolves");
        return;
    };
    if app.core_client.is_none() {
        app.messages_state
            .toasts
            .warning("Chat composing failed: core unavailable");
        return;
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_composing",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ChatComposingLoaded {
                    request_id: 0,
                    project_id: project_id.clone(),
                    channel_id: channel_id.clone(),
                    composing: Vec::new(),
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch: 0,
                });
            };
            let set_req = crate::core::new_request(
                format!("chat-composing-set-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatComposingSet {
                    channel_id: channel_id.clone(),
                    composing,
                },
            );
            if let Ok(CoreResponse::Error { code, message }) = core_client.request(set_req).await {
                let unauthorized = unauthorized_of(&code);
                return Some(TuiCommand::ChatComposingLoaded {
                    request_id: 0,
                    project_id: project_id.clone(),
                    channel_id: channel_id.clone(),
                    composing: Vec::new(),
                    error: Some(format!("{code}: {message}")),
                    unauthorized,
                    unsupported: false,
                    reconnect_epoch: 0,
                });
            }
            let list_req = crate::core::new_request(
                format!("chat-composing-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatComposingList {
                    channel_id: channel_id.clone(),
                },
            );
            match core_client.request(list_req).await {
                Ok(CoreResponse::ChatComposing { composing, .. }) => {
                    Some(TuiCommand::ChatComposingLoaded {
                        request_id: 0,
                        project_id: project_id.clone(),
                        channel_id: channel_id.clone(),
                        composing,
                        error: None,
                        unauthorized: false,
                        unsupported: false,
                        reconnect_epoch: 0,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ChatComposingLoaded {
                        request_id: 0,
                        project_id: project_id.clone(),
                        channel_id: channel_id.clone(),
                        composing: Vec::new(),
                        error: Some(format!("{code}: {message}")),
                        unauthorized: unauthorized_of(&code),
                        unsupported: false,
                        reconnect_epoch: 0,
                    })
                }
                Ok(_) => None,
                Err(e) => Some(TuiCommand::ChatComposingLoaded {
                    request_id: 0,
                    project_id: project_id.clone(),
                    channel_id: channel_id.clone(),
                    composing: Vec::new(),
                    error: Some(format!("Chat composing request failed: {e}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch: 0,
                }),
            }
        },
    );
}

/// Apply a `ChatComposingLoaded` completion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chat_composing_loaded(
    app: &mut App,
    project_id: String,
    channel_id: String,
    composing: Vec<crate::protocol::core::ChatComposingDto>,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    _reconnect_epoch: u64,
) {
    if unsupported || unauthorized || error.is_some() {
        return;
    }
    app.chat
        .apply_composing(&project_id, &channel_id, composing);
    refresh_chat_panel(app);
}

// ── Panel ────────────────────────────────────────────────────────────────

/// Render the project chat panel for the active project. Fetches on
/// demand when the entry is missing or stale, then opens the scrollable
/// dialog with the current (possibly loading) lines. Focus/key behavior
/// follows the standard info-dialog convention (`j`/`k` scroll,
/// `Esc`/`Enter` close); no session state is mutated.
pub(crate) fn show_chat(app: &mut App) {
    let Some(project_id) = app.active_project_id().map(str::to_string) else {
        app.messages_state
            .toasts
            .info("No active project — open a project tab first");
        return;
    };
    if app.chat.needs_refresh(&project_id) {
        start_chat_history(app, project_id.clone());
    }
    app.chat_panel_project = Some(project_id.clone());
    let lines = app.chat.panel_lines(&project_id, now_ms());
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::ProjectChat,
        lines,
    );
}

// ── Observer insert routing ──────────────────────────────────────────────

/// Route observer-mode insert text to project chat.
///
/// This is the M002 collaboration seam: while observing another session,
/// bare insert-mode input (Enter) targets the observed project's chat
/// instead of the read-only session. Only `Chat*` core requests are
/// issued here — turn submit/steer/cancel and permission/question answers
/// never flow through this path (they stay blocked by `ObserverState`).
///
/// Returns `true` when the text was routed (prompt already cleared);
/// `false` when no project is available and the caller should fall back
/// to the read-only placeholder.
pub(crate) fn route_observer_insert_to_chat(app: &mut App, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Prefer the observed session's project (the collaboration context
    // the observer is watching); fall back to the active tab. Both are
    // opaque locators, never paths.
    let project_id = app
        .observer
        .observed_project_id()
        .map(str::to_string)
        .or_else(|| app.active_project_id().map(str::to_string));
    let Some(project_id) = project_id else {
        return false;
    };
    start_chat_send(app, project_id, trimmed.to_string(), None);
    app.prompt_state.prompt.clear();
    app.prompt_state.show_completions = false;
    true
}

// ── Daemon event intake ──────────────────────────────────────────────────

/// Route a daemon `ChatMessageCommitted` payload into the bounded chat
/// reducer. Unknown projects only flag a resync hint (fail closed — no
/// message stored until an authorized fetch confirms it).
pub(crate) fn on_chat_message_committed(app: &mut App, message: ChatMessageDto) {
    if app.chat.apply_event_committed(&message) {
        refresh_chat_panel(app);
    } else if app.chat.note_hint(&message.project_id) {
        let is_active = app.active_project_id() == Some(message.project_id.as_str());
        if is_active {
            start_chat_history(app, message.project_id.clone());
        }
    }
}

/// Route a daemon `ChatMessageEdited` payload into the reducer.
pub(crate) fn on_chat_message_edited(app: &mut App, message: ChatMessageDto) {
    if app.chat.apply_event_edited(&message) {
        refresh_chat_panel(app);
    } else if app.chat.note_hint(&message.project_id) {
        let is_active = app.active_project_id() == Some(message.project_id.as_str());
        if is_active {
            start_chat_history(app, message.project_id.clone());
        }
    }
}

/// Route a daemon `ChatMessageRedacted` hint into the reducer.
pub(crate) fn on_chat_message_redacted(
    app: &mut App,
    project_id: String,
    channel_id: String,
    message_id: String,
    revision: u64,
) {
    if app
        .chat
        .apply_event_redacted(&project_id, &channel_id, &message_id, revision)
    {
        refresh_chat_panel(app);
    } else if app.chat.note_hint(&project_id) {
        let is_active = app.active_project_id() == Some(project_id.as_str());
        if is_active {
            start_chat_history(app, project_id);
        }
    }
}

/// Route a daemon `ChatComposingUpdated` liveness hint into the reducer.
/// Carries no content; the active project re-fetches through the
/// authorized composing-list path only when the chat panel is showing
/// (no polling storm from background typing).
pub(crate) fn on_chat_composing_hint(app: &mut App, project_id: String, channel_id: String) {
    let panel_showing = app
        .dialog_state
        .info_dialog
        .as_ref()
        .map(|d| d.info_type() == crate::tui::components::dialogs::info::InfoType::ProjectChat)
        .unwrap_or(false);
    if !panel_showing {
        app.chat.note_hint(&project_id);
        return;
    }
    if app.chat.note_hint(&project_id) {
        let is_active = app.active_project_id() == Some(project_id.as_str());
        if is_active {
            // Refresh the composing snapshot only; the message window
            // re-fetches on explicit user action.
            let _ = channel_id;
            start_chat_composing_refresh(app, project_id);
        }
    }
}

/// Re-fetch only the composing snapshot for the active channel.
fn start_chat_composing_refresh(app: &mut App, project_id: String) {
    let Some(channel_id) = active_channel_for(app, &project_id) else {
        return;
    };
    if app.core_client.is_none() {
        return;
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "chat_composing_refresh",
        async move {
            let core_client = core_client?;
            let list_req = crate::core::new_request(
                format!("chat-composing-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatComposingList {
                    channel_id: channel_id.clone(),
                },
            );
            match core_client.request(list_req).await {
                Ok(CoreResponse::ChatComposing { composing, .. }) => {
                    Some(TuiCommand::ChatComposingLoaded {
                        request_id: 0,
                        project_id: project_id.clone(),
                        channel_id: channel_id.clone(),
                        composing,
                        error: None,
                        unauthorized: false,
                        unsupported: false,
                        reconnect_epoch: 0,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ChatComposingLoaded {
                        request_id: 0,
                        project_id: project_id.clone(),
                        channel_id: channel_id.clone(),
                        composing: Vec::new(),
                        error: Some(format!("{code}: {message}")),
                        unauthorized: unauthorized_of(&code),
                        unsupported: false,
                        reconnect_epoch: 0,
                    })
                }
                Ok(_) => None,
                Err(_) => None,
            }
        },
    );
}
