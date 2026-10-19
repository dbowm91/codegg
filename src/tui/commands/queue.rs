//! M004: the `/queue` box's three operations — open, refresh, and delete.
//!
//! Everything the box needs already exists daemon-side. `UserPromptQueueList`
//! is a `ViaSession` + `Capability::SessionRead` read, and
//! `UserPromptQueueDelete` is `ViaSession` + `Capability::AgentInvoke` **plus**
//! the durable queue owner, enforced by `ControllerScope::QueueMutation`. This
//! module issues those requests and applies their answers; it decides nothing
//! about whether a delete is allowed.
//!
//! Two properties are load-bearing and both are about not lying to the user:
//!
//! * **A delete applies the projection that delete produced.**
//!   `queue_projection` re-reads after a successful write and returns it in
//!   the same response. Issuing a second read instead would be one extra round
//!   trip and would let a concurrent mutation render a list that disagrees
//!   with the write the user just made.
//! * **A refusal keeps its own identity.** `delivering` is not `expired` and
//!   not `not_found`: the entry is live and its turn is running. Each gets its
//!   own sentence, because collapsing them tells the user their prompt expired
//!   at the moment a turn started — which is a lie they might act on.
//!
//! Concurrency follows the `start_*`/`apply_*` pair: `start_queue_delete` bumps
//! the request generation and the matching apply drops stale completions.
//! `close_dialog` cancels it, so a delete still in flight when the box closes
//! cannot mutate a projection nobody is looking at.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::{App, Dialog, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::components::dialogs::queue::QueueDialog;
use crate::tui::task_lifecycle::TuiTaskKind;

/// The daemon's wire codes this box speaks.
///
/// Matched as literals rather than re-derived from the enum: the TUI must not
/// import the daemon's error type to build its copy, because the moment the
/// enum is refactored the two would move together and a mismatch would become
/// silent instead of a test failure. The tests below pin every constant here
/// against the real `UserPromptQueueError::code()`, which is what caught
/// `user_prompt_queue_entry_expired` — a code that does not exist, written from
/// the `entry_` prefix the neighbouring one carries.
const DELIVERING_CODE: &str = "user_prompt_queue_entry_delivering";
const EXPIRED_CODE: &str = "user_prompt_queue_expired";
const NOT_FOUND_CODE: &str = "user_prompt_queue_not_found";
const NOT_OWNER_CODE: &str = "user_prompt_queue_not_owner";
const UNOWNED_CODE: &str = "user_prompt_queue_unowned";
const OWNER_REVISION_CODE: &str = "user_prompt_queue_owner_revision_conflict";
/// M005. The permutation the client sent named a different live set than the
/// one the daemon has — nearly always a stale projection, because another
/// frontend mutated the queue between this box's read and its write.
const ORDER_MISMATCH_CODE: &str = "user_prompt_queue_order_mismatch";
/// M005. The session already holds the maximum number of retained entries.
const TOO_MANY_ENTRIES_CODE: &str = "user_prompt_queue_too_many_entries";
/// M005. The text exceeded `USER_PROMPT_QUEUE_MAX_ENTRY_BYTES`.
const ENTRY_TOO_LARGE_CODE: &str = "user_prompt_queue_entry_too_large";
/// M005. A field failed validation — blank text is the one a user can hit.
const INVALID_CODE: &str = "user_prompt_queue_invalid";
/// M006. No session is bound, so there is no turn to steer. Raised before the
/// request is built rather than by the daemon.
pub(super) const SESSION_NOT_FOUND_CODE: &str = "session_not_found";
/// M006. The session has no active turn. The daemon is the authority for this
/// one; the box's own pre-flight only avoids a pointless round trip.
pub(super) const NO_ACTIVE_TURN_CODE: &str = "no_active_turn";
/// M006. The turn ended between the box reading its id and the daemon reaching
/// the runtime, so the prompt was never handed over.
pub(super) const TURN_ID_MISMATCH_CODE: &str = "turn_id_mismatch";
/// M006. The steering channel's receiver is gone — the turn is finishing.
pub(super) const STEER_UNAVAILABLE_CODE: &str = "steer_unavailable";
/// M006. The steering channel is saturated; the turn is not accepting more.
pub(super) const STEER_QUEUE_FULL_CODE: &str = "steer_queue_full";
/// M006. This turn was started without a steering channel at all.
pub(super) const STEER_NOT_SUPPORTED_CODE: &str = "steer_not_supported";
/// M006. Another principal holds this session's turn controller (ADR-0007).
pub(super) const NOT_CONTROLLER_CODE: &str = "session_control_not_controller";
/// M006. The turn controller is held by someone else at the claimed revision.
pub(super) const CONTROL_CONFLICT_CODE: &str = "session_control_conflict";
/// M006. The session has no durable queue to steer out of.
pub(super) const QUEUE_UNAVAILABLE_CODE: &str = "user_prompt_queue_unavailable";

/// The tone for a send-now outcome: informational when the user's press did
/// what they asked, a warning when something else decided instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SendNowTone {
    Info,
    Warning,
}

/// M006: the sentence for a send-now that produced no request.
///
/// Split from the daemon's answers because these two never reached it: a
/// `delivering` row and an idle session are both facts this box already holds.
pub fn send_now_refusal_message(
    reason: crate::tui::components::dialogs::queue::QueueSendRefusal,
) -> &'static str {
    use crate::tui::components::dialogs::queue::QueueSendRefusal;
    match reason {
        QueueSendRefusal::Delivering => {
            "That prompt is being delivered right now — a turn is already running for it."
        }
        QueueSendRefusal::NoActiveTurn => {
            "Nothing is running to send it to. It stays queued for the next turn."
        }
    }
}

/// Who receives a steering payload, and who still holds it if the daemon
/// refuses.
///
/// The *codes* are shared — the daemon rejects a queued send-now and a bare
/// addressed reply with the same words, because they push into the same channel
/// on the same turn. What differs is the **consequence**, and a sentence that
/// ignores it lies in one direction or the other: telling an author whose
/// composer still holds their text that it "stays queued" promises a resend
/// nothing is going to perform, and telling them a reply "was delivered" names
/// as recipient a party that ADR-0040 explicitly does not deliver to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SteerConsequence {
    /// The running turn takes it as its next instruction, and the session's
    /// durable prompt queue still holds it if refused. M006 send-now.
    DurableQueue,
    /// The **controlling parent agent** takes it as an instruction about one
    /// subagent, and nothing durable holds it if refused — the text is still in
    /// the composer and the user must press Enter again themselves. C007.
    ComposerOnly,
}

impl SteerConsequence {
    /// The clause describing where a refused payload ended up.
    fn tail(self) -> &'static str {
        match self {
            SteerConsequence::DurableQueue => {
                " It stays queued, so a later turn is offered it again."
            }
            SteerConsequence::ComposerOnly => {
                " Nothing was sent; your text is still in the composer and nothing will \
                 resend it for you."
            }
        }
    }

    /// The sentence for a payload the turn accepted.
    ///
    /// `addressee` is the subagent's name for [`ComposerOnly`](Self::ComposerOnly)
    /// and `None` for [`DurableQueue`](Self::DurableQueue), which has no
    /// subagent to name. The two cannot share a sentence: "marked delivered to
    /// it" describes a queue write that never happens for a bare steer, and a
    /// steer that claimed delivery to the *subagent* would be the exact
    /// misrepresentation ADR-0040's failure modes forbid.
    pub fn accepted(self, addressee: Option<&str>) -> String {
        match (self, addressee) {
            (SteerConsequence::DurableQueue, _) => {
                "Handed to the turn that is running now, and marked delivered to it.".to_string()
            }
            (SteerConsequence::ComposerOnly, Some(name)) => format!(
                "Handed to the agent running this turn, addressed to {name}. Whether it \
                 relays that to the subagent is its call — nothing reached the subagent \
                 directly."
            ),
            (SteerConsequence::ComposerOnly, None) => {
                "Handed to the agent running this turn. Whether it relays that to the \
                 subagent is its call — nothing reached the subagent directly."
                    .to_string()
            }
        }
    }
}

/// M006/C007: the sentence for what the daemon did with a steering payload.
///
/// **Every sentence says what was accepted, never what the model will do.** A
/// payload that reaches a turn's steering channel has been *handed over*;
/// whether the model reads it, believes it, or acts on it this turn is not
/// something this daemon knows at the moment it accepts. Distinct outcomes
/// therefore get distinct sentences: a user who is told "sent" for a channel
/// that was full has been lied to, and a user told the same for a closed channel
/// cannot tell that their message did not go anywhere.
///
/// One vocabulary, two callers. `TurnSteer` (C007, an addressed reply) and
/// send-now (M006, a queued prompt) fail identically, so they must not render
/// identically-with-a-different-consequence: `consequence` supplies the clauses
/// that actually differ, rather than a second thinner function that could drift
/// on the codes this one already gets right.
///
/// `AcceptedNotSettled` gets its own sentence because it is the one case where
/// "resend it" would duplicate a prompt that already ran — the wording has to
/// tell the user to trust the turn, not the queue.
pub fn send_now_outcome_message(
    outcome: &crate::protocol::core::UserPromptQueueSendNowResult,
    consequence: SteerConsequence,
) -> (String, SendNowTone) {
    use crate::protocol::core::UserPromptQueueSendNowResult;
    match outcome {
        UserPromptQueueSendNowResult::Accepted { .. } => {
            (consequence.accepted(None), SendNowTone::Info)
        }
        UserPromptQueueSendNowResult::AcceptedNotSettled { .. } => (
            "The running turn took it, but this queue could not record that — so treat \
             it as sent, and check before sending it again."
                .to_string(),
            SendNowTone::Warning,
        ),
        UserPromptQueueSendNowResult::EntryNotFound => (
            "That prompt is already gone. Refreshing…".to_string(),
            SendNowTone::Warning,
        ),
        UserPromptQueueSendNowResult::Refused { code } => {
            // The arms that reach `consequence.tail()` are the ones where the
            // message itself was the thing refused, so the only open question
            // is where it ended up. The arms that return early are the ones
            // whose remedy is to look somewhere else entirely: a session that
            // is gone retains nothing under either mode, a turn held by
            // another frontend is not this user's to retry at all, and the
            // queue's own codes describe an entry rather than a message.
            let sentence: String = match code.as_str() {
                STEER_QUEUE_FULL_CODE => {
                    "The running turn has no room for another message right now.".to_string()
                }
                STEER_UNAVAILABLE_CODE => {
                    "That turn is finishing and is no longer accepting messages.".to_string()
                }
                STEER_NOT_SUPPORTED_CODE => {
                    "This turn cannot take messages while it runs.".to_string()
                }
                NO_ACTIVE_TURN_CODE => "That turn ended before the message reached it.".to_string(),
                // `turn_id_mismatch` (the runtime is running a different turn)
                // and `session_control_conflict` (the controller lease covers a
                // different turn — `check_turn_controller` returns it when
                // `lease_turn != turn_id`) are different codes with one cause
                // as far as the user is concerned: the turn they addressed is
                // no longer this session's current turn. One sentence, because
                // the remedy is identical.
                TURN_ID_MISMATCH_CODE | CONTROL_CONFLICT_CODE => {
                    "The turn you addressed is no longer this session's current turn, so the \
                     message was not handed over."
                        .to_string()
                }
                NOT_CONTROLLER_CODE => {
                    return (
                        "This session's turn belongs to another frontend, which is the one \
                         that can steer it."
                            .to_string(),
                        SendNowTone::Warning,
                    );
                }
                SESSION_NOT_FOUND_CODE => {
                    return (
                        "This session is no longer available, so nothing was sent.".to_string(),
                        SendNowTone::Warning,
                    );
                }
                QUEUE_UNAVAILABLE_CODE => {
                    return (
                        "There is no durable queue for this session, so nothing was sent."
                            .to_string(),
                        SendNowTone::Warning,
                    );
                }
                // The queue's own vocabulary, reusing the sentences M004/M005
                // already own rather than restating them differently.
                DELIVERING_CODE => {
                    return (
                        "That prompt is being delivered right now — a turn is already \
                         running for it."
                            .to_string(),
                        SendNowTone::Warning,
                    );
                }
                EXPIRED_CODE => {
                    return (
                        "That prompt is no longer waiting; it left the queue before you \
                         got to it."
                            .to_string(),
                        SendNowTone::Warning,
                    );
                }
                NOT_FOUND_CODE => {
                    return (
                        "That prompt is already gone. Refreshing…".to_string(),
                        SendNowTone::Warning,
                    );
                }
                NOT_OWNER_CODE => {
                    return (
                        "This session's queue belongs to another frontend; it can only be \
                         changed there."
                            .to_string(),
                        SendNowTone::Warning,
                    );
                }
                UNOWNED_CODE => {
                    return (
                        "This session's queue has no owner, so nothing can change it from \
                         here."
                            .to_string(),
                        SendNowTone::Warning,
                    );
                }
                other => format!("The running turn did not take it ({other})."),
            };
            (
                format!("{sentence}{}", consequence.tail()),
                SendNowTone::Warning,
            )
        }
    }
}

/// Whether a refusal should put the user's text back in the composer.
///
/// True for the bounds and validation refusals, because the fix is to shorten
/// or finish the text and the box still holds it. False for everything whose
/// remedy is to look at a different queue — ownership, delivery state, expiry.
/// Returning text into the composer in those cases would suggest the retry
/// will work when it cannot.
pub(super) fn refusal_keeps_text(code: &str) -> bool {
    matches!(
        code,
        ENTRY_TOO_LARGE_CODE | TOO_MANY_ENTRIES_CODE | INVALID_CODE
    )
}

/// Turn the daemon's stable wire code into the sentence the user reads.
///
/// `None` for anything unrecognized, which falls back to the code alone. That
/// fallback is deliberate: an unmapped code must still produce *some* honest
/// message rather than silently looking like success.
pub(super) fn refusal_message(code: &str) -> String {
    match code {
        DELIVERING_CODE => {
            "That prompt is being delivered right now — a turn is already running for it."
                .to_string()
        }
        EXPIRED_CODE => {
            "That prompt is no longer waiting; it left the queue before you got to it.".to_string()
        }
        NOT_FOUND_CODE => "That prompt is already gone. Refreshing…".to_string(),
        NOT_OWNER_CODE => {
            "This session's queue belongs to another frontend; it can only be changed there."
                .to_string()
        }
        UNOWNED_CODE => {
            // ADR-0037: adoption stays an explicit, audited operation, and the
            // plan puts it outside the box. So the box says plainly that there
            // is no repair *here*.
            //
            // Deliberately does **not** name a slash command for it. The
            // protocol request exists (`UserPromptQueueAdopt`) but no TUI
            // command registers one, so naming `/queue-adopt` here would be
            // this milestone's own version of the dead-key lie — telling the
            // user to run a command that does not exist. It says what kind of
            // operation would fix it instead.
            "This session's queue has no owner, so nothing can change it from \
             here. Claiming it is a separate, explicit operation outside this box."
                .to_string()
        }
        OWNER_REVISION_CODE => {
            "The queue owner changed while you were looking. Refresh and try again.".to_string()
        }
        ORDER_MISMATCH_CODE => {
            "The queue changed underneath that reorder, so it was refused rather \
             than merged. Showing the current order."
                .to_string()
        }
        TOO_MANY_ENTRIES_CODE => {
            "This session's queue is full. Delete or deliver something before adding more."
                .to_string()
        }
        ENTRY_TOO_LARGE_CODE => {
            "That prompt is too long for one queue entry. Shorten it and save again.".to_string()
        }
        INVALID_CODE => "That prompt was refused as invalid. Check it and try again.".to_string(),
        _ => format!("The queue refused that change ({code})."),
    }
}

/// Open `/queue`.
///
/// Deliberately **not** gated on `core_client`, matching `open_policy_selector`:
/// the box is a pure view over the cached projection, and refusing to open it
/// because the daemon is momentarily unreachable would make the command feel
/// broken for exactly the reason the read cannot help with. An unreachable
/// daemon surfaces from the refresh below as a normal failed read.
pub fn open_queue_box(app: &mut App) {
    if app
        .focus_manager
        .has_dialog(crate::tui::components::component::DialogType::Queue)
    {
        // `FocusManager::push` refuses a duplicate dialog type, so a second
        // `/queue` would otherwise silently do nothing while the command
        // looked like it had been swallowed.
        app.messages_state
            .toasts
            .info("The prompt queue box is already open.");
        return;
    }

    let session_id = app.active_session_id().map(str::to_owned);
    let Some(session_id) = session_id else {
        app.messages_state
            .toasts
            .info("No session is bound yet, so there is no prompt queue to show.");
        return;
    };

    let dialog = QueueDialog::new(
        std::sync::Arc::clone(&app.ui_state.theme),
        app.session_state.user_prompt_queue.clone(),
    );
    app.push_dialog(Dialog::Queue, Box::new(dialog));

    // The cached projection may predate a delivery or another frontend's
    // mutation. The push path covers the first; only a read covers the second,
    // so opening always issues one.
    app.load_user_prompt_queue(session_id);
}

/// `r` in the box: re-read the projection for the active session.
pub fn refresh_queue_box(app: &mut App) {
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        return;
    };
    app.load_user_prompt_queue(session_id);
}

/// Adopt a fresh projection into both the session cache and, when it is open,
/// the mounted box.
///
/// This is the single place the two are updated together. Routing the delete
/// response and the read/push responses through one applier is what makes it
/// impossible for the row 0 count and the box to disagree about how many
/// prompts are waiting.
pub fn adopt_queue_projection(
    app: &mut App,
    session_id: String,
    entries: Vec<crate::protocol::core::UserPromptQueueEntryDto>,
) {
    app.apply_user_prompt_queue_loaded(session_id, entries, None);
}

/// Start `UserPromptQueueDelete` for one entry.
///
/// The `entry_id` is carried from the box rather than re-read from the current
/// cursor, so a projection that changed between the keystroke and the request
/// cannot make the delete name a different prompt.
pub fn start_queue_delete(app: &mut App, entry_id: String) {
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .info("No session is bound, so there is nothing to delete from.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Prompt queue unavailable: no core connection");
        return;
    };

    let request_id = app.dialog_state.queue_delete_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "user_prompt_queue_delete",
        async move {
            let request = crate::core::new_request(
                format!("user-prompt-queue-delete-{}", uuid::Uuid::new_v4()),
                CoreRequest::UserPromptQueueDelete {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                },
            );
            let (entries, code, error) = match core_client.request(request).await {
                // The projection this very write produced. Applied directly —
                // no second read.
                Ok(CoreResponse::UserPromptQueue { entries, .. }) => (entries, None, None),
                Ok(CoreResponse::Error { code, .. }) => {
                    (Vec::new(), Some(code.clone()), Some(code))
                }
                Ok(_) => (
                    Vec::new(),
                    None,
                    Some("The queue delete got an unexpected response".to_string()),
                ),
                Err(error) => (Vec::new(), None, Some(error.to_string())),
            };
            Some(TuiCommand::UserPromptQueueDeleteFinished {
                request_id,
                session_id,
                entries,
                code,
                error,
            })
        },
    );
}

/// Apply a delete completion.
///
/// A stale or cancelled completion mutates nothing at all — not the
/// projection, not the toast, not the selection.
pub fn apply_queue_delete_finished(
    app: &mut App,
    request_id: u64,
    session_id: String,
    entries: Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    code: Option<String>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if !app
            .dialog_state
            .queue_delete_request
            .fail(request_id, error.clone())
        {
            // Superseded or cancelled: the box closed, or a newer delete is in
            // flight. Dropping the message is correct — there is nothing left
            // on screen to tell it to.
            return;
        }
        let message = match &code {
            Some(code) => refusal_message(code),
            None => format!("Could not delete that prompt ({error})."),
        };
        app.messages_state.toasts.warning(&message);

        // A row that vanished under us must not leave a permanently
        // unselectable index behind, so re-read rather than guess. A
        // `delivering` refusal does not re-read: the entry is still there, and
        // re-reading would only re-render what the user is already looking at.
        if code.as_deref() == Some(NOT_FOUND_CODE) {
            refresh_queue_box(app);
        }
        return;
    }

    if !app.dialog_state.queue_delete_request.finish(request_id) {
        return;
    }
    adopt_queue_projection(app, session_id, entries);
}

// ── M005: add, edit, reorder ───────────────────────────────────────────
//
// All three share one shape, and the shape is the point:
//
// * the request is the mutation, and its **own** response carries the
//   post-write projection — applied directly, never followed by a second read;
// * a refusal restores the user's text only when retrying can work;
// * a stale or cancelled completion mutates nothing at all.
//
// They are written as three separate pairs rather than one generic helper
// because their refusal handling genuinely differs: only reorder can be
// `order_mismatch`, only add can mint an id, only add needs the cursor to
// follow a row that did not exist before.

/// Mint an id and send `UserPromptQueueAdd`.
///
/// The id is minted **here**, at commit, and is the at-most-once key the
/// protocol asks for: the same id re-derives the same row rather than
/// duplicating the user's prompt, so a retry after an ambiguous failure cannot
/// queue the same sentence twice.
pub fn start_queue_add(app: &mut App, text: String, run_at_ms: Option<i64>) {
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .info("No session is bound, so there is nothing to add to.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        // Nothing left the process, so the composed text is still here.
        app.focus_manager.with_dialog_mut::<QueueDialog, _>(
            crate::tui::components::component::DialogType::Queue,
            |dialog| dialog.restore_compose(None, text),
        );
        app.messages_state
            .toasts
            .error("Prompt queue unavailable: no core connection");
        return;
    };

    let entry_id = uuid::Uuid::new_v4().to_string();
    let request_id = app.dialog_state.queue_add_request.begin();
    let tx = app.tui_cmd_tx.clone();
    let committed = entry_id.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "user_prompt_queue_add",
        async move {
            let request = crate::core::new_request(
                format!("user-prompt-queue-add-{}", uuid::Uuid::new_v4()),
                CoreRequest::UserPromptQueueAdd {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                    text: text.clone(),
                    run_at_ms,
                },
            );
            let (entries, code, error) = queue_mutation_answer(core_client.request(request).await);
            Some(TuiCommand::UserPromptQueueAddFinished {
                request_id,
                session_id,
                entries,
                code,
                error,
                entry_id: committed,
                // Handed back so a bounds refusal can restore what the user
                // typed. This is the one place composed text travels as far as
                // an in-process command — it never reaches a toast, a hint, or
                // a log, and dropping it would make "shorten it and save
                // again" impossible to act on.
                text,
            })
        },
    );
}

/// Apply an add completion and move the cursor onto what was added.
pub fn apply_queue_add_finished(
    app: &mut App,
    request_id: u64,
    session_id: String,
    entries: Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    code: Option<String>,
    error: Option<String>,
    entry_id: String,
    text: String,
) {
    match apply_refusal(
        &mut app.dialog_state.queue_add_request,
        request_id,
        &code,
        &error,
    ) {
        RefusalOutcome::Stale => return,
        RefusalOutcome::Refused {
            message,
            restore_text,
        } => {
            app.messages_state.toasts.warning(&message);
            if restore_text {
                app.focus_manager.with_dialog_mut::<QueueDialog, _>(
                    crate::tui::components::component::DialogType::Queue,
                    |dialog| dialog.restore_compose(None, text),
                );
            }
            if code.as_deref() == Some(TOO_MANY_ENTRIES_CODE) {
                // The queue is full; the count on screen is the thing to look at.
                refresh_queue_box(app);
            }
            return;
        }
        RefusalOutcome::Applied => {}
    }

    adopt_queue_projection(app, session_id, entries);
    // Follow the row that was just created. Keyed by the id minted at commit,
    // so the cursor lands on the user's own prompt and not on whatever row
    // happened to occupy the old index.
    app.focus_manager.with_dialog_mut::<QueueDialog, _>(
        crate::tui::components::component::DialogType::Queue,
        |dialog| {
            dialog.select_entry_id(&entry_id);
        },
    );
}

/// Send `UserPromptQueueEdit` for one entry.
pub fn start_queue_edit(app: &mut App, entry_id: String, text: String, run_at_ms: Option<i64>) {
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .info("No session is bound, so there is nothing to edit.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        // Nothing left the process, so the edit is still composable.
        app.focus_manager.with_dialog_mut::<QueueDialog, _>(
            crate::tui::components::component::DialogType::Queue,
            |dialog| dialog.restore_compose(Some(entry_id), text),
        );
        app.messages_state
            .toasts
            .error("Prompt queue unavailable: no core connection");
        return;
    };

    let request_id = app.dialog_state.queue_edit_request.begin();
    let tx = app.tui_cmd_tx.clone();
    let edited = entry_id.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "user_prompt_queue_edit",
        async move {
            let request = crate::core::new_request(
                format!("user-prompt-queue-edit-{}", uuid::Uuid::new_v4()),
                CoreRequest::UserPromptQueueEdit {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                    text: text.clone(),
                    run_at_ms,
                },
            );
            let (entries, code, error) = queue_mutation_answer(core_client.request(request).await);
            Some(TuiCommand::UserPromptQueueEditFinished {
                request_id,
                session_id,
                entries,
                code,
                error,
                entry_id: edited,
                text,
            })
        },
    );
}

/// Apply an edit completion.
///
/// A success applies the projection the edit produced, which is what keeps the
/// row's identity and position unchanged on screen: the daemon decides both,
/// and this simply renders its answer. A `delivering` refusal keeps the
/// selection where it was — the box never moves the cursor on a refusal, so a
/// rejected edit cannot quietly retarget the user at a different row.
pub fn apply_queue_edit_finished(
    app: &mut App,
    request_id: u64,
    session_id: String,
    entries: Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    code: Option<String>,
    error: Option<String>,
    entry_id: String,
    text: String,
) {
    match apply_refusal(
        &mut app.dialog_state.queue_edit_request,
        request_id,
        &code,
        &error,
    ) {
        RefusalOutcome::Stale => return,
        RefusalOutcome::Refused {
            message,
            restore_text,
        } => {
            app.messages_state.toasts.warning(&message);
            if restore_text {
                app.focus_manager.with_dialog_mut::<QueueDialog, _>(
                    crate::tui::components::component::DialogType::Queue,
                    |dialog| dialog.restore_compose(Some(entry_id), text),
                );
            }
            // A row that vanished under the editor cannot be re-seeded from the
            // projection, so re-read to show what is actually there. It is
            // deliberately **not** re-seeded into the composer: the row is gone,
            // so there is nothing to seed from and retyping is the only option.
            if code.as_deref() == Some(NOT_FOUND_CODE) {
                refresh_queue_box(app);
            }
            return;
        }
        RefusalOutcome::Applied => {}
    }

    adopt_queue_projection(app, session_id, entries);
}

/// Send `UserPromptQueueReorder` with the permutation the box built.
pub fn start_queue_reorder(app: &mut App, entry_ids: Vec<String>) {
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .info("No session is bound, so there is nothing to reorder.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Prompt queue unavailable: no core connection");
        return;
    };

    let request_id = app.dialog_state.queue_reorder_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "user_prompt_queue_reorder",
        async move {
            let request = crate::core::new_request(
                format!("user-prompt-queue-reorder-{}", uuid::Uuid::new_v4()),
                CoreRequest::UserPromptQueueReorder {
                    session_id: session_id.clone(),
                    entry_ids,
                },
            );
            let (entries, code, error) = queue_mutation_answer(core_client.request(request).await);
            Some(TuiCommand::UserPromptQueueReorderFinished {
                request_id,
                session_id,
                entries,
                code,
                error,
            })
        },
    );
}

/// Apply a reorder completion.
///
/// `order_mismatch` re-reads **once** and reports. It does not retry: the
/// daemon's check is exact-length set equality, so a blind retry against a
/// stale projection is a coin flip that turns one refusal into a contention
/// source. Re-reading surfaces the truth — usually another frontend's mutation
/// — and the user's next `[`/`]` is built from the corrected order.
pub fn apply_queue_reorder_finished(
    app: &mut App,
    request_id: u64,
    session_id: String,
    entries: Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    code: Option<String>,
    error: Option<String>,
) {
    match apply_refusal(
        &mut app.dialog_state.queue_reorder_request,
        request_id,
        &code,
        &error,
    ) {
        RefusalOutcome::Stale => return,
        RefusalOutcome::Refused { message, .. } => {
            app.messages_state.toasts.warning(&message);
            if code.as_deref() == Some(ORDER_MISMATCH_CODE) {
                refresh_queue_box(app);
            }
            return;
        }
        RefusalOutcome::Applied => {}
    }

    adopt_queue_projection(app, session_id, entries);
}

/// M006: ask the daemon to hand one queued prompt to the running turn.
///
/// The request names the entry and nothing else — no text, no turn id. The
/// daemon loads the stored prompt and resolves the active turn itself, so this
/// keystroke cannot send text that no queue entry accounts for, and cannot steer
/// a turn that ended a moment ago.
pub fn start_queue_send_now(app: &mut App, entry_id: String) {
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .info("No session is bound, so there is nothing to send.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Prompt queue unavailable: no core connection");
        return;
    };

    let request_id = app.dialog_state.queue_send_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "user_prompt_queue_send_now",
        async move {
            let request = crate::core::new_request(
                format!("user-prompt-queue-send-now-{}", uuid::Uuid::new_v4()),
                CoreRequest::UserPromptQueueSendNow {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                },
            );
            let (entries, outcome, error) = send_now_answer(core_client.request(request).await);
            Some(TuiCommand::UserPromptQueueSendNowFinished {
                request_id,
                session_id,
                entries,
                outcome,
                error,
            })
        },
    );
}

/// Apply a send-now completion.
///
/// The projection is adopted on **every** outcome, including the refusals: the
/// daemon attaches it precisely because a refusal may mean another frontend
/// already moved or removed the row, and leaving the box showing a row the
/// daemon no longer holds is how a user ends up pressing a key for a prompt
/// that is no longer there.
///
/// No retry. Every refusal here is a fact about the turn — full, closing,
/// unsupported, or not yours — and retrying a request whose answer is
/// "definitely no" is how one refusal becomes a contention source.
pub fn apply_queue_send_now_finished(
    app: &mut App,
    request_id: u64,
    session_id: String,
    entries: Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    outcome: Option<crate::protocol::core::UserPromptQueueSendNowResult>,
    error: Option<String>,
) {
    // `code` is absent by design: a send-now refusal arrives *inside* a
    // successful response as an outcome, not as a wire error. This shared path
    // therefore only decides whether the completion is still current.
    match apply_refusal(
        &mut app.dialog_state.queue_send_request,
        request_id,
        &None,
        &error,
    ) {
        RefusalOutcome::Stale => return,
        RefusalOutcome::Refused { message, .. } => {
            // Prefer the daemon's own outcome wording when the response carried
            // one — it names the turn's condition, which a transport error
            // cannot.
            let message = match outcome {
                Some(ref outcome) => {
                    send_now_outcome_message(outcome, SteerConsequence::DurableQueue).0
                }
                None => message,
            };
            app.messages_state.toasts.warning(&message);
        }
        RefusalOutcome::Applied => {
            if let Some(outcome) = outcome {
                let (message, tone) =
                    send_now_outcome_message(&outcome, SteerConsequence::DurableQueue);
                match tone {
                    SendNowTone::Info => app.messages_state.toasts.info(&message),
                    SendNowTone::Warning => app.messages_state.toasts.warning(&message),
                }
            }
        }
    }

    adopt_queue_projection(app, session_id, entries);
}

/// Unpack a send-now response into `(projection, outcome, error)`.
///
/// The projection rides along with every outcome, so this cannot report a
/// refusal without also reporting what the daemon currently believes the queue
/// holds.
fn send_now_answer(
    answer: Result<CoreResponse, crate::error::AppError>,
) -> (
    Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    Option<crate::protocol::core::UserPromptQueueSendNowResult>,
    Option<String>,
) {
    match answer {
        Ok(CoreResponse::UserPromptQueueSendNowOutcome {
            entries, outcome, ..
        }) => (entries, Some(outcome), None),
        Ok(CoreResponse::Error { code, message }) => (
            Vec::new(),
            Some(crate::protocol::core::UserPromptQueueSendNowResult::Refused { code }),
            Some(message),
        ),
        Ok(_) => (
            Vec::new(),
            None,
            Some("The queue got an unexpected response".to_string()),
        ),
        Err(error) => (Vec::new(), None, Some(error.to_string())),
    }
}

/// What a mutation completion turned out to be.
///
/// Three states, not two. Collapsing "success" and "stale/cancelled" into a
/// single `None` is a live bug: the caller then treats a completion that
/// arrived after `close_dialog` cancelled it as a success and writes its
/// projection anyway — exactly what the close-during-mutation guard exists to
/// prevent. The distinction is the whole point of the request generation.
pub(super) enum RefusalOutcome {
    /// Current, and it was a success. Apply the projection it returned.
    Applied,
    /// Current, and the daemon refused. `restore_text` says whether the user's
    /// words should go back into the composer.
    Refused { message: String, restore_text: bool },
    /// Superseded or cancelled. The caller must do **nothing at all** — no
    /// projection write, no toast, no selection change.
    Stale,
}

pub(super) fn apply_refusal(
    state: &mut crate::tui::app::state::AsyncUiRequestState,
    request_id: u64,
    code: &Option<String>,
    error: &Option<String>,
) -> RefusalOutcome {
    if let Some(error) = error {
        if !state.fail(request_id, error.clone()) {
            return RefusalOutcome::Stale;
        }
        let message = match code {
            Some(code) => refusal_message(code),
            None => format!("That change did not apply ({error})."),
        };
        let restore_text = code.as_deref().is_some_and(refusal_keeps_text);
        return RefusalOutcome::Refused {
            message,
            restore_text,
        };
    }
    if !state.finish(request_id) {
        return RefusalOutcome::Stale;
    }
    RefusalOutcome::Applied
}

/// Unpack a mutation response into `(projection, code, error)`.
///
/// One place so the three paths cannot disagree about what a refusal looks
/// like — which is how M004's first draft shipped a wire code that did not
/// exist.
fn queue_mutation_answer(
    answer: Result<CoreResponse, crate::error::AppError>,
) -> (
    Vec<crate::protocol::core::UserPromptQueueEntryDto>,
    Option<String>,
    Option<String>,
) {
    match answer {
        // The projection this very write produced. Applied directly — no
        // second read.
        Ok(CoreResponse::UserPromptQueue { entries, .. }) => (entries, None, None),
        Ok(CoreResponse::Error { code, .. }) => (Vec::new(), Some(code.clone()), Some(code)),
        Ok(_) => (
            Vec::new(),
            None,
            Some("The queue got an unexpected response".to_string()),
        ),
        Err(error) => (Vec::new(), None, Some(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::session::user_prompt_queue::UserPromptQueueError;

    /// Every code this module maps is pinned against the daemon's real
    /// `code()`. These are literals on purpose — the TUI must not import the
    /// daemon's error enum — so the pin is the only thing standing between a
    /// rename and a refusal that silently degrades to the generic sentence.
    ///
    /// This is not hypothetical: the first draft of this mapping carried
    /// `user_prompt_queue_entry_expired`, which **does not exist**. The `entry_`
    /// prefix came from the neighbouring `entry_delivering` code, and without
    /// this assertion the expired refusal would have rendered "The queue
    /// refused that change (user_prompt_queue_entry_expired)" forever.
    #[test]
    fn user_prompt_queue_every_mapped_code_matches_the_daemon() {
        let pinned = [
            (UserPromptQueueError::Delivering, DELIVERING_CODE),
            (UserPromptQueueError::Expired, EXPIRED_CODE),
            (UserPromptQueueError::NotFound, NOT_FOUND_CODE),
            (UserPromptQueueError::NotQueueOwner, NOT_OWNER_CODE),
            (UserPromptQueueError::QueueUnowned, UNOWNED_CODE),
            (
                UserPromptQueueError::OwnerRevisionConflict {
                    expected: 1,
                    current: 2,
                },
                OWNER_REVISION_CODE,
            ),
            (UserPromptQueueError::OrderMismatch, ORDER_MISMATCH_CODE),
            (
                UserPromptQueueError::TooManyEntries { limit: 5 },
                TOO_MANY_ENTRIES_CODE,
            ),
            (
                UserPromptQueueError::EntryTooLarge { limit: 5 },
                ENTRY_TOO_LARGE_CODE,
            ),
            (
                UserPromptQueueError::Invalid {
                    field: "text",
                    message: "blank".to_string(),
                },
                INVALID_CODE,
            ),
        ];
        for (error, expected) in pinned {
            assert_eq!(
                error.code(),
                expected,
                "the TUI matches `{expected}` as a literal; if the daemon renames \
                 it, this refusal silently degrades to the generic sentence"
            );
            // Every pinned code must also have its own sentence, or the mapping
            // is dead code that will never run.
            let message = refusal_message(expected);
            assert!(
                !message.starts_with("The queue refused that change"),
                "`{expected}` has no mapped sentence: {message:?}"
            );
        }
    }

    /// Non-vacuity for the pin above: the codes the TUI does *not* map fall
    /// through, and must still produce an honest sentence rather than nothing.
    ///
    /// M005 mapped `order_mismatch`, `too_many_entries`, `entry_too_large`, and
    /// `invalid`, so this list shrank when it did. What is left must still be
    /// reported verbatim — a code this build has never heard of is the case
    /// that matters, because it is the one that will arrive after a daemon
    /// upgrade.
    #[test]
    fn user_prompt_queue_unmapped_codes_are_reported_verbatim() {
        for code in [
            // `Storage` is the one daemon code this build never maps to a
            // sentence: it is an internal fault, not a decision about the
            // user's prompt.
            "user_prompt_queue_storage_error",
            "some_future_queue_code",
        ] {
            let message = refusal_message(code);
            assert!(
                message.contains(code),
                "an unmapped code must still be reported verbatim: {message:?}"
            );
        }
    }

    /// Each refusal keeps its own sentence. Collapsing `delivering` into
    /// `expired` would tell the user their prompt expired at the moment a turn
    /// started.
    #[test]
    fn user_prompt_queue_refusals_do_not_share_a_sentence() {
        let messages: Vec<String> = [
            DELIVERING_CODE,
            "user_prompt_queue_entry_expired",
            "user_prompt_queue_not_found",
            "user_prompt_queue_not_owner",
        ]
        .iter()
        .map(|code| refusal_message(code))
        .collect();
        for (i, a) in messages.iter().enumerate() {
            for (j, b) in messages.iter().enumerate() {
                assert!(
                    i == j || a != b,
                    "{a:?} and {b:?} are the same sentence, so one refusal is \
                     indistinguishable from another"
                );
            }
        }
    }

    #[test]
    fn user_prompt_queue_delivering_refusal_names_the_condition() {
        let message = refusal_message(DELIVERING_CODE);
        assert!(
            message.to_lowercase().contains("deliver"),
            "the refusal must name the condition: {message:?}"
        );
        assert!(
            !message.to_lowercase().contains("expired"),
            "a delivering entry has not expired: {message:?}"
        );
    }

    /// An unmapped code must still produce an honest message rather than
    /// looking like a success.
    #[test]
    fn user_prompt_queue_unknown_code_still_produces_a_message() {
        let message = refusal_message("some_future_code");
        assert!(message.contains("some_future_code"), "got {message:?}");
    }
}
