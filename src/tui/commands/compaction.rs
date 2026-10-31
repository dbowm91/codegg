//! The `/compact` command: the frontend asks, the daemon compacts.
//!
//! C002 (ADR-0060): the daemon owns compaction. The command **asks** the
//! daemon to compact and reports the daemon's answer — a refusal included.
//! The frontend never claims a compaction it did not cause.
//!
//! The compaction happens at the daemon's next compaction opportunity for the
//! session, and the daemon reports it through `AppEvent::CompactionTriggered`.

use crate::protocol::core::CoreResponse;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Ask the daemon to compact this session's context.
pub(crate) fn handle_compact_session(app: &mut App) {
    if app.session_state.session.is_none() {
        app.messages_state
            .toasts
            .info("No session is attached, so there is nothing to compact.");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Could not reach the agent: no core connection");
        return;
    };
    let scope = crate::tui::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "compact_session",
        async move {
            // The daemon's own catalog command, so the frontend asks for
            // `compact` and the daemon decides what that means (ADR-0052).
            let answer =
                crate::tui::command_route::send_command(&core_client, scope, "compact", "", "")
                    .await;
            Some(TuiCommand::CompactSessionFinished {
                response: answer.as_ref().ok().cloned(),
                error: answer.err().map(|error| error.to_string()),
            })
        },
    );
}

/// Apply the daemon's answer to a compaction request.
///
/// A refusal is reported as a refusal, and a transport failure says the
/// request never reached the daemon — neither is dressed up as a compaction.
pub(crate) fn apply_compact_session_finished(
    app: &mut App,
    response: Option<CoreResponse>,
    error: Option<String>,
) {
    let count = app.session_state.compaction_count;
    let (message, failed) = compact_answer_message(response.as_ref(), error.as_deref(), count);
    if failed {
        app.messages_state.toasts.error(&message);
    } else {
        app.messages_state.toasts.info(&message);
    }
}

/// The message the daemon's answer earns, and whether it is a failure.
///
/// The count is the TUI's observed total, not a session's.
pub(crate) fn compact_answer_message(
    response: Option<&CoreResponse>,
    error: Option<&str>,
    count: usize,
) -> (String, bool) {
    match response {
        Some(CoreResponse::SessionCompact { turn_active, .. }) => {
            let when = if *turn_active {
                "in the running turn"
            } else {
                "at the start of the next turn"
            };
            (
                format!(
                    "Compaction requested. The daemon compacts this session's context {when}. \
                     Compactions so far: {count}."
                ),
                false,
            )
        }
        Some(CoreResponse::Error { code, message }) => (
            format!("The daemon refused the compaction request: {message} ({code})"),
            true,
        ),
        Some(CoreResponse::CommandExecute { result }) => {
            let message = match result.status {
                codegg_protocol::core::CommandOutcomeStatusDto::Refused => format!(
                    "The daemon refused the compaction request: {}",
                    result.summary
                ),
                _ => format!(
                    "The compaction request did not complete: {}",
                    result.summary
                ),
            };
            (message, true)
        }
        Some(_) => (
            "The daemon's answer to the compaction request was not understood.".to_string(),
            true,
        ),
        None => (
            format!(
                "The compaction request did not reach the daemon: {}",
                error.unwrap_or("no reason was given")
            ),
            true,
        ),
    }
}
