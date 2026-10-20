//! Stopping and carrying on with the agent turn in flight.
//!
//! A turn that cannot be interrupted is a turn the user can only wait out. A
//! multi-minute `bash` build looks exactly like a disconnect: nothing moves,
//! nothing says why, and the only honest reaction is to kill the terminal.
//! This module gives that state three answers — `/stop` to interrupt it, a
//! visible stopped state, and `/start` to carry on.
//!
//! Both directions are thin by design. `CoreRequest::TurnCancel` already
//! exists and the daemon already honours it (it sets the turn's cancel
//! channel); the only thing the TUI was missing was the turn id needed to
//! address it. Conversely there is no protocol-level "resume a stopped turn",
//! so [`start`] re-submits rather than pretending to revive a dead one —
//! the cancelled turn cannot be resurrected, and a `/start` that claimed
//! otherwise would silently run something the user did not ask for.
//!
//! The spelling is `/start` rather than `/resume` because `/sessions` owns
//! the `resume` alias for session switching; see `BuiltinSlashAction::StartTurn`.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::state::prompt::STOP_ARM_WINDOW;
use crate::tui::app::TuiCommand;
use crate::tui::app::{App, SessionStatus};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// The prompt `/start` submits when the user has not typed one.
///
/// Deliberately empty of instructions: the agent already has the whole
/// transcript, including the turn the user just stopped, so "continue" asks
/// it to pick up where it left off rather than re-deriving what to do.
const START_PROMPT: &str = "Continue where you left off.";

/// How the stop/resume key is written wherever the user is told about it.
///
/// One spelling shared by the strip, the confirmation, and the dispatch call:
/// three independently typed labels are three chances for the hint to name a
/// key the user cannot find.
///
/// The spelling is the chord, not the old bare `x`: ADR-0042 gave the bare
/// letter to the composer's Helix line-select, so the action moved to
/// `Ctrl+Alt+X` and the hint has to name the key that actually works.
pub(crate) const STOP_KEY_LABEL: &str = "Ctrl+Alt+X";

/// Stop the running turn.
///
/// Refuses rather than guessing when there is nothing to stop: a `/stop`
/// pressed on an idle session should say so, not claim success for a request
/// it never made.
pub(crate) fn stop(app: &mut App) {
    if !matches!(app.session_state.session_status, SessionStatus::Working) {
        app.messages_state
            .toasts
            .info("Nothing to stop - the agent is not working");
        return;
    }
    let Some(session_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) else {
        app.messages_state
            .toasts
            .error("Cannot stop the agent: no session is bound");
        return;
    };
    // `TurnCancel` is addressed by `(session_id, turn_id)` and the daemon
    // rejects a mismatch outright, so without the id there is nothing to send.
    // This is the honest failure rather than a turn that looks stopped and
    // keeps running.
    let Some(turn_id) = app.session_state.active_turn_id.clone() else {
        app.messages_state.toasts.error(
            "Cannot stop the agent: this turn has not reported an id yet. Try again in a moment.",
        );
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Cannot stop the agent: the codegg core is unavailable.");
        return;
    };

    // Mark stopped immediately. The daemon's cooperative cancel means the turn
    // unwinds over its next poll, so waiting for `TurnCompleted` would leave a
    // visible lag between the keypress and the badge — and a user who pressed
    // stop twice needs the second press to be a no-op, not a second request.
    app.session_state.stopped_turn = true;
    clear_pending_stop_armed(app);

    spawn_registered_tui_task(
        app.tui_cmd_tx.clone(),
        &mut app.task_registry,
        TuiTaskKind::Command,
        "turn_cancel",
        async move {
            let request = crate::core::new_request(
                format!("turn-cancel-{}", uuid::Uuid::new_v4()),
                CoreRequest::TurnCancel {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                },
            );
            let error = match core_client.request(request).await {
                Ok(CoreResponse::Ack) => None,
                Ok(CoreResponse::Error { code, message }) => Some(format!("{code}: {message}")),
                Ok(other) => Some(format!("unexpected core response: {other:?}")),
                Err(error) => Some(error.to_string()),
            };
            Some(TuiCommand::TurnCancelRequested {
                session_id,
                turn_id,
                error,
            })
        },
    );
}

/// Apply a `TurnCancelRequested` completion.
///
/// `no_active_turn` and `turn_id_mismatch` mean the turn already finished on
/// its own between the keypress and the request — the outcome the user asked
/// for, reached without us. Reporting those as errors would punish the common
/// race, so they are absorbed and only a genuine transport failure is surfaced.
pub(crate) fn apply_cancel_result(
    app: &mut App,
    session_id: &str,
    turn_id: &str,
    error: Option<String>,
) {
    // Stale completion: a newer turn (or a session switch) owns the state now.
    if app.session_state.active_turn_id.as_deref() != Some(turn_id)
        || app.session_state.session.as_ref().map(|s| s.id.as_str()) != Some(session_id)
    {
        return;
    }
    let Some(error) = error else {
        app.messages_state.toasts.info("Stopping the agent...");
        return;
    };
    if error.starts_with("no_active_turn") || error.starts_with("turn_id_mismatch") {
        app.session_state.active_turn_id = None;
        app.session_state.stopped_turn = true;
        app.messages_state
            .toasts
            .info("The agent had already finished - nothing to stop");
        return;
    }
    // The request failed, so the turn is still running: undo the optimistic
    // badge rather than leaving a stopped agent that is not stopped.
    app.session_state.stopped_turn = false;
    app.messages_state
        .toasts
        .error(&format!("Could not stop the agent: {error}"));
}

/// Carry on after a stopped agent by starting a fresh turn.
///
/// This is a new turn, not a resurrected one: the daemon has no
/// resume-a-cancelled-turn request, and the cancelled turn's work is simply
/// gone. The transcript carries the context, so re-submitting a short
/// continuation prompt reproduces what the user means by "carry on".
pub(crate) fn start(app: &mut App) {
    if !app.session_state.stopped_turn {
        app.messages_state
            .toasts
            .info("The agent is not stopped - nothing to carry on from");
        return;
    }
    if matches!(app.session_state.session_status, SessionStatus::Working) {
        app.messages_state
            .toasts
            .info("The agent is already working");
        return;
    }
    // The stopped turn can no longer be cancelled, and the next turn must not
    // inherit that badge.
    app.session_state.stopped_turn = false;
    clear_pending_stop_armed(app);

    if app.session_state.session.is_none() {
        // Reached by stopping a session that has since been closed. Creating
        // one here is better than submitting into nothing.
        crate::tui::commands::sessions::start_new_session(app);
        app.messages_state
            .toasts
            .info("Carrying on in a new session - send a prompt to continue");
        return;
    }

    // A draft the user typed while the agent was stopped is what they meant to
    // send, so it wins over the generic continuation prompt. `send_prompt`
    // owns consuming the composer.
    if app.prompt_state.prompt.get_text().trim().is_empty() {
        app.prompt_state.prompt.set_text(START_PROMPT.to_string());
    }
    app.send_prompt();
}

/// Clear an armed stop confirmation.
pub(crate) fn clear_pending_stop_armed(app: &mut App) {
    app.prompt_state.stop_armed_at = None;
}

/// Handle the stop/resume key.
///
/// One key, two meanings, chosen by state: a stopped agent carries on, a
/// working one stops. Stop is the destructive direction, so it takes a
/// confirmed second press inside [`STOP_ARM_WINDOW`] — an accidental `x` while
/// reaching for a chord must not kill a turn.
pub(crate) fn toggle_stop_or_resume(app: &mut App, key_label: &str) {
    if app.session_state.stopped_turn {
        clear_pending_stop_armed(app);
        start(app);
        return;
    }
    if !matches!(app.session_state.session_status, SessionStatus::Working) {
        clear_pending_stop_armed(app);
        return;
    }
    // Second press inside the window: commit.
    if app
        .prompt_state
        .stop_armed_at
        .is_some_and(|armed| armed.elapsed() < STOP_ARM_WINDOW)
    {
        stop(app);
        return;
    }
    // First press (or a lapsed arm): arm, and say exactly what to press.
    app.prompt_state.stop_armed_at = Some(std::time::Instant::now());
    app.messages_state
        .toasts
        .info(&format!("press {key_label} again to stop agent"));
}

/// Drop an armed confirmation that has outlived its window.
///
/// Called from the render pump so the prompt disappears on its own even if no
/// further key is pressed.
pub(crate) fn expire_pending_stop_armed(app: &mut App) {
    if app
        .prompt_state
        .stop_armed_at
        .is_some_and(|armed| armed.elapsed() >= STOP_ARM_WINDOW)
    {
        app.prompt_state.stop_armed_at = None;
    }
}

/// Whether the stop confirmation is currently showing.
pub(crate) fn stop_armed(app: &App) -> bool {
    app.prompt_state.stop_armed_at.is_some_and(|armed| {
        armed.elapsed() < STOP_ARM_WINDOW
            && matches!(app.session_state.session_status, SessionStatus::Working)
            && !app.session_state.stopped_turn
    })
}
