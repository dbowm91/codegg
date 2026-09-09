//! Authorized read-only session observation (Presence M003).
//!
//! The TUI owns no observation truth. Every observe/resume/stop performs an
//! explicit projection round-trip through the daemon-owned `CoreClient`
//! (`ProjectionCapabilities` negotiation, then `ProjectionSubscribe` with
//! session scope, `ProjectionResume` for reconnect/resync, and
//! `ProjectionUnsubscribe` on stop). Results land back on the event loop as
//! `TuiCommand::ObserveSubscribed` / `ObserveResumed` completions guarded by
//! per-observation request id + reconnect epoch, so stale completions (after
//! a tab switch, stop, or reconnect) are dropped at apply time.
//!
//! Concurrency: `start_*` bumps the reducer generation via
//! `ObserverState::begin_observe` / `begin_resume` and `apply_*` drops stale
//! completions. Observer disconnect tears down only the observer-owned
//! subscription; the target session is never steered, cancelled, or
//! mutated through this path.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::state::observe::observer_blocked_message;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Kick off an observation of `session_id` in `project_id`.
///
/// Replaces any existing observation: the previous observer-owned
/// subscription is unsubscribed best-effort before the new subscribe
/// begins, so at most one observed session is active.
pub(crate) fn start_observe(app: &mut App, project_id: String, session_id: String) {
    if project_id.is_empty() || session_id.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /observe <session-id> — open a project tab first");
        return;
    }
    // Tear down any previous observation first (observer-owned only).
    if let Some(old_sub) = app.observer.stop() {
        stop_subscription_best_effort(app, old_sub);
    }
    let Some(request_id) = app.observer.begin_observe(&project_id, &session_id) else {
        app.messages_state
            .toasts
            .warning("Invalid session locator — observation not started");
        return;
    };
    if app.core_client.is_none() {
        let epoch = app.observer.reconnect_epoch;
        apply_observe_subscribed(
            app,
            request_id,
            project_id,
            session_id,
            None,
            None,
            Some("Core unavailable — check daemon status with /doctor".to_string()),
            false,
            true,
            epoch,
        );
        return;
    }
    let reconnect_epoch = app.observer.reconnect_epoch;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "observe_subscribe",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ObserveSubscribed {
                    request_id,
                    project_id,
                    session_id,
                    subscription_id: None,
                    cursor: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            };

            // 1. Capability negotiation. Older daemons that do not
            //    advertise projection return `supported: false` or an
            //    error; both degrade to the generic unavailable state
            //    without breaking project tabs.
            let cap_request = crate::core::new_request(
                format!("observe-capabilities-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionCapabilities,
            );
            let supported = match core_client.request(cap_request).await {
                Ok(CoreResponse::ProjectionCapabilitiesResponse { supported, .. }) => supported,
                Ok(CoreResponse::Error { .. }) => false,
                Ok(_) => false,
                Err(_) => false,
            };
            if !supported {
                return Some(TuiCommand::ObserveSubscribed {
                    request_id,
                    project_id,
                    session_id,
                    subscription_id: None,
                    cursor: None,
                    error: None,
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            }

            // 2. Authorized session subscribe. Authorization is enforced
            //    daemon-side (`session.observe`); denials arrive as
            //    `project_not_found` and are indistinguishable from absent.
            let sub_request = crate::core::new_request(
                format!("observe-subscribe-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionSubscribe {
                    request: codegg_protocol::projection::replay::ProjectionSubscriptionRequest {
                        scope: codegg_protocol::projection::replay::ProjectionStreamKind::Session,
                        scope_id: session_id.clone(),
                        cursor: None,
                        projection_version: 1,
                    },
                },
            );
            match core_client.request(sub_request).await {
                Ok(CoreResponse::ProjectionSubscribed {
                    subscription_id,
                    cursor,
                    ..
                }) => Some(TuiCommand::ObserveSubscribed {
                    request_id,
                    project_id,
                    session_id,
                    subscription_id: Some(subscription_id),
                    cursor: Some(cursor),
                    error: None,
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    let unauthorized = code == "project_not_found";
                    Some(TuiCommand::ObserveSubscribed {
                        request_id,
                        project_id,
                        session_id,
                        subscription_id: None,
                        cursor: None,
                        error: Some(format!("{code}: {message}")),
                        unauthorized,
                        unsupported: false,
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::ObserveSubscribed {
                    request_id,
                    project_id,
                    session_id,
                    subscription_id: None,
                    cursor: None,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ObserveSubscribed {
                    request_id,
                    project_id,
                    session_id,
                    subscription_id: None,
                    cursor: None,
                    error: Some(format!("Observe subscribe failed: {e}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply an `ObserveSubscribed` completion. Drops stale completions
/// (wrong request id or reconnect epoch) and enforces the session-routing
/// guard in [`crate::tui::app::state::ObserverState`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_observe_subscribed(
    app: &mut App,
    request_id: u64,
    project_id: String,
    session_id: String,
    subscription_id: Option<codegg_protocol::projection::replay::ProjectionSubscriptionId>,
    cursor: Option<codegg_protocol::projection::replay::ProjectionCursor>,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if unsupported {
        let _ = app.observer.apply_error(
            request_id,
            &session_id,
            String::new(),
            true,
            reconnect_epoch,
        );
        app.messages_state
            .toasts
            .info("Observation unavailable — daemon does not support session projections");
        return;
    }
    if unauthorized {
        let _ = app
            .observer
            .apply_denied(request_id, &session_id, reconnect_epoch);
        // Privacy-preserving: identical message for denied vs absent so
        // observers learn nothing about existence.
        app.messages_state
            .toasts
            .warning("Session not found or not authorized for observation");
        return;
    }
    if let (Some(sub), Some(cur)) = (subscription_id, cursor) {
        if app.observer.apply_subscribed(
            request_id,
            &project_id,
            &session_id,
            sub,
            cur,
            reconnect_epoch,
        ) {
            if let Some(banner) = app.observer.banner_line() {
                app.messages_state
                    .toasts
                    .info(&format!("{banner} — input is read-only"));
            }
        }
        return;
    }
    if let Some(err) = error {
        let _ =
            app.observer
                .apply_error(request_id, &session_id, err.clone(), false, reconnect_epoch);
        app.messages_state
            .toasts
            .warning(&format!("Observe failed: {err}"));
    }
}

/// Resume the active observation after reconnect/lag/resync.
///
/// Uses the last authoritative cursor when present (`ProjectionResume`);
/// otherwise performs a fresh `ProjectionSubscribe`. Authorization is
/// rechecked daemon-side on every resume; revoked grants clean transient
/// state and deny as `project_not_found`.
pub(crate) fn resume_observe(app: &mut App) {
    let Some((request_id, session_id, cursor)) = app.observer.begin_resume() else {
        return;
    };
    if app.core_client.is_none() {
        let epoch = app.observer.reconnect_epoch;
        apply_observe_resumed(
            app,
            request_id,
            session_id,
            None,
            0,
            Some("Core unavailable".to_string()),
            false,
            epoch,
        );
        return;
    }
    let reconnect_epoch = app.observer.reconnect_epoch;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "observe_resume",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ObserveResumed {
                    request_id,
                    session_id,
                    cursor: None,
                    last_delivered_seq: 0,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    reconnect_epoch,
                });
            };
            if let Some(cursor) = cursor {
                let resume_request = crate::core::new_request(
                    format!("observe-resume-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionResume {
                        cursor: cursor.clone(),
                        include_snapshot_if_resync: true,
                    },
                );
                match core_client.request(resume_request).await {
                    Ok(CoreResponse::ProjectionReplay { batch, .. }) => {
                        let next = batch
                            .next_cursor
                            .as_ref()
                            .map(|c| c.event_seq)
                            .unwrap_or(batch.current_high_water);
                        // Reconstruct a cursor at the replay end for the
                        // reducer watermark.
                        let resumed = codegg_protocol::projection::replay::ProjectionCursor {
                            stream_id: batch.descriptor.stream_id.clone(),
                            event_seq: batch.replay_end_seq,
                            projection_version: batch.descriptor.projection_version,
                        };
                        return Some(TuiCommand::ObserveResumed {
                            request_id,
                            session_id,
                            cursor: Some(resumed),
                            last_delivered_seq: next,
                            error: None,
                            unauthorized: false,
                            reconnect_epoch,
                        });
                    }
                    Ok(CoreResponse::ProjectionResyncRequired { descriptor, .. }) => {
                        // Resync: fall through to a fresh subscribe below
                        // so the observer converges on the authoritative
                        // snapshot instead of a stale cursor.
                        let _ = descriptor;
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        let unauthorized = code == "project_not_found";
                        return Some(TuiCommand::ObserveResumed {
                            request_id,
                            session_id,
                            cursor: None,
                            last_delivered_seq: 0,
                            error: Some(format!("{code}: {message}")),
                            unauthorized,
                            reconnect_epoch,
                        });
                    }
                    Ok(other) => {
                        return Some(TuiCommand::ObserveResumed {
                            request_id,
                            session_id,
                            cursor: None,
                            last_delivered_seq: 0,
                            error: Some(format!("Unexpected core response: {other:?}")),
                            unauthorized: false,
                            reconnect_epoch,
                        });
                    }
                    Err(e) => {
                        return Some(TuiCommand::ObserveResumed {
                            request_id,
                            session_id,
                            cursor: None,
                            last_delivered_seq: 0,
                            error: Some(format!("Observe resume failed: {e}")),
                            unauthorized: false,
                            reconnect_epoch,
                        });
                    }
                }
            }
            // Fresh subscribe fallback (no cursor or resync required).
            let sub_request = crate::core::new_request(
                format!("observe-resubscribe-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionSubscribe {
                    request: codegg_protocol::projection::replay::ProjectionSubscriptionRequest {
                        scope: codegg_protocol::projection::replay::ProjectionStreamKind::Session,
                        scope_id: session_id.clone(),
                        cursor: None,
                        projection_version: 1,
                    },
                },
            );
            match core_client.request(sub_request).await {
                Ok(CoreResponse::ProjectionSubscribed { cursor, .. }) => {
                    let last_delivered_seq = cursor_event_seq(&cursor);
                    Some(TuiCommand::ObserveResumed {
                        request_id,
                        session_id,
                        cursor: Some(cursor),
                        last_delivered_seq,
                        error: None,
                        unauthorized: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    let unauthorized = code == "project_not_found";
                    Some(TuiCommand::ObserveResumed {
                        request_id,
                        session_id,
                        cursor: None,
                        last_delivered_seq: 0,
                        error: Some(format!("{code}: {message}")),
                        unauthorized,
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::ObserveResumed {
                    request_id,
                    session_id,
                    cursor: None,
                    last_delivered_seq: 0,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::ObserveResumed {
                    request_id,
                    session_id,
                    cursor: None,
                    last_delivered_seq: 0,
                    error: Some(format!("Observe resubscribe failed: {e}")),
                    unauthorized: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

fn cursor_event_seq(cursor: &codegg_protocol::projection::replay::ProjectionCursor) -> u64 {
    cursor.event_seq
}

/// Apply an `ObserveResumed` completion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_observe_resumed(
    app: &mut App,
    request_id: u64,
    session_id: String,
    cursor: Option<codegg_protocol::projection::replay::ProjectionCursor>,
    last_delivered_seq: u64,
    error: Option<String>,
    unauthorized: bool,
    reconnect_epoch: u64,
) {
    if unauthorized {
        let _ = app
            .observer
            .apply_denied(request_id, &session_id, reconnect_epoch);
        app.messages_state
            .toasts
            .warning("Session not found or not authorized for observation");
        return;
    }
    if let Some(cursor) = cursor {
        if app.observer.apply_resumed(
            request_id,
            &session_id,
            cursor,
            last_delivered_seq,
            reconnect_epoch,
        ) {
            app.messages_state
                .toasts
                .info("Observation resynced — live");
        }
        return;
    }
    if let Some(err) = error {
        let _ =
            app.observer
                .apply_error(request_id, &session_id, err.clone(), false, reconnect_epoch);
        app.messages_state
            .toasts
            .warning(&format!("Observe resync failed: {err}"));
    }
}

/// Stop the active observation. Tears down only the observer-owned
/// subscription; the target session keeps running. Clears local observed
/// state so hidden data is never rendered from a cache.
pub(crate) fn stop_observing(app: &mut App) {
    let Some(owned) = app.observer.stop() else {
        if app.observer.is_observing() {
            // Denied/unsupported shell with no subscription id.
            app.observer.clear();
        }
        app.messages_state.toasts.info("Not observing");
        return;
    };
    stop_subscription_best_effort(app, owned);
    app.messages_state
        .toasts
        .info("Stopped observing — control restored");
}

fn stop_subscription_best_effort(
    app: &mut App,
    subscription_id: codegg_protocol::projection::replay::ProjectionSubscriptionId,
) {
    let Some(core_client) = app.core_client.clone() else {
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "observe_unsubscribe",
        async move {
            let req = crate::core::new_request(
                format!("observe-unsubscribe-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionUnsubscribe {
                    subscription_id: subscription_id.clone(),
                },
            );
            let _ = core_client.request(req).await;
            Some(TuiCommand::ObserveUnsubscribed {
                subscription_id: Some(subscription_id),
            })
        },
    );
}

/// Central observer-mode guard for slash commands. Returns `true` when the
/// command was blocked (toast already shown). Callers MUST check this
/// before any mutating dispatch while observing.
pub(crate) fn check_observer_block(app: &mut App, raw_input: &str) -> bool {
    // Extract the slash command name (`/observe foo` -> `observe`).
    let name = raw_input
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    if name.is_empty() {
        return false;
    }
    // The observe lifecycle itself is always allowed.
    if matches!(
        name.to_lowercase().as_str(),
        "observe" | "watch" | "stop-observing" | "unwatch"
    ) {
        return false;
    }
    if app.observer.blocks_command(name) {
        app.messages_state
            .toasts
            .warning(&observer_blocked_message(name));
        return true;
    }
    false
}
