//! Prompt/session continuation commands.
//!
//! A prompt submitted without an attached session is already visible in the
//! local message projection.  This module only creates the missing session in
//! a registered task, then applies one route-validated completion and submits
//! the captured prompt exactly once.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::state::UiRouteToken;
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_scoped_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Start the one pending SessionCreate continuation, if any.
pub(crate) fn start_session_create_for_prompt(app: &mut App) {
    let Some(pending) = app.prompt_state.start_session_submit_task() else {
        return;
    };

    let request_id = pending.request_id;
    let route = pending.route.clone();
    let prompt = pending.prompt.clone();
    let context = pending.context.clone();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let scope_tab_id = route
        .tab_id
        .as_ref()
        .map(|tab_id| tab_id.as_str().to_string());
    let scope_epoch = Some(route.active_view_epoch);

    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "prompt_session_create",
        scope_tab_id,
        None,
        scope_epoch,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::PromptSessionCreated {
                    request_id,
                    route,
                    prompt,
                    session: None,
                    error: Some("Core client not configured; cannot create a session".to_string()),
                });
            };

            let request = crate::core::new_request(
                format!(
                    "session-create-prompt-{request_id}-{}",
                    uuid::Uuid::new_v4()
                ),
                CoreRequest::SessionCreate {
                    directory: context.workspace_root.to_string_lossy().into_owned(),
                    title: None,
                    project_id: context.project_id.clone(),
                    workspace_id: context.workspace_id.clone(),
                },
            );

            let completion = match core_client.request(request).await {
                Ok(CoreResponse::Session { session }) => TuiCommand::PromptSessionCreated {
                    request_id,
                    route,
                    prompt,
                    session: Some(session),
                    error: None,
                },
                Ok(CoreResponse::Error { code, message }) => TuiCommand::PromptSessionCreated {
                    request_id,
                    route,
                    prompt,
                    session: None,
                    error: Some(format!("{code}: {message}")),
                },
                Ok(_) => TuiCommand::PromptSessionCreated {
                    request_id,
                    route,
                    prompt,
                    session: None,
                    error: Some("Unexpected response while creating a session".to_string()),
                },
                Err(error) => TuiCommand::PromptSessionCreated {
                    request_id,
                    route,
                    prompt,
                    session: None,
                    error: Some(error.to_string()),
                },
            };
            Some(completion)
        },
    );

    if task_id.is_none() {
        let _ = app.prompt_state.session_submit_request.fail(
            request_id,
            "TUI command channel unavailable; prompt was not submitted".to_string(),
        );
        apply_prompt_session_failure(
            app,
            request_id,
            &pending.route,
            pending.prompt,
            "TUI command channel unavailable; prompt was not submitted",
        );
    }
}

/// Apply a SessionCreate completion on the event loop.
pub(crate) fn apply_session_create_for_prompt(
    app: &mut App,
    request_id: u64,
    route: UiRouteToken,
    prompt: String,
    session: Option<crate::protocol::dto::Session>,
    error: Option<String>,
) {
    // A completion is only meaningful while the active view still represents
    // the original no-session slot.  Session selection, tab switching, close,
    // reconnect, and shutdown invalidate the request before this point.
    if !app
        .prompt_state
        .session_submit_request
        .is_current(request_id)
        || app.prompt_state.session_submit_request.is_cancelled()
    {
        return;
    }

    let Some(check) = current_route_check(app) else {
        return;
    };
    if !route.matches(&check)
        || app.session_state.session.is_some()
        || app.active_session_id().is_some()
    {
        app.prompt_state.cancel_session_submit();
        app.prompt_state.pending_send = false;
        return;
    }

    app.prompt_state.clear_session_submit();

    if let Some(error) = error {
        if app
            .prompt_state
            .session_submit_request
            .fail(request_id, error.clone())
        {
            apply_prompt_session_failure(app, request_id, &route, prompt, &error);
        }
        return;
    }

    let Some(session) = session else {
        if app
            .prompt_state
            .session_submit_request
            .fail(request_id, "SessionCreate returned no session".to_string())
        {
            apply_prompt_session_failure(
                app,
                request_id,
                &route,
                prompt,
                "SessionCreate returned no session",
            );
        }
        return;
    };

    let session = match crate::protocol_conversions::dto_to_session(session) {
        Ok(session) => session,
        Err(error) => {
            if app
                .prompt_state
                .session_submit_request
                .fail(request_id, error.to_string())
            {
                apply_prompt_session_failure(app, request_id, &route, prompt, &error.to_string());
            }
            return;
        }
    };

    if !app.prompt_state.session_submit_request.finish(request_id) {
        return;
    }

    app.set_session(session);
    // The user message was inserted by send_prompt before SessionCreate.
    // Submit the immutable captured text exactly once; never re-read the
    // latest message because another local edit may have happened meanwhile.
    app.dispatch_turn_submit_request(prompt);
    app.prompt_state.pending_send = false;
    app.prompt_state.prompt.set_waiting(false);
}

fn current_route_check(app: &App) -> Option<crate::tui::app::state::RouteCheck> {
    let context = app.project_execution_context().ok()?;
    let tab_id = app.active_tab_id();
    Some(app.routing_registry.check_for(
        tab_id.as_ref(),
        context.project_id.as_deref(),
        context.workspace_id.as_deref(),
        context.session_id.as_deref(),
        app.view_switch.active_view_epoch,
    ))
}

fn apply_prompt_session_failure(
    app: &mut App,
    _request_id: u64,
    _route: &UiRouteToken,
    prompt: String,
    error: &str,
) {
    app.prompt_state.clear_session_submit();
    let current_draft = app.prompt_state.prompt.get_text();
    if !current_draft.trim().is_empty() && current_draft != prompt {
        if app.prompt_state.stashed_prompts.len() >= 100 {
            app.prompt_state.stashed_prompts.remove(0);
        }
        app.prompt_state.stashed_prompts.push(current_draft);
    }
    app.prompt_state.prompt.set_text(prompt.clone());
    app.prompt_state.prompt.set_waiting(false);
    app.prompt_state.mark_retry_without_message(prompt);
    app.prompt_state.pending_send = false;
    app.session_state.session_status = crate::tui::app::SessionStatus::Error;
    app.messages_state
        .toasts
        .error(&format!("Session creation failed: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::protocol::core::{CoreEvent, EventEnvelope, RequestEnvelope};
    use crate::tui::app::state::{PendingSessionSubmit, ProjectExecutionContext};
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Notify};

    struct SlowClient {
        release: Arc<Notify>,
    }

    #[async_trait]
    impl CoreClient for SlowClient {
        async fn request(
            &self,
            _request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, crate::error::AppError> {
            self.release.notified().await;
            Ok(CoreResponse::Error {
                code: "test".to_string(),
                message: "released".to_string(),
            })
        }

        fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            rx
        }
    }

    fn pending(app: &App, request_id: u64) -> PendingSessionSubmit {
        let context = ProjectExecutionContext {
            project_id: app.active_project_id().map(str::to_string),
            workspace_id: app.active_workspace_id().map(str::to_string),
            session_id: None,
            workspace_root: PathBuf::from("/tmp"),
        };
        PendingSessionSubmit {
            request_id,
            prompt: "hello".to_string(),
            route: UiRouteToken::new(
                app.active_tab_id(),
                context.project_id.clone(),
                context.workspace_id.clone(),
                None,
                app.view_switch.active_view_epoch,
                app.routing_registry.reconnect_epoch,
                request_id,
            ),
            context,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn delayed_session_create_is_registered_without_blocking_caller() {
        let release = Arc::new(Notify::new());
        let client = Arc::new(SlowClient {
            release: release.clone(),
        });
        let mut app = App::new_for_testing("/tmp".to_string());
        app.set_core_client(client);
        let (tx, mut rx) = mpsc::channel(2);
        app.tui_cmd_tx = Some(tx);
        let request_id = app.prompt_state.session_submit_request.begin();
        let pending = pending(&app, request_id);
        app.prompt_state.pending_session_submit = Some(pending);
        app.prompt_state.pending_send = true;

        start_session_create_for_prompt(&mut app);
        assert_eq!(app.task_registry.active_count(), 1);
        assert!(rx.try_recv().is_err());

        release.notify_one();
        let completion = rx.recv().await.expect("completion should arrive");
        assert!(matches!(
            completion,
            TuiCommand::PromptSessionCreated { .. }
        ));
    }

    #[test]
    fn create_failure_restores_prompt_without_duplicate_message_on_retry() {
        let mut app = App::new_for_testing("/tmp".to_string());
        let request_id = app.prompt_state.session_submit_request.begin();
        let pending = pending(&app, request_id);
        let route = pending.route.clone();
        app.prompt_state.pending_session_submit = Some(pending);
        app.prompt_state.pending_send = true;
        app.messages_state
            .messages
            .add_user_message("hello".to_string(), Some(false));

        apply_session_create_for_prompt(
            &mut app,
            request_id,
            route,
            "hello".to_string(),
            None,
            Some("daemon unavailable".to_string()),
        );

        assert_eq!(app.prompt_state.prompt.get_text(), "hello");
        assert!(!app.prompt_state.pending_send);
        let before = app.messages_state.messages.messages.len();
        app.process_msg(crate::tui::app::TuiMsg::SubmitPrompt);
        assert_eq!(app.messages_state.messages.messages.len(), before);
    }

    #[test]
    fn double_submit_is_coalesced_before_session_create_starts() {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.prompt_state.prompt.set_text("hello".to_string());

        app.process_msg(crate::tui::app::TuiMsg::SubmitPrompt);
        let request_id = app
            .prompt_state
            .pending_session_submit
            .as_ref()
            .expect("first submit should capture a session continuation")
            .request_id;
        let message_count = app.messages_state.messages.messages.len();

        app.process_msg(crate::tui::app::TuiMsg::SubmitPrompt);

        assert_eq!(
            app.prompt_state
                .pending_session_submit
                .as_ref()
                .map(|pending| pending.request_id),
            Some(request_id)
        );
        assert_eq!(app.messages_state.messages.messages.len(), message_count);
    }

    #[test]
    fn stale_route_completion_cannot_bind_or_submit() {
        let mut app = App::new_for_testing("/tmp".to_string());
        let request_id = app.prompt_state.session_submit_request.begin();
        let pending = pending(&app, request_id);
        let route = pending.route.clone();
        app.prompt_state.pending_session_submit = Some(pending);
        app.prompt_state.pending_send = true;
        app.view_switch.bump_epoch();

        apply_session_create_for_prompt(
            &mut app,
            request_id,
            route,
            "hello".to_string(),
            None,
            Some("should be dropped".to_string()),
        );

        assert!(app.session_state.session.is_none());
        assert!(!app.prompt_state.pending_send);
        assert!(app.prompt_state.prompt.get_text().is_empty());
    }
}
