use crate::tui::app::state::{ComposerMode, PendingTaskCreate, TaskModelChoice};
use crate::tui::app::state::{ProjectExecutionContext, UiRouteToken};
use crate::tui::chat_browser::ChatComposeSeed;
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
    /// Project Work Orders M003: composer submission mode. Distinct from
    /// `InputMode` (text editing/Vim); owned by project/session UI state.
    pub composer_mode: ComposerMode,
    /// Captured Task prompt and route while `WorkOrderCreate` is in
    /// flight. The editable prompt widget is never consulted by the
    /// async continuation; a failed create restores the prompt exactly
    /// once for explicit retry (same pattern as `pending_session_submit`).
    pub pending_task_create: Option<PendingTaskCreate>,
    /// Reentrancy guard mirroring `session_submit_started` for the Task
    /// create continuation.
    pub task_submit_started: bool,
    /// Project Work Orders M003: resolved Task-composer model choice
    /// (last valid Task model when available, else the normal
    /// current/default selection). Convenience default only; ordinary
    /// session preference behavior is unchanged.
    pub task_model_choice: Option<TaskModelChoice>,
    /// Armed chat context for the next composer submission.
    ///
    /// Set by a chat surface that cannot own a text buffer of its own — the
    /// side panel — when `r`/`e` is pressed. It is *not* a second draft: the
    /// prompt widget still holds the text, and this records only what the
    /// text means (a reply target, or a body being replaced).
    ///
    /// While armed it outranks panel focus for
    /// [`App::input_target`](crate::tui::app::App::input_target): the user
    /// explicitly asked to reply to *this* message, so moving focus to the
    /// session panel must not silently redirect what they type into the
    /// agent. Cancelling clears it and leaves the text in place.
    pub chat_seed: Option<ChatComposeSeed>,
    /// When the "press again to stop the agent" confirmation was armed.
    ///
    /// Stopping a running turn is destructive and one keystroke away from a
    /// stray `x`, so it is gated behind a second press of the same key inside
    /// [`STOP_ARM_WINDOW`]. `None` is the disarmed state; the arm also expires
    /// on its own so a confirmation prompt cannot sit on screen forever after
    /// the user has looked away.
    pub stop_armed_at: Option<std::time::Instant>,
}

/// How long the stop confirmation stays armed before it lapses.
///
/// Long enough to press a key twice deliberately, short enough that a
/// confirmation nobody acts on does not outlive the attention it was asking
/// for.
pub const STOP_ARM_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);

/// How many displaced drafts the prompt stash retains.
///
/// Every path that puts a submitted prompt back into the composer goes
/// through [`PromptState::restore_prompt_with_stash`], so this is the single
/// bound. It was previously a literal `100` repeated in three places, which
/// meant the bound was three promises rather than one.
pub const PROMPT_STASH_BOUND: usize = 100;

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

    /// Put a prompt the user already submitted back into the composer.
    ///
    /// The one restore shape in the TUI. Session-create failure, reconnect, and
    /// a refused turn submission all lose the user's text the same way: the
    /// composer is cleared optimistically before the daemon has accepted
    /// anything, so every refusal has to undo that. Keeping one method means
    /// the bounded stash and the no-duplicate-transcript rule are proved once
    /// rather than three times.
    ///
    /// * A non-empty draft typed while the submission was in flight is
    ///   displaced into the bounded stash, never dropped.
    /// * The restored text is put back **whole**. Truncating a prompt to fit a
    ///   bound would change what the user wrote, so the bound applies to how
    ///   many displaced drafts are kept, never to this text.
    /// * [`PromptState::mark_retry_without_message`] is set so resubmitting the
    ///   restored prompt produces one user transcript entry, not two.
    ///
    /// An empty `prompt` is ignored: there is nothing to give back, and
    /// restoring it would silently clear whatever the user has since typed.
    pub fn restore_prompt_with_stash(&mut self, prompt: &str) -> bool {
        if prompt.trim().is_empty() {
            return false;
        }
        let current_draft = self.prompt.get_text();
        if !current_draft.trim().is_empty() && current_draft != prompt {
            if self.stashed_prompts.len() >= PROMPT_STASH_BOUND {
                self.stashed_prompts.remove(0);
            }
            self.stashed_prompts.push(current_draft);
        }
        self.prompt.set_text(prompt.to_string());
        self.prompt.set_waiting(false);
        self.mark_retry_without_message(prompt.to_string());
        true
    }

    /// Toggle the composer between Session and Task submission modes.
    /// `InputMode` is untouched; the editable prompt text is preserved.
    pub fn toggle_composer_mode(&mut self) -> ComposerMode {
        self.composer_mode = self.composer_mode.toggle();
        self.composer_mode
    }

    /// Invalidate an in-flight Task create on tab close/switch,
    /// reconnect, or shutdown. The daemon WorkOrder, if already
    /// committed, is never deleted to "undo" a stale UI completion;
    /// the projection refreshes when next foregrounded.
    pub fn cancel_task_submit(&mut self) -> Option<PendingTaskCreate> {
        let pending = self.pending_task_create.take();
        self.task_submit_started = false;
        self.prompt.set_waiting(false);
        pending
    }
}
