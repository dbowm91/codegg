//! Typed classification of one `turn.submit` outcome (user prompt queue M001).
//!
//! The daemon already distinguishes why a submit was refused, and the frontend
//! used to flatten every refusal into one formatted string that was handed to
//! the same teardown as a submit that genuinely never started a turn. That is
//! not a cosmetic difference:
//!
//! * [`TurnSubmitOutcome::Contended`] means a turn **is** running on the
//!   daemon. Tearing down active-turn state there discarded the identity and
//!   elapsed age of a live turn and stopped the spinner, so the UI asserted
//!   the opposite of the truth at exactly the moment the user was waiting.
//! * [`TurnSubmitOutcome::TransportFailure`] means the connection is gone. It
//!   must never be reported as contention: the user reads the code to decide
//!   between "wait for the turn" and "check the connection".
//!
//! Classification happens once, at the single point where the daemon response
//! or transport error is interpreted, and the resulting value travels to the
//! completion arm as data. The completion arm no longer parses strings.
//!
//! ## Why an unknown code is a rejection, not "unexpected"
//!
//! [`classify`] is total: every possible result maps to exactly one variant.
//! An error code this module has never heard of maps to
//! [`TurnSubmitOutcome::Rejected`], deliberately, because that is the only
//! class that keeps the user's text. The daemon adds refusal codes over time;
//! routing an unrecognised one to a class that refuses to restore the prompt
//! would lose the user's words the first time a new refusal reason appeared.
//! Contention is therefore never a default — it is reached only through the
//! explicit code table below. [`TurnSubmitOutcome::Unexpected`] is reserved
//! for a response *shape* this frontend cannot interpret, which is a
//! genuinely different condition: there the submit may have been accepted, so
//! the frontend must not offer a resubmit.
//!
//! ## Not the same "queue"
//!
//! [`TurnSubmitOutcome`] is about one submission's fate. It is unrelated to
//! the agent-loop's `drain_follow_up`, which is an agent-internal,
//! turn-boundary prompt drain, and unrelated to any durable user prompt queue
//! (roadmap M002+). Neither of those exists on this path yet.

use crate::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};

/// Daemon error codes that mean "a turn is already active for this session".
///
/// `turn_already_active` is returned from two distinct places in
/// `CoreRequest::TurnSubmit` — the in-memory active-turn collision and the
/// durable controller-lease conflict — and the frontend cannot tell them
/// apart. It does not need to: both mean a turn is running and this submit
/// must not touch turn state.
///
/// This is a deliberate allowlist, not a prefix match. A code that merely
/// *sounds* like contention (`turn_pending`, `busy`) is a rejection until the
/// daemon actually documents it as contention.
const CONTENTION_CODES: &[&str] = &["turn_already_active"];

/// What the daemon (or the transport) did with one turn submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnSubmitOutcome {
    /// The daemon accepted the submission (receipt or plain acknowledgement).
    Accepted,
    /// A turn is already active for this session; this submit was refused.
    ///
    /// The running turn is untouched. The submission is recoverable: the user
    /// can resubmit once the active turn ends.
    Contended { code: String, message: String },
    /// The daemon refused for some reason other than an active turn: a bad
    /// agent index, an unresolvable provider, a controller-lease conflict, an
    /// expired staged promotion. Recoverable — the text comes back.
    Rejected { code: String, message: String },
    /// The request never produced a usable answer because the transport failed.
    ///
    /// Distinct from a refusal: the daemon may or may not have seen it, so the
    /// report must name a connection problem rather than a daemon decision.
    TransportFailure { message: String },
    /// The transport succeeded but returned something this frontend cannot
    /// interpret as a submit result.
    ///
    /// The submit may have been accepted, so the prompt is deliberately **not**
    /// restored: offering a resubmit here is how one prompt becomes two turns.
    Unexpected { detail: String },
}

impl TurnSubmitOutcome {
    /// Whether this outcome means no turn was started by this submission.
    pub fn is_failure(&self) -> bool {
        !matches!(self, Self::Accepted)
    }

    /// Whether an in-flight turn must be left exactly as the frontend found it.
    ///
    /// True only for contention. Every other class either started no turn or
    /// is too uncertain to assert that a turn exists.
    pub fn preserves_active_turn(&self) -> bool {
        matches!(self, Self::Contended { .. })
    }

    /// Whether the submitted prompt should be put back in the composer.
    ///
    /// Every class except [`TurnSubmitOutcome::Unexpected`]: an uninterpreted
    /// response may already have started the turn.
    pub fn restores_prompt(&self) -> bool {
        !matches!(self, Self::Accepted | Self::Unexpected { .. })
    }

    /// Whether this outcome is a turn failure for latched-reporting purposes.
    ///
    /// Contention is excluded: the turn is still running, so latching an error
    /// on the row above the composer would replace the live turn's status with
    /// a stale `✗` line (see `App::situational_hints`'s documented
    /// precedence). The refusal is reported as a toast instead, which is
    /// transient by design and is retained in `/logs`.
    pub fn reports_as_turn_failure(&self) -> bool {
        matches!(
            self,
            Self::Rejected { .. } | Self::TransportFailure { .. } | Self::Unexpected { .. }
        )
    }

    /// One user-facing line naming what happened to the submit.
    pub fn failure_text(&self) -> Option<String> {
        match self {
            Self::Accepted => None,
            Self::Contended { message, .. } => Some(format!("Turn not submitted: {message}")),
            Self::Rejected { code, message } => {
                Some(format!("Turn not submitted: {code}: {message}"))
            }
            Self::TransportFailure { message } => Some(format!(
                "Turn not submitted: the connection to the core failed ({message})"
            )),
            Self::Unexpected { detail } => Some(format!(
                "Turn submit returned an unexpected response: {detail}"
            )),
        }
    }
}

/// Classify one submitted request's result.
///
/// Total over `Result<CoreResponse, AppError>`: every possible value maps to
/// exactly one [`TurnSubmitOutcome`], and an unrecognised daemon error code
/// becomes [`TurnSubmitOutcome::Rejected`] rather than contention.
pub fn classify(result: Result<CoreResponse, crate::error::AppError>) -> TurnSubmitOutcome {
    match result {
        Ok(CoreResponse::Ack) | Ok(CoreResponse::TurnAdmissionReceipt { .. }) => {
            TurnSubmitOutcome::Accepted
        }
        Ok(CoreResponse::Error { code, message }) => {
            if CONTENTION_CODES.contains(&code.as_str()) {
                TurnSubmitOutcome::Contended { code, message }
            } else {
                TurnSubmitOutcome::Rejected { code, message }
            }
        }
        Ok(other) => TurnSubmitOutcome::Unexpected {
            detail: response_variant_name(&other),
        },
        Err(error) => TurnSubmitOutcome::TransportFailure {
            message: error.to_string(),
        },
    }
}

/// Name a response variant without dumping its payload.
///
/// The submit path never needs the body of an unexpected response — only
/// enough to tell a user (and `/logs`) which shape arrived. Reading the
/// leading token of the debug form keeps this total over a large, growing
/// enum without listing every variant here, which is exactly what let the
/// previous string-formatting approach report whatever it happened to be
/// handed.
fn response_variant_name(response: &CoreResponse) -> String {
    let rendered = format!("{response:?}");
    rendered
        .split(['{', '(', ' '])
        .next()
        .unwrap_or("unknown")
        .to_string()
}

/// Submit a `turn.submit` and classify its result.
///
/// One entry point so the spawn site and the completion arm cannot drift: the
/// task body below is the only place that knows how a response is read.
pub async fn submit(
    client: std::sync::Arc<dyn crate::core::CoreClient>,
    request: RequestEnvelope<CoreRequest>,
) -> TurnSubmitOutcome {
    classify(client.request(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(code: &str) -> Result<CoreResponse, crate::error::AppError> {
        Ok(CoreResponse::Error {
            code: code.to_string(),
            message: format!("{code} happened"),
        })
    }

    /// Every `CoreResponse::Error` code `CoreRequest::TurnSubmit` can return.
    ///
    /// Sourced from the `TurnSubmit` arm in `src/core/daemon_turns.rs` plus the
    /// two `code()` ladders it delegates to
    /// (`SessionTurnAdmissionError`/`SessionControlError` and
    /// `HumanShellPromotionError`). If the daemon grows a code, this table is
    /// where it gets classified — the `unknown_*` cases below are the safety
    /// net for anything added without updating it.
    const TURN_SUBMIT_CODES: &[&str] = &[
        // TurnSubmit arm, direct.
        "turn_submission_identity_conflict",
        "invalid_agent_index",
        "connection_state",
        "invalid_model_router_config",
        "semantic_router_not_found",
        "provider_not_found",
        "session_unbound",
        "turn_asset_refresh_failed",
        // Contention: the in-memory active-turn collision and the durable
        // controller-lease conflict are both reported as this one code.
        "turn_already_active",
        // `SessionControlError::code()` reachable via the receipt lookup and
        // the promotion admission.
        "session_control_conflict",
        "session_control_not_found",
        "session_control_not_controller",
        "session_control_invalid_input",
        "session_control_unavailable",
        "session_control_storage_error",
        // `HumanShellPromotionError::code()`.
        "human_shell_promotion_invalid",
        "human_shell_promotion_identity_conflict",
        "human_shell_promotion_queue_full",
        "human_shell_promotion_not_found",
        "human_shell_promotion_expired",
        "human_shell_promotion_consumed",
        "human_shell_promotion_stale_binding",
        "human_shell_promotion_unavailable",
    ];

    #[test]
    fn only_the_active_turn_code_is_contention() {
        let contended: Vec<&str> = TURN_SUBMIT_CODES
            .iter()
            .copied()
            .filter(|code| matches!(classify(err(code)), TurnSubmitOutcome::Contended { .. }))
            .collect();
        assert_eq!(
            contended,
            vec!["turn_already_active"],
            "exactly one daemon code may preserve active-turn state"
        );
    }

    #[test]
    fn every_known_daemon_code_classifies_as_a_typed_outcome() {
        for code in TURN_SUBMIT_CODES {
            let outcome = classify(err(code));
            assert!(
                outcome.is_failure(),
                "{code} must not read as an accepted submit"
            );
            assert!(
                !matches!(outcome, TurnSubmitOutcome::Accepted),
                "{code} must not default into success"
            );
        }
    }

    #[test]
    fn an_unknown_code_is_a_rejection_not_contention() {
        let outcome = classify(err("turn_scheduler_saturated"));
        assert!(
            matches!(outcome, TurnSubmitOutcome::Rejected { .. }),
            "an unrecognised refusal reason must behave like any other \
             recoverable rejection, got {outcome:?}"
        );
        assert!(!outcome.preserves_active_turn());
        assert!(outcome.restores_prompt());
    }

    #[test]
    fn a_contention_sounding_code_is_not_contention_until_the_daemon_says_so() {
        // The allowlist is exact. A future `turn_pending` code must be
        // classified by its own decision, not by resemblance.
        let outcome = classify(err("turn_pending"));
        assert!(matches!(outcome, TurnSubmitOutcome::Rejected { .. }));
        assert!(!outcome.preserves_active_turn());
    }

    #[test]
    fn ack_and_receipt_are_accepted() {
        assert_eq!(classify(Ok(CoreResponse::Ack)), TurnSubmitOutcome::Accepted);
        assert_eq!(
            classify(Ok(CoreResponse::TurnAdmissionReceipt {
                receipt: crate::protocol::core::TurnAdmissionReceiptDto {
                    submission_id: "s".into(),
                    session_id: "x".into(),
                    turn_id: "t".into(),
                    status: "accepted".into(),
                    accepted_at_ms: 1,
                    updated_at_ms: 1,
                },
            })),
            TurnSubmitOutcome::Accepted
        );
        assert!(!classify(Ok(CoreResponse::Ack)).is_failure());
        assert!(classify(Ok(CoreResponse::Ack)).failure_text().is_none());
    }

    #[test]
    fn a_transport_failure_is_not_reported_as_contention() {
        let outcome = TurnSubmitOutcome::TransportFailure {
            message: "connection closed".into(),
        };
        assert!(!outcome.preserves_active_turn());
        assert_ne!(
            outcome,
            TurnSubmitOutcome::Contended {
                code: "turn_already_active".into(),
                message: "connection closed".into()
            }
        );
        let text = outcome.failure_text().expect("named failure");
        assert!(
            text.contains("connection"),
            "a lost connection must name the connection, got {text:?}"
        );
    }

    #[test]
    fn an_uninterpreted_response_is_reported_and_does_not_offer_a_resubmit() {
        let outcome = classify(Ok(CoreResponse::SessionList {
            sessions: Vec::new(),
        }));
        assert!(matches!(outcome, TurnSubmitOutcome::Unexpected { .. }));
        assert!(outcome.is_failure());
        // The submit may have been accepted; restoring would risk a second
        // turn for one prompt.
        assert!(
            !outcome.restores_prompt(),
            "an unexpected response must not restore the prompt"
        );
        let text = outcome.failure_text().expect("named failure");
        assert!(
            text.contains("SessionList"),
            "names the shape, got {text:?}"
        );
    }

    #[test]
    fn contention_is_not_a_latched_turn_failure() {
        // The working-strip status row gives `last_error` absolute precedence,
        // so latching here would hide the running turn's spinner and elapsed
        // time — the exact state corruption this milestone removes.
        let contended = TurnSubmitOutcome::Contended {
            code: "turn_already_active".into(),
            message: "A turn is already active for this session".into(),
        };
        assert!(!contended.reports_as_turn_failure());
        for rejected in [
            TurnSubmitOutcome::Rejected {
                code: "provider_not_found".into(),
                message: "x".into(),
            },
            TurnSubmitOutcome::TransportFailure {
                message: "x".into(),
            },
            TurnSubmitOutcome::Unexpected { detail: "x".into() },
        ] {
            assert!(
                rejected.reports_as_turn_failure(),
                "{rejected:?} must latch a turn failure"
            );
        }
    }

    #[test]
    fn failure_text_never_claims_contention_for_another_class() {
        let rejected = classify(err("provider_not_found"));
        let text = rejected.failure_text().expect("named failure");
        assert!(text.contains("provider_not_found"), "{text}");
        assert!(
            !text.contains("already active"),
            "a provider refusal must not read as contention, got {text:?}"
        );
    }

    // ---------------------------------------------------------------------
    // Completion-arm behaviour.
    //
    // The classification tests above prove the mapping. These prove what the
    // event loop does with it, which is where the state corruption lived.
    // ---------------------------------------------------------------------

    use crate::tui::app::state::AppMode;
    use crate::tui::app::types::SessionStatus;
    use crate::tui::app::App;
    use std::time::{Duration, Instant};

    const PROMPT: &str = "run the migration and tell me what broke";

    /// An app mid-turn: a live `active_turn_id`, a working clock, a running
    /// thinking indicator, and a composer that was optimistically cleared.
    fn app_with_live_turn() -> App {
        let mut app = App::new_for_testing("/tmp".into());
        app.session_state.session = Some(crate::session::Session {
            id: "s-1".into(),
            directory: "/tmp".into(),
            ..Default::default()
        });
        app.prompt_state.pending_send = true;
        app.prompt_state.prompt.clear();
        app.session_state.active_turn_id = Some("turn-live".into());
        app.session_state.session_status = SessionStatus::Working;
        app.session_state.working_since = Some(Instant::now() - Duration::from_secs(42));
        app.status_bar.set_thinking(true, None);
        app.pending_turn_submission = Some(crate::tui::app::PendingTurnSubmission {
            session_id: "s-1".into(),
            request_id: "req-1".into(),
            request_sha256: "sha-1".into(),
            prompt: PROMPT.into(),
        });
        app
    }

    fn begin(app: &mut App) -> u64 {
        app.turn_submit_request.begin()
    }

    fn contended() -> TurnSubmitOutcome {
        TurnSubmitOutcome::Contended {
            code: "turn_already_active".into(),
            message: "A turn is already active for this session".into(),
        }
    }

    #[test]
    fn contention_leaves_the_running_turn_exactly_as_it_found_it() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);
        let since = app.session_state.working_since;

        assert!(app.apply_turn_submission_finished("req-1", generation, contended()));

        // The turn's identity and age survive — this is the substantive fix.
        assert_eq!(
            app.session_state.active_turn_id.as_deref(),
            Some("turn-live"),
            "a turn that is still running must keep its identity"
        );
        assert_eq!(app.session_state.working_since, since);
        assert_eq!(app.session_state.session_status, SessionStatus::Working);
        assert!(app.status_bar.thinking, "the spinner keeps running");
        // ...and no latched turn failure, because the row above the composer
        // gives `last_error` absolute precedence over the live status.
        assert_eq!(
            app.session_state.last_error, None,
            "contention is not a turn failure; latching it would replace the \
             running turn's status line with a stale error"
        );
        // The submit's own state is released and the text comes back.
        assert!(!app.prompt_state.pending_send);
        assert_eq!(app.prompt_state.prompt.get_text(), PROMPT);
        assert!(app.pending_turn_submission.is_none());
    }

    #[test]
    fn contention_with_no_active_turn_preserves_nothing_it_does_not_have() {
        let mut app = App::new_for_testing("/tmp".into());
        app.session_state.session = Some(crate::session::Session {
            id: "s-1".into(),
            directory: "/tmp".into(),
            ..Default::default()
        });
        app.pending_turn_submission = Some(crate::tui::app::PendingTurnSubmission {
            session_id: "s-1".into(),
            request_id: "req-1".into(),
            request_sha256: "sha-1".into(),
            prompt: PROMPT.into(),
        });
        let generation = begin(&mut app);

        assert!(app.apply_turn_submission_finished("req-1", generation, contended()));

        // A frontend that attached late can be told a turn is running while
        // holding no turn state of its own. It must not invent one.
        assert_eq!(app.session_state.active_turn_id, None);
        assert_ne!(app.session_state.session_status, SessionStatus::Error);
        assert_eq!(app.prompt_state.prompt.get_text(), PROMPT);
    }

    #[test]
    fn a_refusal_restores_the_prompt_and_stashes_the_newer_draft() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);
        // The user kept typing while the submission was in flight.
        app.prompt_state
            .prompt
            .set_text("actually, also update the changelog".into());

        assert!(app.apply_turn_submission_finished(
            "req-1",
            generation,
            TurnSubmitOutcome::Rejected {
                code: "provider_not_found".into(),
                message: "Provider not found: nope".into(),
            }
        ));

        assert_eq!(
            app.prompt_state.prompt.get_text(),
            PROMPT,
            "the refused prompt comes back"
        );
        assert_eq!(
            app.prompt_state.stashed_prompts,
            vec!["actually, also update the changelog".to_string()],
            "the newer draft is preserved, not dropped"
        );
        // A refusal that started no turn still reports as one.
        assert_eq!(app.session_state.session_status, SessionStatus::Error);
        assert_eq!(app.session_state.active_turn_id, None);
        assert!(
            app.session_state
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("provider_not_found")),
            "the daemon's own reason is named, got {:?}",
            app.session_state.last_error
        );
    }

    #[test]
    fn resubmitting_a_restored_prompt_does_not_duplicate_its_transcript_entry() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);
        app.apply_turn_submission_finished(
            "req-1",
            generation,
            TurnSubmitOutcome::Rejected {
                code: "provider_not_found".into(),
                message: "x".into(),
            },
        );

        // `send_prompt` consults this before inserting a user message. A
        // restored prompt already has its transcript entry from the first
        // submit, so the retry must not add a second.
        assert!(app.prompt_state.take_retry_without_message(PROMPT.trim()));
    }

    #[test]
    fn a_lost_connection_is_reported_as_a_connection_problem() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);

        assert!(app.apply_turn_submission_finished(
            "req-1",
            generation,
            TurnSubmitOutcome::TransportFailure {
                message: "connection reset".into(),
            }
        ));

        let error = app
            .session_state
            .last_error
            .as_deref()
            .expect("a reported failure");
        assert!(
            error.contains("connection"),
            "a lost connection must not read as contention, got {error:?}"
        );
        assert!(!error.contains("already active"), "got {error:?}");
        // Recovery is possible, so the text comes back.
        assert_eq!(app.prompt_state.prompt.get_text(), PROMPT);
    }

    #[test]
    fn an_unexpected_response_is_reported_without_offering_a_resubmit() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);

        assert!(app.apply_turn_submission_finished(
            "req-1",
            generation,
            TurnSubmitOutcome::Unexpected {
                detail: "SessionList".into(),
            }
        ));

        assert!(
            app.session_state
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("unexpected")),
            "an uninterpreted response must be reported, not swallowed, got {:?}",
            app.session_state.last_error
        );
        assert_eq!(
            app.prompt_state.prompt.get_text(),
            "",
            "the submit may already have been accepted; offering a resubmit \
             here is how one prompt becomes two turns"
        );
    }

    #[test]
    fn a_superseded_completion_changes_nothing() {
        let mut app = app_with_live_turn();
        let stale = begin(&mut app);
        // A second submit supersedes the first.
        let _current = begin(&mut app);
        app.pending_turn_submission = Some(crate::tui::app::PendingTurnSubmission {
            session_id: "s-1".into(),
            request_id: "req-2".into(),
            request_sha256: "sha-2".into(),
            prompt: "the newer prompt".into(),
        });

        assert!(
            !app.apply_turn_submission_finished(
                "req-1",
                stale,
                TurnSubmitOutcome::Rejected {
                    code: "provider_not_found".into(),
                    message: "stale".into(),
                }
            ),
            "a superseded completion is dropped"
        );

        // Nothing at all moved: no turn teardown, no composer release, no
        // report, and the current submission is still the pending one.
        assert_eq!(
            app.session_state.active_turn_id.as_deref(),
            Some("turn-live"),
            "a stale completion must not null a live turn's identity"
        );
        assert!(
            app.status_bar.thinking,
            "the running turn keeps its spinner"
        );
        assert_eq!(app.session_state.last_error, None);
        assert_eq!(app.prompt_state.prompt.get_text(), "");
        assert!(app.prompt_state.pending_send, "the composer guard survives");
        let pending = app
            .pending_turn_submission
            .as_ref()
            .expect("the current submission is still pending");
        assert_eq!(pending.request_id, "req-2");
    }

    #[test]
    fn a_completion_arriving_after_cancel_changes_nothing() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);
        // Tab switch / reconnect: the daemon turn is not undone, but the local
        // completion is invalidated and the prompt is handed back.
        let restored = app.cancel_turn_submit().expect("the prompt comes back");
        assert_eq!(restored, PROMPT);

        assert!(
            !app.apply_turn_submission_finished("req-1", generation, contended()),
            "a cancelled completion is dropped"
        );
        assert_eq!(
            app.session_state.active_turn_id.as_deref(),
            Some("turn-live")
        );
        assert_eq!(app.session_state.last_error, None);
        assert!(app.prompt_state.pending_send);
    }

    #[test]
    fn restoring_a_long_prompt_is_bounded_never_truncated() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);
        // Fill the stash to its bound so a further push must evict.
        app.prompt_state.stashed_prompts = (0..crate::tui::app::state::prompt::PROMPT_STASH_BOUND)
            .map(|index| format!("draft-{index}"))
            .collect();
        let oversized = "x".repeat(120_000);
        app.pending_turn_submission = Some(crate::tui::app::PendingTurnSubmission {
            session_id: "s-1".into(),
            request_id: "req-1".into(),
            request_sha256: "sha-1".into(),
            prompt: oversized.clone(),
        });

        assert!(app.apply_turn_submission_finished(
            "req-1",
            generation,
            TurnSubmitOutcome::Rejected {
                code: "provider_not_found".into(),
                message: "x".into(),
            }
        ));

        assert_eq!(
            app.prompt_state.stashed_prompts.len(),
            crate::tui::app::state::prompt::PROMPT_STASH_BOUND,
            "the stash stays inside its bound"
        );
        assert_eq!(
            app.prompt_state.prompt.get_text().len(),
            oversized.len(),
            "a restored prompt is never truncated: a bound that clipped the \
             user's words would change what they wrote"
        );
    }

    #[test]
    fn accepted_is_accepted_and_clears_the_pending_record() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);
        assert!(app.apply_turn_submission_finished(
            "req-1",
            generation,
            TurnSubmitOutcome::Accepted
        ));
        assert!(app.pending_turn_submission.is_none());
        // An acceptance is not a failure and must not latch anything.
        assert_eq!(app.session_state.last_error, None);
        assert!(!app.turn_submit_request.is_loading());
    }

    #[test]
    fn a_completion_whose_request_id_does_not_match_reports_but_restores_nothing() {
        let mut app = app_with_live_turn();
        let generation = begin(&mut app);

        assert!(
            !app.apply_turn_submission_finished(
                "req-someone-else",
                generation,
                TurnSubmitOutcome::Rejected {
                    code: "provider_not_found".into(),
                    message: "x".into(),
                }
            ),
            "a completion for another submission's id is not applied"
        );
        assert!(
            app.pending_turn_submission
                .as_ref()
                .is_some_and(|pending| pending.request_id == "req-1"),
            "the retained submission survives"
        );
        assert_eq!(app.session_state.last_error, None);
    }

    #[test]
    fn an_observer_is_still_refused_before_any_submission_is_attempted() {
        // The observer gate sits a few lines above the code this milestone
        // changed; this is the guard against routing it back into turn state.
        // A fresh app, so the only thing under test is the gate.
        let mut app = App::new_for_testing("/tmp".into());
        assert!(
            app.observer.begin_observe("proj-1", "sess-1").is_some(),
            "observation is active"
        );
        app.prompt_state.prompt.set_text(PROMPT.into());

        app.send_prompt();

        assert!(
            app.pending_turn_submission.is_none(),
            "an observer must never reach a turn submission"
        );
        assert_eq!(
            app.prompt_state.prompt.get_text(),
            "",
            "and the refused text is not treated as a turn"
        );
    }

    #[test]
    fn a_socket_daemon_frontend_reports_a_missing_connection_without_losing_the_prompt() {
        let mut app = App::new_for_testing("/tmp".into());
        app.session_state.session = Some(crate::session::Session {
            id: "s-1".into(),
            directory: "/tmp".into(),
            ..Default::default()
        });
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/codegg-test.sock".into(),
        };
        app.prompt_state.prompt.set_text(PROMPT.into());

        app.send_prompt();

        // No core client: nothing can carry the prompt. The optimistic clear in
        // `send_prompt` must be undone, not left as a vanished message.
        assert_eq!(
            app.prompt_state.prompt.get_text(),
            PROMPT,
            "a prompt that could not be sent comes back"
        );
        assert_eq!(app.session_state.session_status, SessionStatus::Error);
        assert!(!app.prompt_state.pending_send);
    }
}
