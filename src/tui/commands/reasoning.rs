//! Provider reasoning control (M004): the `/reasoning` option surface and
//! its daemon write path.
//!
//! The daemon owns the selection. `/reasoning` derives the options the
//! selected model actually accepts, advances the cycle to the next one, and
//! writes it through `CoreRequest::ReasoningPreferenceSet`. The local
//! display is updated from the daemon's **response**, never from the option
//! the user just picked: when the daemon refuses the write, or when another
//! frontend changed the selection first, the daemon's value wins and the
//! local display is corrected.
//!
//! The option set comes from the daemon's model-resolution response, which
//! carries the model's effective capability. A model with no published
//! capability — and a peer that predates the capability entirely — yields
//! the known-label fallback, and the command says that it is a fallback
//! rather than presenting the ladder as the model's full option set.
//!
//! Concurrency follows the `start_*/apply_*` pair pattern used by the
//! runtime-policy commands: every `start_*` bumps the request generation
//! and the matching `apply_*` drops stale completions.

use codegg_protocol::core::{ReasoningMode, ReasoningOverride, RuntimePreferenceDto};

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::provider::{reasoning_options, ReasoningOptionSet};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// The options the current model accepts, as the frontend sees them.
///
/// The set is always a subset of what the capability permits; the frontend
/// never widens it.
pub fn option_set(app: &App) -> ReasoningOptionSet {
    reasoning_options(app.current_reasoning_capability())
}

/// Advance the cycle to the next option the model accepts.
///
/// The cycle moves through the **model's** options rather than a static
/// ladder. A model that accepts no disable is never offered one, and a
/// model that always reasons has no off to cycle to.
///
/// A selection that is not one of the model's options — the model changed
/// under it, or another frontend wrote a selection this model does not
/// accept — restarts the cycle at the model's first option rather than
/// inventing a neighbouring level.
pub fn next_mode(current: &ReasoningMode, options: &ReasoningOptionSet) -> ReasoningMode {
    if options.options.is_empty() {
        return ReasoningMode::Inherit;
    }
    let Some(index) = options.options.iter().position(|option| option == current) else {
        return options.options[0].clone();
    };
    let next = (index + 1) % options.options.len();
    options.options[next].clone()
}

/// The daemon override that selects one option.
///
/// `Inherit` clears the selection, which puts the turn back on the agent /
/// model-profile default.
pub fn override_for(mode: &ReasoningMode) -> Option<ReasoningOverride> {
    match mode {
        ReasoningMode::Inherit => None,
        ReasoningMode::Effort(level) => Some(ReasoningOverride::Effort {
            level: level.clone(),
        }),
        ReasoningMode::Budget(tokens) => Some(ReasoningOverride::Budget { tokens: *tokens }),
        ReasoningMode::Off => Some(ReasoningOverride::Off),
    }
}

/// Why the model's option set is what it is.
///
/// The command explains a reduced option set rather than appearing broken:
/// a model that always reasons has no off, and a model with no published
/// capability falls back to the known-label ladder.
pub fn option_note(options: &ReasoningOptionSet) -> Option<String> {
    if options.always_reasons {
        return Some("this model always reasons, so there is no off".to_string());
    }
    if !options.allows_disable() {
        return Some("this model does not accept an off switch".to_string());
    }
    if options.fallback {
        return Some(
            "known labels only — this model publishes no reasoning capability".to_string(),
        );
    }
    None
}

/// A readable message for a daemon refusal.
///
/// The typed refusals are surfaced, not swallowed: a user who asked for a
/// reasoning control needs to know whether the daemon refused the
/// vocabulary, the model's capability, the selection's shape, or a
/// concurrent write.
pub fn refusal_message(code: &str, message: &str) -> String {
    match code {
        "preference_conflict" => format!(
            "Extended thinking: the daemon's selection changed underneath this request ({message})"
        ),
        "invalid_reasoning_selection" => {
            format!("Extended thinking: the daemon refused that selection ({message})")
        }
        "reasoning_vocabulary_unsupported" => format!(
            "Extended thinking: this daemon does not accept that reasoning label ({message})"
        ),
        "reasoning_capability_unsupported" => format!(
            "Extended thinking: the model does not accept that reasoning selection ({message})"
        ),
        "preference_unavailable" => {
            format!("Extended thinking: the daemon cannot store the selection ({message})")
        }
        _ => format!("Extended thinking: {message}"),
    }
}

/// Write one selection through the daemon.
///
/// The completion lands as `TuiCommand::ReasoningPreferenceUpdated`; the
/// local display is updated from the daemon's response, not from the option
/// the user picked.
pub(crate) fn start_preference_update(
    app: &mut App,
    selection: Option<ReasoningOverride>,
    previous: ReasoningMode,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.error(
            "Extended thinking unavailable: this frontend has no daemon connection, so the \
             selection cannot be stored",
        );
        return;
    };
    let request_id = app.ui_state.reasoning_request.begin();
    let expected_revision = app.ui_state.reasoning_preference_revision;
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "reasoning_preference_set",
        async move {
            let preference = match core_client
                .request(crate::core::new_request(
                    format!("reasoning-set-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ReasoningPreferenceSet {
                        selection,
                        expected_revision,
                    },
                ))
                .await
            {
                Ok(CoreResponse::ReasoningPreference { preference }) => preference,
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::ReasoningPreferenceUpdated {
                        request_id,
                        preference: None,
                        previous,
                        error: Some(refusal_message(&code, &message)),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::ReasoningPreferenceUpdated {
                        request_id,
                        preference: None,
                        previous,
                        error: Some(
                            "Extended thinking update got an unexpected daemon response"
                                .to_string(),
                        ),
                    });
                }
                Err(error) => {
                    return Some(TuiCommand::ReasoningPreferenceUpdated {
                        request_id,
                        preference: None,
                        previous,
                        error: Some(format!("Extended thinking update request failed: {error}")),
                    });
                }
            };
            Some(TuiCommand::ReasoningPreferenceUpdated {
                request_id,
                preference: Some(preference),
                previous,
                error: None,
            })
        },
    );
}

/// Apply a write completion. Drops stale completions.
///
/// The daemon's value wins: the local display is corrected from the
/// response. A refusal puts the daemon's last known value back, so the
/// display never claims a selection the daemon did not accept.
pub(crate) fn apply_preference_updated(
    app: &mut App,
    request_id: u64,
    preference: Option<RuntimePreferenceDto>,
    previous: ReasoningMode,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .ui_state
            .reasoning_request
            .fail(request_id, error.clone())
        {
            app.ui_state.reasoning_mode = previous;
            app.messages_state.toasts.error(&error);
        }
        return;
    }
    let Some(preference) = preference else {
        if app.ui_state.reasoning_request.fail(
            request_id,
            "the daemon returned no reasoning preference".to_string(),
        ) {
            app.ui_state.reasoning_mode = previous;
            app.messages_state
                .toasts
                .error("Extended thinking: the daemon returned no preference payload");
        }
        return;
    };
    if !app.ui_state.reasoning_request.finish(request_id) {
        return;
    }
    app.ui_state.reasoning_preference_revision = Some(preference.revision);
    let daemon_mode = ReasoningMode::from(preference.reasoning_selection.clone());
    let corrected = daemon_mode != app.ui_state.reasoning_mode;
    app.ui_state.reasoning_mode = daemon_mode.clone();
    if corrected {
        // The daemon's selection differs from the option the user picked —
        // another frontend wrote it, or the daemon narrowed it. Say so
        // rather than leaving the display quietly disagreeing with the
        // daemon.
        app.messages_state.toasts.warning(&format!(
            "Extended thinking: the daemon's selection is {}",
            daemon_mode.detail()
        ));
    }
}

/// Read the daemon's selection.
///
/// The selection is daemon state, so a frontend that reconnects receives it
/// from the daemon rather than from local state. The completion lands as
/// `TuiCommand::ReasoningPreferenceLoaded`.
pub(crate) fn start_preference_refresh(app: &mut App) {
    let Some(core_client) = app.core_client.clone() else {
        return;
    };
    let request_id = app.ui_state.reasoning_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "reasoning_preference_get",
        async move {
            let preference = match core_client
                .request(crate::core::new_request(
                    format!("reasoning-get-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ReasoningPreferenceGet,
                ))
                .await
            {
                Ok(CoreResponse::ReasoningPreference { preference }) => Some(preference),
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::ReasoningPreferenceLoaded {
                        request_id,
                        preference: None,
                        error: Some(refusal_message(&code, &message)),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::ReasoningPreferenceLoaded {
                        request_id,
                        preference: None,
                        error: Some(
                            "Extended thinking read got an unexpected daemon response".to_string(),
                        ),
                    });
                }
                Err(error) => {
                    return Some(TuiCommand::ReasoningPreferenceLoaded {
                        request_id,
                        preference: None,
                        error: Some(format!("Extended thinking read request failed: {error}")),
                    });
                }
            };
            Some(TuiCommand::ReasoningPreferenceLoaded {
                request_id,
                preference,
                error: None,
            })
        },
    );
}

/// Apply a read completion. Drops stale completions.
pub(crate) fn apply_preference_loaded(
    app: &mut App,
    request_id: u64,
    preference: Option<RuntimePreferenceDto>,
    error: Option<String>,
) {
    if let Some(error) = error {
        // A read failure is not worth a toast at startup: the selection is
        // simply not known yet, and the working strip shows "default"
        // rather than a wrong value.
        tracing::debug!(target: "codegg::tui::reasoning", %error, "reasoning preference read failed");
        app.ui_state.reasoning_request.fail(request_id, error);
        return;
    }
    let Some(preference) = preference else {
        app.ui_state.reasoning_request.fail(
            request_id,
            "the daemon returned no reasoning preference".to_string(),
        );
        return;
    };
    if !app.ui_state.reasoning_request.finish(request_id) {
        return;
    }
    app.ui_state.reasoning_preference_revision = Some(preference.revision);
    app.ui_state.reasoning_mode = ReasoningMode::from(preference.reasoning_selection.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::error::AppError;
    use codegg_protocol::core::{CoreEvent, EventEnvelope, RequestEnvelope};
    use codegg_protocol::provider::{ReasoningCapabilityAxis, ReasoningCapabilityDto};
    use std::sync::{Arc, Mutex};

    /// The selections the daemon was asked to write, in order.
    type RecordedWrites = Arc<Mutex<Vec<Option<ReasoningOverride>>>>;

    /// A daemon that records the reasoning writes it receives and replies
    /// with a canned answer.
    struct ReasoningDaemon {
        writes: RecordedWrites,
        reply: Arc<Mutex<CoreResponse>>,
    }

    impl ReasoningDaemon {
        fn new(reply: CoreResponse) -> (Arc<Self>, RecordedWrites) {
            let writes = Arc::new(Mutex::new(Vec::new()));
            (
                Arc::new(Self {
                    writes: Arc::clone(&writes),
                    reply: Arc::new(Mutex::new(reply)),
                }),
                writes,
            )
        }
    }

    #[async_trait::async_trait]
    impl CoreClient for ReasoningDaemon {
        async fn request(
            &self,
            request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, AppError> {
            match request.payload {
                CoreRequest::ReasoningPreferenceSet { selection, .. } => {
                    self.writes.lock().expect("writes lock").push(selection);
                    Ok(self.reply.lock().expect("reply lock").clone())
                }
                _ => Ok(CoreResponse::Ack),
            }
        }

        fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    fn preference(selection: Option<ReasoningOverride>) -> RuntimePreferenceDto {
        RuntimePreferenceDto {
            principal_id: "local".to_string(),
            approval_mode: crate::protocol::core::ApprovalModeDto::Interactive,
            sandbox_profile: crate::protocol::core::SandboxProfileDto::WorkspaceWrite,
            revision: 7,
            updated_at_ms: 0,
            last_provider_connection_id: None,
            last_model_id: None,
            last_task_provider_connection_id: None,
            last_task_model_id: None,
            reasoning_selection: selection,
        }
    }

    fn capability(
        effort: ReasoningCapabilityAxis,
        labels: &[&str],
        disable: ReasoningCapabilityAxis,
    ) -> ReasoningCapabilityDto {
        ReasoningCapabilityDto {
            effort,
            accepted_effort_labels: labels.iter().map(|l| (*l).to_string()).collect(),
            disable,
            budget: ReasoningCapabilityAxis::Unsupported,
            min_budget_tokens: None,
            max_budget_tokens: None,
            always_reasons: None,
        }
    }

    /// An app wired to a daemon, with a command channel the test can drain.
    fn app_with_daemon(
        daemon: Arc<ReasoningDaemon>,
    ) -> (App, tokio::sync::mpsc::Receiver<TuiCommand>) {
        let mut app = App::new_for_testing("/tmp".to_string());
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        app.set_core_client(daemon);
        (app, rx)
    }

    /// The command sends `ReasoningPreferenceSet` and the local display is
    /// updated from the daemon's **response**, not from the option the user
    /// picked.
    #[tokio::test]
    async fn the_command_writes_the_selection_through_the_daemon() {
        let (daemon, writes) = ReasoningDaemon::new(CoreResponse::ReasoningPreference {
            preference: preference(Some(ReasoningOverride::Effort {
                level: "high".to_string(),
            })),
        });
        let (mut app, mut rx) = app_with_daemon(daemon);
        app.agent_state.current_model = "minimax/minimax-m3".to_string();
        app.record_reasoning_capability(
            "minimax-m3",
            Some(capability(
                ReasoningCapabilityAxis::Supported,
                &["low", "medium", "high"],
                ReasoningCapabilityAxis::Supported,
            )),
        );

        app.dispatch_builtin_command(crate::tui::command::BuiltinSlashAction::Reasoning, None);

        let command = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("a completion arrives")
            .expect("the channel is open");
        let TuiCommand::ReasoningPreferenceUpdated {
            request_id,
            preference,
            previous,
            error,
        } = command
        else {
            panic!("expected a reasoning preference completion");
        };
        assert!(error.is_none(), "the write was refused: {error:?}");
        assert_eq!(previous, ReasoningMode::Inherit);

        // The request carried the selection the user picked.
        let written = writes.lock().expect("writes lock").clone();
        assert_eq!(
            written,
            vec![Some(ReasoningOverride::Effort {
                level: "low".to_string(),
            })],
            "the command must write the next option through the daemon"
        );

        apply_preference_updated(&mut app, request_id, preference, previous, None);
        assert_eq!(
            app.ui_state.reasoning_mode,
            ReasoningMode::Effort("high".to_string()),
            "the daemon's response drives the local display"
        );
        assert_eq!(app.ui_state.reasoning_preference_revision, Some(7));
    }

    /// A daemon-side selection **wins** over a stale local display.
    #[tokio::test]
    async fn a_daemon_side_selection_wins_over_a_stale_local_display() {
        let (daemon, _writes) = ReasoningDaemon::new(CoreResponse::ReasoningPreference {
            preference: preference(Some(ReasoningOverride::Effort {
                level: "max".to_string(),
            })),
        });
        let (mut app, mut rx) = app_with_daemon(daemon);
        app.agent_state.current_model = "minimax/minimax-m3".to_string();
        // The local display is stale: another frontend already moved the
        // daemon's selection.
        app.ui_state.reasoning_mode = ReasoningMode::Effort("low".to_string());

        app.dispatch_builtin_command(crate::tui::command::BuiltinSlashAction::Reasoning, None);
        let command = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("a completion arrives")
            .expect("the channel is open");
        let TuiCommand::ReasoningPreferenceUpdated {
            request_id,
            preference,
            previous,
            error,
        } = command
        else {
            panic!("expected a reasoning preference completion");
        };
        assert!(error.is_none());

        apply_preference_updated(&mut app, request_id, preference, previous, None);
        assert_eq!(
            app.ui_state.reasoning_mode,
            ReasoningMode::Effort("max".to_string()),
            "the daemon's value wins over the stale local display"
        );
    }

    /// Every typed refusal is surfaced, not swallowed, and the local display
    /// falls back to the daemon's last known value.
    #[tokio::test]
    async fn a_typed_refusal_is_surfaced_rather_than_swallowed() {
        for code in [
            "preference_conflict",
            "invalid_reasoning_selection",
            "reasoning_vocabulary_unsupported",
            "reasoning_capability_unsupported",
        ] {
            let (daemon, _writes) = ReasoningDaemon::new(CoreResponse::Error {
                code: code.to_string(),
                message: format!("{code} happened"),
            });
            let (mut app, mut rx) = app_with_daemon(daemon);
            app.agent_state.current_model = "minimax/minimax-m3".to_string();
            let previous = app.ui_state.reasoning_mode.clone();

            app.dispatch_builtin_command(crate::tui::command::BuiltinSlashAction::Reasoning, None);
            let command = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("a completion arrives")
                .expect("the channel is open");
            let TuiCommand::ReasoningPreferenceUpdated {
                request_id,
                preference,
                previous: carried,
                error,
            } = command
            else {
                panic!("expected a reasoning preference completion");
            };
            assert!(preference.is_none());
            let error = error.expect("the refusal must not be swallowed");
            assert!(
                error.contains(code),
                "the refusal must name its code, got {error:?}"
            );

            apply_preference_updated(&mut app, request_id, preference, carried, Some(error));
            assert_eq!(
                app.ui_state.reasoning_mode, previous,
                "a refused write leaves the daemon's last known value displayed"
            );
            let toasts: Vec<String> = app
                .messages_state
                .toasts
                .iter()
                .map(|toast| toast.message.clone())
                .collect();
            assert!(
                toasts.iter().any(|toast| toast.contains(code)),
                "the refusal must reach the user, got {toasts:?}"
            );
        }
    }

    /// The frontend's fallback for an absent capability offers the
    /// known-label ladder and does not present it as the model's full option
    /// set.
    #[test]
    fn an_absent_capability_falls_back_to_the_known_label_ladder() {
        let app = App::new_for_testing("/tmp".to_string());
        let options = option_set(&app);
        assert!(options.fallback, "an absent payload is a fallback");
        for label in crate::protocol::core::KNOWN_REASONING_EFFORT_LABELS {
            assert!(
                options
                    .options
                    .contains(&ReasoningMode::Effort(label.to_string())),
                "the fallback offers the known label '{label}'"
            );
        }
        let note = option_note(&options).expect("the fallback is stated");
        assert!(
            note.contains("known labels"),
            "the fallback must say it is a fallback, got {note:?}"
        );
    }

    /// A model that permits no disable is never offered one.
    #[test]
    fn a_model_that_permits_no_disable_is_never_offered_one() {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.agent_state.current_model = "minimax/minimax-m3".to_string();
        app.record_reasoning_capability(
            "minimax-m3",
            Some(capability(
                ReasoningCapabilityAxis::Supported,
                &["low", "high"],
                ReasoningCapabilityAxis::Unsupported,
            )),
        );
        let options = option_set(&app);
        assert!(!options.options.contains(&ReasoningMode::Off));
        assert!(!options.allows_disable());
        let note = option_note(&options).expect("the reduced option set is explained");
        assert!(
            note.contains("off switch"),
            "the command must explain the missing off, got {note:?}"
        );
    }

    /// The always-reasons case is stated: the model always reasons, so there
    /// is no off.
    #[test]
    fn an_always_reasons_model_is_stated_and_has_no_off() {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.agent_state.current_model = "minimax/minimax-m3".to_string();
        let mut capability = capability(
            ReasoningCapabilityAxis::Supported,
            &["low", "high"],
            ReasoningCapabilityAxis::Supported,
        );
        capability.always_reasons = Some(true);
        app.record_reasoning_capability("minimax-m3", Some(capability));

        let options = option_set(&app);
        assert!(options.always_reasons);
        assert!(!options.options.contains(&ReasoningMode::Off));
        let note = option_note(&options).expect("the always-reasons case is stated");
        assert!(
            note.contains("always reasons"),
            "the command must state the always-reasons case, got {note:?}"
        );
    }

    /// The cycle moves through the **model's** options, not a static ladder.
    #[test]
    fn the_cycle_moves_through_the_models_options() {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.agent_state.current_model = "minimax/minimax-m3".to_string();
        app.record_reasoning_capability(
            "minimax-m3",
            Some(capability(
                ReasoningCapabilityAxis::Supported,
                &["low", "high"],
                ReasoningCapabilityAxis::Unsupported,
            )),
        );
        let options = option_set(&app);
        assert_eq!(
            options.options,
            vec![
                ReasoningMode::Inherit,
                ReasoningMode::Effort("low".to_string()),
                ReasoningMode::Effort("high".to_string()),
            ]
        );
        assert_eq!(
            next_mode(&ReasoningMode::Inherit, &options),
            ReasoningMode::Effort("low".to_string())
        );
        assert_eq!(
            next_mode(&ReasoningMode::Effort("low".to_string()), &options),
            ReasoningMode::Effort("high".to_string())
        );
        assert_eq!(
            next_mode(&ReasoningMode::Effort("high".to_string()), &options),
            ReasoningMode::Inherit,
            "the cycle wraps"
        );
    }
}
