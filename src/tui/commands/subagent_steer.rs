//! C007: hand the composer an addressed reply to the turn that owns a subagent.
//!
//! # What this sends, and to whom
//!
//! One request: [`CoreRequest::TurnSteer`] carrying the **parent session's**
//! `turn_id` — never a subagent's `task_id` and never its `run_id`.
//! `TurnSteer` is keyed on a *session turn* (`daemon_turns.rs`: the runtime is
//! looked up by `session_id` and the handle's `turn_id` must equal the one
//! sent), so naming a subagent's daemon-local `u64` `task_id` here would be a
//! type-shaped lie that only ever resolves to `turn_id_mismatch`.
//!
//! The subagent is not the recipient. ADR-0041 *Indirect*: the frontend steers
//! the controlling parent turn, and the parent decides whether to relay. Every
//! sentence this module produces names that parent, never the subagent.
//!
//! # Why there is no transcript echo
//!
//! A steer is not stored as a session message. The daemon hands the text to the
//! loop's steering channel (`pending_steer`), which injects it into the
//! in-flight provider transcript and nothing else — there is no row in the
//! message store. So the TUI does **not** push a user message into the
//! transcript: it would render as durable on screen and vanish on reload or
//! export, which is the kind of divergence the projection rules exist to
//! prevent. The record is the toast, and it says only what was accepted.

use crate::protocol::core::CoreRequest;
use crate::protocol::core::CoreResponse;
use crate::tui::app::state::AsyncUiRequestState;
use crate::tui::app::state::SubagentAddressee;
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::commands::queue::{
    apply_refusal, send_now_outcome_message, RefusalOutcome, SteerConsequence,
};
use crate::tui::task_lifecycle::TuiTaskKind;

/// Arm the composer to address `task_id`'s run, if that run is still live.
///
/// Returns `false` when nothing could be armed, having already said why in the
/// composer badge terms the user can act on. This is the arming half of the
/// `/subagent-send` command; binding no new key is deliberate (C007 §5), so the
/// gesture is a command the user is already typing rather than a chord they have
/// to learn.
pub fn arm_addressee(app: &mut App, task_id: &str) -> bool {
    let row = app
        .ui_state
        .subagents
        .rows
        .iter()
        .find(|row| row.task_id == task_id)
        .cloned();
    let Some(row) = row else {
        app.messages_state
            .toasts
            .warning("That subagent is no longer in the tree.");
        return false;
    };
    let Some(addressee) = SubagentAddressee::from_row(&row) else {
        app.messages_state
            .toasts
            .warning("That subagent has finished, so there is nothing running to address.");
        return false;
    };
    adopt(app, addressee);
    true
}

/// Arm the composer from the focused subagent tab.
///
/// Tab-scoped on purpose: the tab is the surface that already shows one run's
/// detail, and a tab can be detached, so this refuses rather than addressing a
/// run the projection has stopped vouching for.
pub fn arm_addressee_from_focused_tab(app: &mut App) -> bool {
    let Some(tab) = app.ui_state.subagents.active_tab().cloned() else {
        app.messages_state
            .toasts
            .warning("Open a subagent tab first — there is no focused run.");
        return false;
    };
    let Some(addressee) = SubagentAddressee::from_tab(&tab) else {
        app.messages_state.toasts.warning(
            "That subagent has finished or left the tree, so there is nothing running to \
             address.",
        );
        return false;
    };
    adopt(app, addressee);
    true
}

/// Arm the composer and say so.
///
/// One place, because the sentence is the contract: it names the **parent** as
/// the recipient, and a second copy would be a second thing to keep honest when
/// ADR-0041's failure modes are exactly the ones this wording exists to avoid.
fn adopt(app: &mut App, addressee: SubagentAddressee) {
    let name = addressee.label().to_string();
    app.prompt_state.subagent_addressee = Some(addressee);
    app.messages_state.toasts.info(&format!(
        "Your next prompt goes to the agent running this turn, addressed to {name}."
    ));
}

/// `/subagent-send [off | <name or task id>]`.
///
/// Three forms, one rule: the run must still be live.
///
/// * `off` clears the addressee.
/// * bare uses the focused subagent tab, so the user never has to type a name.
/// * anything else matches a row by `task_id` first and display name second.
///   `task_id` wins because names are drawn from a curated pool and are not
///   guaranteed unique across concurrent runs, while a `task_id` is.
///
/// Matching by name is a convenience, not an addressing mechanism: the matched
/// row's `task_id` is what gets captured, so a name collision cannot make one
/// run's composer claim to address another. An ambiguous name refuses rather
/// than picking, because silently choosing one of two runs the user cannot tell
/// apart is the failure this whole milestone exists to avoid.
pub fn dispatch_addressee_command(app: &mut App, raw_input: Option<&str>) {
    // `raw_input` is the whole typed line, so the command name is stripped
    // first — the same shape `/skills` reads its query with.
    let argument = raw_input
        .unwrap_or_default()
        .trim()
        .strip_prefix("/subagent-send")
        .unwrap_or_default()
        .trim();
    let argument = (!argument.is_empty()).then_some(argument);
    match argument {
        Some(value) if value.eq_ignore_ascii_case("off") => {
            disarm_addressee(app);
        }
        None => {
            arm_addressee_from_focused_tab(app);
        }
        Some(value) => match find_row(app, value) {
            AddresseeMatch::Row(task_id) => {
                arm_addressee(app, &task_id);
            }
            AddresseeMatch::Ambiguous(names) => {
                app.messages_state.toasts.warning(&format!(
                    "{value} matches {} runs ({}). Use the task id so the address is exact.",
                    names.len(),
                    names.join(", ")
                ));
            }
            AddresseeMatch::None => {
                app.messages_state.toasts.warning(&format!(
                    "No running subagent called {value} in this session."
                ));
            }
        },
    }
}

/// How a typed argument resolved against the projected rows.
enum AddresseeMatch {
    /// Exactly one live run matched; carries its `task_id`.
    Row(String),
    /// Several live runs share the name, so the user must disambiguate.
    Ambiguous(Vec<String>),
    /// Nothing matched.
    None,
}

fn find_row(app: &App, value: &str) -> AddresseeMatch {
    let rows = &app.ui_state.subagents.rows;
    // A task id identifies one run by construction, so it is checked first and
    // does not participate in the ambiguity rule below.
    if let Some(row) = rows.iter().find(|row| row.task_id == value) {
        return AddresseeMatch::Row(row.task_id.clone());
    }
    let mut matched: Vec<&crate::tui::app::state::SubagentRow> =
        rows.iter().filter(|row| row.label() == value).collect();
    match matched.len() {
        0 => AddresseeMatch::None,
        1 => AddresseeMatch::Row(matched[0].task_id.clone()),
        _ => {
            matched.sort_by(|a, b| a.task_id.cmp(&b.task_id));
            AddresseeMatch::Ambiguous(matched.iter().map(|row| row.task_id.clone()).collect())
        }
    }
}

/// Drop the addressee, restoring the ordinary `TurnSubmit` path.
///
/// Exposed as a command so a user who armed by mistake is never stuck: the
/// badge is visible, but "undo it" must not require knowing which chord clears
/// it.
pub fn disarm_addressee(app: &mut App) {
    if app.prompt_state.subagent_addressee.take().is_some() {
        app.messages_state
            .toasts
            .info("Subagent addressee cleared — your next prompt starts a normal turn.");
    }
}

/// Hand one addressed reply to the controlling parent turn.
///
/// `body` is the text the user typed, unmodified. This prepends the addressee
/// header; it never edits, trims into, or reinterprets the words.
///
/// Refuses **without** falling back to `TurnSubmit`. That is the whole safety
/// property of the milestone: an armed composer whose steer is refused must not
/// silently start a *new* turn with a reply written for a subagent, because the
/// user would see a normal-looking submitted prompt and have no way to tell that
/// their address went nowhere.
pub fn start_subagent_reply(app: &mut App, body: String) {
    let Some(addressee) = app.prompt_state.subagent_addressee.clone() else {
        // Unreachable through `send_prompt`, which only calls this while armed.
        // Reported rather than ignored so a future caller that reaches it
        // unarmed gets a sentence instead of a silent `TurnSubmit`.
        app.messages_state
            .toasts
            .warning("No subagent is addressed, so that starts a normal turn instead.");
        return;
    };
    let Some(session_id) = app.active_session_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .warning("No session is bound, so there is nothing to steer.");
        return;
    };
    // Read the turn id *now*, at send time, not at arm time. A turn that ended
    // and was replaced between arming and Enter is the ordinary case, not a
    // race worth guarding: reading it fresh means the steer names whatever is
    // genuinely running, and only a turn that ends in the window between this
    // read and the daemon's own check can still come back `turn_id_mismatch`.
    let Some(turn_id) = app.session_state.active_turn_id.clone() else {
        app.messages_state.toasts.warning(
            "Nothing is running to steer — a reply to a subagent needs the turn that owns \
             it.",
        );
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Could not reach the agent: no core connection");
        return;
    };

    // Cleared here, **after** every refusal above, and not by the caller. A
    // bare addressed reply has no durable queue behind it, so a cleared
    // composer on a refused send is a user's words destroyed with no copy
    // anywhere. The refusal toasts say why nothing happened and the text stays
    // put, which is the only place it can be recovered from.
    app.prompt_state.prompt.clear();
    app.prompt_state.show_completions = false;

    let request_id = app.prompt_state.subagent_steer_request.begin();
    let text = addressee.addressed_text(&body);
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "turn_steer_subagent_reply",
        async move {
            let request = crate::core::new_request(
                format!("turn-steer-subagent-{}", uuid::Uuid::new_v4()),
                CoreRequest::TurnSteer {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    text,
                },
            );
            let (code, error) = steer_answer(core_client.request(request).await, &turn_id);
            Some(TuiCommand::SubagentSteerFinished {
                request_id,
                addressee: addressee.label().to_string(),
                body,
                code,
                error,
            })
        },
    );
}

/// Apply a steer completion.
///
/// On refusal the user's text goes back into the composer and the addressee
/// stays armed, so pressing Enter again re-reads the *then*-current turn id and
/// is a genuine retry rather than a resend to a turn that already ended. That
/// only holds while the run is live, which the arriving snapshot enforces
/// independently through
/// [`reconcile_addressee`](crate::tui::app::state::reconcile_addressee).
///
/// The completion is dropped entirely when it is stale — see
/// [`apply_refusal`](crate::tui::commands::queue::apply_refusal).
pub fn apply_subagent_steer_finished(
    app: &mut App,
    request_id: u64,
    addressee: String,
    body: String,
    code: Option<String>,
    error: Option<String>,
) {
    let outcome = apply_refusal(
        &mut app.prompt_state.subagent_steer_request,
        request_id,
        &code,
        &error,
    );
    if matches!(outcome, RefusalOutcome::Stale) {
        return;
    }
    // A refusal restores the text whatever `refusal_keeps_text` thinks. That
    // rule is about a queue entry the box still holds; here nothing durable
    // holds a bare reply, so restoring is the only way the words are not lost.
    // Accepted does not restore: the turn took it, and a second copy would be
    // a duplicate instruction.
    if matches!(outcome, RefusalOutcome::Refused { .. }) {
        // Only into an **empty** composer. If the user has already started
        // typing something else, prepending or replacing would destroy that
        // instead; leaving their newer text is the smaller loss, and the toast
        // below tells them the reply is gone rather than silently relocating it.
        if app.prompt_state.prompt.get_text().trim().is_empty() && !body.trim().is_empty() {
            app.prompt_state.prompt.set_text(body);
        }
        let message = steer_outcome_message(&addressee, code.as_deref(), error.as_deref());
        app.messages_state.toasts.warning(&message);
        return;
    }
    let message = SteerConsequence::ComposerOnly.accepted(Some(&addressee));
    app.messages_state.toasts.info(&message);
}

/// The sentence for what the daemon did with an addressed reply.
///
/// Deliberately routed through the queue's own `send_now_outcome_message` with
/// [`SteerConsequence::ComposerOnly`] rather than given a second table. The two
/// paths push into the same channel on the same turn and are refused with the
/// same codes, so a private mapping here would be a second place to keep in
/// step — and the plan for this milestone treats a second, thinner vocabulary as
/// the likely defect rather than the fix.
fn steer_outcome_message(addressee: &str, code: Option<&str>, error: Option<&str>) -> String {
    if let Some(code) = code {
        let outcome = crate::protocol::core::UserPromptQueueSendNowResult::Refused {
            code: code.to_string(),
        };
        return send_now_outcome_message(&outcome, SteerConsequence::ComposerOnly).0;
    }
    match error {
        Some(error) => format!(
            "That reply did not reach the turn ({error}). Your text is still \
             in the composer."
        ),
        None => format!(
            "That reply to {addressee} was not accepted. Your text is still in the \
             composer."
        ),
    }
}

/// Unpack a `TurnSteer` response into `(code, error)`.
///
/// `TurnSteer` answers `CoreResponse::Ack` or `CoreResponse::Error`, so the
/// accepted path carries no payload at all. A refusal keeps its **code** and
/// drops the daemon's prose on purpose: the prose interpolates the requested and
/// active turn ids, and the frontend's sentence must not quote identifiers the
/// user never typed.
fn steer_answer(
    answer: Result<CoreResponse, crate::error::AppError>,
    requested_turn_id: &str,
) -> (Option<String>, Option<String>) {
    match answer {
        Ok(CoreResponse::Ack) => (None, None),
        Ok(CoreResponse::Error { code, message }) => {
            // `turn_id_mismatch` is the only code whose daemon prose carries
            // both ids. Replace it with a placeholder-free variant so a
            // transport-level echo can never reach a toast.
            let message = if message.contains(requested_turn_id) {
                "The turn changed before the reply reached it.".to_string()
            } else {
                message
            };
            (Some(code), Some(message))
        }
        Ok(_) => (
            None,
            Some("That reply got an unexpected response".to_string()),
        ),
        Err(error) => (None, Some(error.to_string())),
    }
}

/// Whether a steer completion is still the current one.
///
/// Exposed for the integration tests, which need the same stale rule the
/// applier uses rather than a second copy of it.
pub fn is_current(state: &AsyncUiRequestState, request_id: u64) -> bool {
    state.is_current(request_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::commands::queue::{
        CONTROL_CONFLICT_CODE, NOT_CONTROLLER_CODE, NO_ACTIVE_TURN_CODE, SESSION_NOT_FOUND_CODE,
        STEER_NOT_SUPPORTED_CODE, STEER_QUEUE_FULL_CODE, STEER_UNAVAILABLE_CODE,
        TURN_ID_MISMATCH_CODE,
    };

    /// ADR-0041's *Failure modes*, checked as literal properties: the accepted
    /// sentence names the **parent** as recipient and never claims delivery to
    /// the subagent.
    #[test]
    fn subagent_steer_the_accepted_sentence_names_the_parent_not_the_subagent() {
        let sentence = SteerConsequence::ComposerOnly.accepted(Some("Sally"));
        assert!(
            sentence.contains("the agent running this turn"),
            "the parent must be named as the recipient: {sentence:?}"
        );
        assert!(
            !sentence.contains("delivered to Sally") && !sentence.contains("sent to Sally"),
            "the subagent is not the recipient: {sentence:?}"
        );
        for forbidden in ["force", "guarantee", "always reaches", "the model will"] {
            assert!(
                !sentence.contains(forbidden),
                "the sentence promised {forbidden:?}: {sentence:?}"
            );
        }
    }

    /// Every refusal the daemon can return gets a sentence that is distinct
    /// from every other's. Two codes sharing a sentence is the defect this
    /// milestone's reporting rule exists to prevent: a user told "sent" for a
    /// channel that was full has been lied to.
    #[test]
    fn subagent_steer_every_refusal_code_has_a_distinct_sentence() {
        // Grouped by **remedy**, not by code. Distinctness is the property
        // that matters — a user who cannot tell two outcomes apart cannot act on
        // either — but two codes that leave the user in the same position
        // deserve one sentence.
        //
        // `session_control_conflict` used to be grouped with
        // `session_control_not_controller`, on the reading that the two differ
        // only in *why* a foreign principal holds the turn. Reading
        // `check_turn_controller` (`src/core/daemon_control.rs`) says otherwise:
        // conflict is returned when `lease_turn != turn_id`, i.e. the lease
        // covers a **different turn**, and not-controller when a different
        // principal holds the lease for the one named. The first is a stale
        // turn — retrying against the current turn works; the second is not
        // this user's turn at all. Telling a conflict "another frontend has it"
        // sends the user looking for a frontend that may not exist. They group
        // with `turn_id_mismatch`, which is the same fact seen at the runtime
        // instead of the control plane.
        let groups: &[&[&str]] = &[
            &[STEER_QUEUE_FULL_CODE],
            &[STEER_UNAVAILABLE_CODE],
            &[STEER_NOT_SUPPORTED_CODE],
            &[TURN_ID_MISMATCH_CODE, CONTROL_CONFLICT_CODE],
            &[NO_ACTIVE_TURN_CODE],
            &[NOT_CONTROLLER_CODE],
            &[SESSION_NOT_FOUND_CODE],
        ];
        let mut seen: Vec<String> = Vec::new();
        for group in groups {
            let sentences: Vec<String> = group
                .iter()
                .map(|code| steer_outcome_message("Sally", Some(code), Some(code)))
                .collect();
            for sentence in &sentences {
                assert!(
                    !seen.contains(sentence),
                    "two refusal remedies share one sentence: {sentence:?}"
                );
            }
            // Within a group the sentence is one, not merely similar.
            for sentence in &sentences {
                assert_eq!(sentence, &sentences[0], "a grouped remedy must agree");
            }
            seen.extend(sentences);
        }
    }

    /// The codes whose refusal is about the *message* must say where the words
    /// went, because a bare addressed reply has no other home.
    ///
    /// Scoped deliberately: `session_control_not_controller` is excluded, and
    /// for a reason worth stating. Its remedy is another frontend, not a
    /// retry, so "it is still in your composer" would invite an action that
    /// cannot work. Every *other* code leaves the text with the user and says so
    /// — including `session_control_conflict`, which is a stale turn rather
    /// than a foreign one and is retryable against the current turn.
    #[test]
    fn subagent_steer_a_refused_message_names_where_the_text_survived() {
        for code in [
            STEER_QUEUE_FULL_CODE,
            STEER_UNAVAILABLE_CODE,
            STEER_NOT_SUPPORTED_CODE,
            TURN_ID_MISMATCH_CODE,
            CONTROL_CONFLICT_CODE,
            NO_ACTIVE_TURN_CODE,
        ] {
            let sentence = steer_outcome_message("Sally", Some(code), Some(code));
            assert!(
                sentence.contains("composer"),
                "a refused reply names where the text survived ({code}): {sentence:?}"
            );
        }
    }

    /// The exact regression: a stale-turn conflict must not be dressed up as a
    /// foreign frontend. Only `not_controller` earns that sentence.
    #[test]
    fn subagent_steer_a_stale_turn_conflict_is_not_reported_as_a_foreign_frontend() {
        let sentence = steer_outcome_message("Sally", Some(CONTROL_CONFLICT_CODE), None);
        assert!(
            !sentence.contains("another frontend"),
            "the lease covers a different turn, not a different frontend: {sentence:?}"
        );
        assert_ne!(
            sentence,
            steer_outcome_message("Sally", Some(NOT_CONTROLLER_CODE), None),
            "the two control-plane refusals must not read identically"
        );
    }

    /// The controller denial is the one refusal whose remedy is *not* a retry,
    /// so it must not tell the user to try again from here.
    #[test]
    fn subagent_steer_a_controller_denial_names_the_other_frontend_instead() {
        let sentence = steer_outcome_message("Sally", Some(NOT_CONTROLLER_CODE), None);
        assert!(sentence.contains("another frontend"), "{sentence:?}");
        assert!(
            !sentence.contains("try again"),
            "retrying a turn another frontend controls cannot work: {sentence:?}"
        );
    }

    /// A refused addressed reply has no durable queue behind it, so its
    /// sentence must never promise a resend that nothing will perform.
    #[test]
    fn subagent_steer_a_refusal_never_claims_the_text_is_still_queued() {
        for code in [
            STEER_QUEUE_FULL_CODE,
            STEER_UNAVAILABLE_CODE,
            STEER_NOT_SUPPORTED_CODE,
            TURN_ID_MISMATCH_CODE,
            CONTROL_CONFLICT_CODE,
            NO_ACTIVE_TURN_CODE,
        ] {
            let sentence = steer_outcome_message("Sally", Some(code), Some(code));
            assert!(
                !sentence.contains("stays queued") && !sentence.contains("stayed queued"),
                "{code} promised a queue the addressed path does not use: {sentence:?}"
            );
        }
    }

    /// The same code must read differently for a queued prompt and for a bare
    /// addressed reply — that difference is the whole reason
    /// `SteerConsequence` exists.
    #[test]
    fn subagent_steer_the_same_code_reads_differently_for_the_two_senders() {
        let queued = send_now_outcome_message(
            &crate::protocol::core::UserPromptQueueSendNowResult::Refused {
                code: STEER_QUEUE_FULL_CODE.to_string(),
            },
            SteerConsequence::DurableQueue,
        )
        .0;
        let addressed = steer_outcome_message("Sally", Some(STEER_QUEUE_FULL_CODE), None);
        assert_ne!(queued, addressed);
        assert!(queued.contains("stays queued"));
        assert!(addressed.contains("composer"));
    }

    /// A transport failure is not a daemon refusal, so it must not borrow a
    /// refusal sentence — and it must still say the words survived.
    #[test]
    fn subagent_steer_a_transport_failure_is_not_reported_as_a_daemon_refusal() {
        let sentence = steer_outcome_message("Sally", None, Some("socket closed"));
        assert!(sentence.contains("socket closed"), "{sentence:?}");
        assert!(sentence.contains("composer"), "{sentence:?}");
        assert!(!sentence.contains("stays queued"), "{sentence:?}");
    }

    /// The daemon's `turn_id_mismatch` prose interpolates both turn ids. The
    /// frontend must not echo an identifier the user never typed into a toast.
    #[test]
    fn subagent_steer_a_refusal_prose_quoting_the_turn_id_is_replaced() {
        let (code, error) = steer_answer(
            Ok(CoreResponse::Error {
                code: TURN_ID_MISMATCH_CODE.to_string(),
                message: "Requested turn_id 'turn-9' does not match active turn_id 'turn-8'"
                    .to_string(),
            }),
            "turn-9",
        );
        assert_eq!(code.as_deref(), Some(TURN_ID_MISMATCH_CODE));
        let error = error.expect("carries a message");
        assert!(
            !error.contains("turn-9") && !error.contains("turn-8"),
            "a turn id leaked into the sentence: {error:?}"
        );
    }

    /// The ack path must not invent a refusal.
    #[test]
    fn subagent_steer_an_ack_is_accepted_with_no_code() {
        let (code, error) = steer_answer(Ok(CoreResponse::Ack), "turn-9");
        assert!(code.is_none());
        assert!(error.is_none());
    }
}
