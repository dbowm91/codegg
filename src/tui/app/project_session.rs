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

    /// Return the active workspace root for legacy local services. The
    /// value is still resolved from the active tab, never from process cwd.
    pub fn active_workspace_root(&self) -> Option<PathBuf> {
        self.project_execution_context()
            .ok()
            .map(|context| context.workspace_root)
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

    /// Invalidate the frontend-only SessionCreate continuation.  The daemon
    /// request is intentionally not undone: it may already have committed a
    /// durable session, but that session must not be rebound to a new tab or
    /// retried implicitly by the frontend.
    pub fn invalidate_pending_session_submit(&mut self, restore_prompt: bool) {
        let pending = self.prompt_state.cancel_session_submit();
        if restore_prompt {
            if let Some(pending) = pending {
                let current_draft = self.prompt_state.prompt.get_text();
                if !current_draft.trim().is_empty() && current_draft != pending.prompt {
                    if self.prompt_state.stashed_prompts.len() >= 100 {
                        self.prompt_state.stashed_prompts.remove(0);
                    }
                    self.prompt_state.stashed_prompts.push(current_draft);
                }
                self.prompt_state.prompt.set_text(pending.prompt.clone());
                self.prompt_state.mark_retry_without_message(pending.prompt);
                self.messages_state.toasts.warning(
                    "Session creation was interrupted; review the prompt and press Enter to retry",
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
        // active project; an observer-target disconnect leaves chat
        // usable because chat refresh is independent of observation.
        self.chat.on_reconnect();
        if let Some(project_id) = self.active_project_id().map(str::to_string) {
            crate::tui::commands::chat::start_chat_history(self, project_id);
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
