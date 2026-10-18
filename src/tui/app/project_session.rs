//! Project, session, routing, and projection lifecycle.
//!
//! This module owns active-tab context and transport/projection transitions.
//! It does not own canonical session or projection truth; those remain in the
//! existing state controllers and daemon protocol.

use super::App;
use crate::protocol::projection::controller::ControllerApplyOutcome;
use crate::protocol::projection::event::ProjectionEnvelope;
use crate::protocol::projection::replay::ProjectionSubscriptionId;
use crate::tui::app::state::ProjectTabId;
use crate::tui::app::types::SessionStatus;
use std::path::PathBuf;

impl App {
    pub fn project_execution_context(
        &self,
    ) -> Result<crate::tui::app::state::ProjectExecutionContext, String> {
        crate::tui::app::state::resolve_active_execution_context(&self.project_tabs)
    }

    /// M005: chat/composer target project. While the Workspace primary
    /// view is active, the selected project owns routing; otherwise the
    /// active tab. `None` when the view has no selection or no tab is
    /// active. Never falls back to cwd or hidden state.
    pub fn chat_target_project_id(&self) -> Option<String> {
        if crate::tui::commands::workspace_dashboard::is_workspace_view_active(self) {
            crate::tui::commands::workspace_dashboard::workspace_selected_project_id(self)
        } else {
            self.active_project_id().map(str::to_string)
        }
    }

    /// Return the active workspace root for legacy local services. The
    /// value is still resolved from the active tab, never from process cwd.
    pub fn active_workspace_root(&self) -> Option<PathBuf> {
        self.project_execution_context()
            .ok()
            .map(|context| context.workspace_root)
    }

    /// Bounded-source label for TUI project context surfaces. The
    /// project name comes from the active tab's daemon detail/catalog
    /// cache, and the root comes only from its execution context.
    pub(crate) fn active_project_display_context(&self) -> Option<String> {
        let tab = self.active_tab()?;
        let project_name = tab
            .project_details
            .as_ref()
            .map(|details| details.project.display_name.as_str())
            .or_else(|| {
                tab.project_id.as_deref().and_then(|project_id| {
                    self.project_catalog
                        .entries
                        .iter()
                        .find(|entry| entry.project_id == project_id)
                        .map(|entry| entry.display_name.as_str())
                })
            })
            .unwrap_or(tab.label.as_str());
        let root = self
            .active_workspace_root()
            .map(|path| path.display().to_string());
        match (project_name.is_empty(), root) {
            (false, Some(root)) => Some(format!("{project_name} · {root}")),
            (false, None) => Some(project_name.to_string()),
            (true, Some(root)) => Some(root),
            (true, None) => None,
        }
    }

    /// Return the typed project id, or the explicit compatibility key for
    /// legacy requests that still use a directory-shaped project field.
    pub fn active_project_key(&self) -> Option<String> {
        self.project_execution_context()
            .ok()
            .map(|context| context.project_key())
    }

    /// Refresh project-local commands after an active-tab or asset-source
    /// change. Built-ins remain stable; only the explicit active root is
    /// consulted for project-local command files.
    pub fn refresh_project_command_registry(&mut self) {
        let Ok(context) = self.project_execution_context() else {
            self.command_registry = crate::tui::command::CommandRegistry::new();
            self.prompt_state.slash_completions = self
                .command_registry
                .commands()
                .iter()
                .map(
                    |command| crate::tui::components::completion_overlay::CompletionItem {
                        label: command.name.clone(),
                        description: (!command.description.is_empty())
                            .then(|| command.description.clone()),
                        kind: crate::tui::components::completion_overlay::CompletionItemKind::File,
                    },
                )
                .collect();
            self.dialog_state
                .command_palette
                .set_registry(&self.command_registry);
            return;
        };
        self.command_registry =
            crate::tui::command::CommandRegistry::new_for_workspace_root(&context.workspace_root);
        self.prompt_state.slash_completions = self
            .command_registry
            .commands()
            .iter()
            .map(
                |command| crate::tui::components::completion_overlay::CompletionItem {
                    label: command.name.clone(),
                    description: (!command.description.is_empty())
                        .then(|| command.description.clone()),
                    kind: crate::tui::components::completion_overlay::CompletionItemKind::File,
                },
            )
            .collect();
        self.dialog_state
            .command_palette
            .set_registry(&self.command_registry);
    }

    /// Guarantee the launch directory owns a session by the time the event
    /// loop starts.
    ///
    /// "The directory you ran the TUI in" is the session. Opening CodeGG in a
    /// folder and having to press `Ctrl+N` before the chat, file browser, or
    /// agent surfaces respond is a dead first impression: every one of those
    /// surfaces resolves its binding through the active session, so a launch
    /// that leaves `session` as `None` looks like a broken install rather than
    /// a missing keystroke.
    ///
    /// This is idempotent by construction — it is a no-op whenever a session
    /// is already bound, so `--session`, `--fork`, and `-c/--continue` keep
    /// exactly the session they asked for and this never races them. It runs
    /// after the launch-directory bootstrap so the request carries the typed
    /// `(project_id, workspace_id)` pair instead of a bare directory locator,
    /// which is the same shape `Ctrl+N` uses and the one the daemon can
    /// actually resolve.
    ///
    /// It is deliberately not `App::new_session`: that path clears the
    /// transcript, resets the route, and can open a template picker, none of
    /// which belong in a launch that has nothing to clear.
    pub fn ensure_launch_session(&mut self) {
        if self.session_state.session.is_some() {
            return;
        }
        crate::tui::commands::sessions::start_new_session(self);
    }

    /// Invalidate the frontend-only SessionCreate continuation.  The daemon
    /// request is intentionally not undone: it may already have committed a
    /// durable session, but that session must not be rebound to a new tab or
    /// retried implicitly by the frontend.
    ///
    /// Also invalidates an in-flight `turn.submit` completion. The daemon turn
    /// is not undone either — it keeps running and its events still arrive —
    /// but a late completion must not act on state the user has moved past.
    pub fn invalidate_pending_session_submit(&mut self, restore_prompt: bool) {
        let pending_session = self.prompt_state.cancel_session_submit();
        let pending_turn = self.cancel_turn_submit();
        if restore_prompt {
            if let Some(pending) = pending_session {
                self.prompt_state.restore_prompt_with_stash(&pending.prompt);
                self.messages_state.toasts.warning(
                    "Session creation was interrupted; review the prompt and press Enter to retry",
                );
            } else if let Some(prompt) = pending_turn {
                // A submit interrupted by a dropped connection is restored the
                // same way a refused one is. The daemon may well have accepted
                // it before the socket went away, so the user resubmits
                // explicitly: `retry_without_message` keeps that from adding a
                // second transcript entry and daemon-side turn admission keeps
                // it from becoming a second turn.
                self.prompt_state.restore_prompt_with_stash(&prompt);
                self.messages_state.toasts.warning(
                    "Turn submission was interrupted; review the prompt and press Enter to retry",
                );
            }
        }
        self.prompt_state.pending_send = false;
        if matches!(self.session_state.session_status, SessionStatus::Working) {
            self.session_state.session_status = SessionStatus::Idle;
        }
    }

    /// Switch the active tab and update projection client state to
    /// match. Returns `true` when the switch succeeded.
    pub fn switch_active_tab(&mut self, tab_id: &ProjectTabId) -> bool {
        let switched = self.project_tabs.set_active(tab_id);
        if switched {
            // M004: a tab switch moves the project context out from under
            // the popup's scope binding, so the popup closes even though
            // the attachment itself survives.
            self.editor_state.completion.clear();
            // C001: dropping the transient trigger bearer on scope loss.
            // Metadata stays cached; the secret is never carried across
            // projects/tabs.
            crate::tui::commands::work_orders::clear_trigger_secret_on_scope_loss(self);
            let project_scope = self.active_project_id().map(str::to_string);
            self.sidebar.set_project_scope(project_scope.as_deref());
            self.invalidate_pending_session_submit(false);
            self.projection_client
                .set_active_tab(Some(tab_id.as_str().to_string()));
            self.refresh_project_command_registry();
            // Presence M002: bounded refresh for the newly active
            // project; routing stays keyed by project_id.
            if let Some(pid) = self.active_project_id().map(str::to_string) {
                if self.presence.needs_refresh(&pid) {
                    crate::tui::commands::presence::start_refresh_presence(self, pid);
                }
            }
            // Project Collaboration M002: bounded chat refresh for the
            // newly active project; drafts/messages stay per-project so
            // rapid switching never cross-routes.
            if let Some(pid) = self.active_project_id().map(str::to_string) {
                if self.chat.needs_refresh(&pid) {
                    crate::tui::commands::chat::start_chat_history(self, pid);
                }
            }
        }
        switched
    }

    /// Notify the projection client that the underlying transport
    /// reconnect completed. Drops all subscription state and bumps
    /// the reconnect epoch.
    ///
    /// Interactive terminal attachments are transport-bound (M002
    /// `handle_disconnect`): the daemon released them server-side, so
    /// every live view moves to reconnecting with scrollback retained.
    /// Re-attach (`/terminal-attach`) resumes from the last cursor when
    /// the process still lives.
    pub fn on_projection_reconnect(&mut self) {
        self.invalidate_pending_session_submit(true);
        // C001: reconnect drops the transient bearer without logging it;
        // later metadata shows configuration and offers rotation.
        crate::tui::commands::work_orders::clear_trigger_secret_on_scope_loss(self);
        self.projection_client.on_reconnect();
        self.interactive_terminals.note_transport_disconnect();
        // Presence M002: lag/resync replaces stale presentation from the
        // authoritative snapshot. Bump the presence epoch (drops
        // pre-reconnect completions) and re-fetch the active project.
        self.presence.on_reconnect();
        self.routing_registry.bump_reconnect_epoch();
        if let Some(project_id) = self.active_project_id().map(str::to_string) {
            crate::tui::commands::presence::start_refresh_presence(self, project_id);
        }
        // Presence M003: observer reconnect replays from the authoritative
        // cursor. Bump the observer epoch (drops pre-reconnect
        // completions) and resume the active observation when present.
        // Revoked grants deny as `project_not_found` and clean transient
        // state daemon-side; multiple observers share no mutable
        // ownership (each holds its own subscription id).
        self.observer.on_reconnect();
        if self.observer.target().is_some() {
            crate::tui::commands::observe::resume_observe(self);
        }
        // Project Collaboration M002: reconnect resumes the M001 cursor
        // (`next_cursor`) or resyncs the bounded window. Bump the chat
        // epoch (drops pre-reconnect completions) and re-fetch the
        // chat target (M005: Workspace selection while the view is
        // active, otherwise the active project); an observer-target
        // disconnect leaves chat usable because chat refresh is
        // independent of observation.
        self.chat.on_reconnect();
        if let Some(project_id) = self.chat_target_project_id() {
            crate::tui::commands::chat::start_chat_history(self, project_id);
        }
        // Project Work Orders M004: reconnect rebuilds the open
        // dashboard from the daemon projection (fresh generation, one
        // bounded request); stale cached counts never survive an
        // epoch change and revocation clears via whole-page replace.
        crate::tui::commands::workspace_dashboard::resync_dashboard_after_reconnect(self);
        // TUI editor C001: a reconnect may have replaced the daemon replica.
        // Stale semantic spans must never paint against the resynced text;
        // diagnostics are marked stale rather than cleared so the last
        // complete set remains visible until the next refresh.
        self.editor_state.lsp.clear_semantic();
        // M004: a reconnect invalidates the popup and its pending request
        // outright — suggestions carry no resync semantics worth keeping.
        self.editor_state.completion.clear();
        if !self.editor_state.lsp.diagnostics.is_empty() {
            self.editor_state.lsp.diagnostics_stale = true;
        }
    }

    /// Presence M002: route a daemon `PresenceUpdated { project_id }`
    /// liveness hint into the bounded presence reducer. No collaborator
    /// detail is carried; the active project re-fetches through the
    /// authorized snapshot path. Inactive projects flag resync and
    /// refresh on foreground (no polling storm).
    pub fn on_presence_hint(&mut self, project_id: String) {
        if self.presence.note_hint(&project_id) {
            let is_active = self.active_project_id() == Some(project_id.as_str());
            if is_active {
                crate::tui::commands::presence::start_refresh_presence(self, project_id);
            }
        }
    }

    /// Apply a projection envelope to the projection client. The
    /// caller is responsible for routing the envelope to the correct
    /// tab; this method only updates the projection controller state.
    pub fn apply_projection_envelope(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
        tab_id: &str,
        envelope: ProjectionEnvelope,
    ) -> ControllerApplyOutcome {
        self.projection_client
            .apply_envelope(subscription_id, tab_id, envelope)
    }

    /// Enter raw compatibility mode. Called when the daemon does not
    /// advertise projection capability or when the user explicitly
    /// disables projection-primary mode.
    pub fn enter_projection_raw_compatibility(&mut self, reason: impl Into<String>) {
        self.projection_client.enter_raw_compatibility(reason);
    }
}
