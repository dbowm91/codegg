use crate::tui::app::state::{ProjectExecutionContext, UiRouteToken};
use crate::tui::components::completion_overlay::CompletionItem;

/// Immutable payload captured when a prompt needs a session before its turn
/// can be submitted.  The editable prompt widget is deliberately not used by
/// the async continuation: a user may continue typing while the request is
/// in flight without changing the text that will be submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSessionSubmit {
    pub request_id: u64,
    pub prompt: String,
    pub context: ProjectExecutionContext,
    pub route: UiRouteToken,
}

pub struct PromptState {
    pub prompt: crate::tui::components::prompt::PromptWidget,
    pub slash_completions: Vec<CompletionItem>,
    pub file_completions: Vec<CompletionItem>,
    pub agent_completions: Vec<CompletionItem>,
    pub completion_filter: String,
    pub show_completions: bool,
    pub completion_type: crate::tui::app::types::CompletionType,
    pub completion_sel: usize,
    pub stashed_prompts: Vec<String>,
    pub stash_pos: Option<usize>,
    pub pending_send: bool,
    /// Async lifecycle for the frontend-only session-create continuation.
    pub session_submit_request: crate::tui::app::state::AsyncUiRequestState,
    /// Captured prompt and route while SessionCreate is in flight.
    pub pending_session_submit: Option<PendingSessionSubmit>,
    /// Prevents the event-loop tick from spawning the same continuation more
    /// than once while its registered task is still running.
    pub session_submit_started: bool,
    /// A failed session creation already has a visible user message.  The
    /// next explicit retry should not insert that same message a second time.
    pub retry_without_message: Option<String>,
}

impl PromptState {
    /// Begin one session-create continuation.  A second submit while this is
    /// pending is coalesced by the caller and cannot create another request.
    pub fn begin_session_submit(
        &mut self,
        prompt: String,
        context: ProjectExecutionContext,
        route: UiRouteToken,
    ) -> Option<u64> {
        if self.pending_session_submit.is_some() || self.session_submit_request.is_loading() {
            return None;
        }
        let request_id = self.session_submit_request.begin();
        self.pending_session_submit = Some(PendingSessionSubmit {
            request_id,
            prompt,
            context,
            route,
        });
        Some(request_id)
    }

    /// Clone the captured payload exactly once when the registered task is
    /// spawned.  The state retains it until completion/cancellation so a
    /// reconnect can restore the prompt for an explicit retry.
    pub fn start_session_submit_task(&mut self) -> Option<PendingSessionSubmit> {
        if self.session_submit_started {
            return None;
        }
        let pending = self.pending_session_submit.clone()?;
        self.session_submit_started = true;
        Some(pending)
    }

    pub fn clear_session_submit(&mut self) {
        self.pending_session_submit = None;
        self.session_submit_started = false;
    }

    /// Invalidate an in-flight continuation on tab close, switch, reconnect,
    /// or shutdown.  The daemon request, if already sent, is not undone.
    pub fn cancel_session_submit(&mut self) -> Option<PendingSessionSubmit> {
        let pending = self.pending_session_submit.take();
        self.session_submit_started = false;
        if self.session_submit_request.is_loading() {
            self.session_submit_request.cancel();
        }
        self.prompt.set_waiting(false);
        pending
    }

    /// Mark a failed continuation as retryable without duplicating its local
    /// user-message projection.
    pub fn mark_retry_without_message(&mut self, prompt: String) {
        self.retry_without_message = Some(prompt);
    }

    pub fn take_retry_without_message(&mut self, prompt: &str) -> bool {
        if self.retry_without_message.as_deref() == Some(prompt) {
            self.retry_without_message = None;
            true
        } else {
            self.retry_without_message = None;
            false
        }
    }
}
