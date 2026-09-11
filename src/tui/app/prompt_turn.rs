//! Prompt submission and turn-start lifecycle.
//!
//! This module captures immutable route/session context before a turn starts
//! and leaves asynchronous session creation to the existing command/runtime
//! continuation. It owns no daemon truth and never crosses an await boundary
//! with a mutable App reference.

use super::{send_tui, App, TuiCommand};
use crate::protocol::tui::TuiMessage as RemoteTuiMessage;
use crate::tui::app::state::AppMode;
use crate::tui::app::types::{HistoryEntry, SessionStatus};
use crate::tui::route::Route;

impl App {
    pub(crate) fn send_prompt(&mut self) {
        let text = self.prompt_state.prompt.get_text();
        let trimmed_text = text.trim().to_string();
        tracing::debug!(target: "codegg::tui::app",
            "send_prompt: text='{}', trimmed='{}', pending_send={}",
            text,
            trimmed_text,
            self.prompt_state.pending_send
        );

        if trimmed_text.is_empty() {
            tracing::debug!(target: "codegg::tui::app", "send_prompt: returning - trimmed text is empty");
            return;
        }
        if self.prompt_state.pending_send {
            self.messages_state.toasts.warning(
                if self.prompt_state.pending_session_submit.is_some() {
                    "Still creating a session for the previous prompt"
                } else {
                    "Still waiting for previous prompt to finish"
                },
            );
            tracing::debug!(target: "codegg::tui::app", "send_prompt: returning - pending_send already true");
            return;
        }
        // Presence M003: observer mode is explicitly read-only. Slash
        // commands route to the allowlisted dispatcher; bare insert-mode
        // input (chat, human-shell `!`) routes to project chat via the
        // M002 collaboration seam and is never sent as a turn.
        if self.observer.blocks_prompt_submit() {
            if trimmed_text.starts_with('/') {
                if self.handle_slash_command(&text) {
                    tracing::debug!(target: "codegg::tui::app", "send_prompt: handled slash command, clearing prompt");
                    self.prompt_state.prompt.clear();
                    self.prompt_state.show_completions = false;
                    return;
                }
                self.messages_state.toasts.warning(
                    &crate::tui::app::state::observe::observer_blocked_message(&trimmed_text),
                );
                self.prompt_state.prompt.clear();
                self.prompt_state.show_completions = false;
                return;
            }
            // M002: observer insert-mode text targets the observed
            // project's chat. Zero turn steering/control flows through
            // this path — only `Chat*` core requests are issued.
            if self.route_observer_text_to_chat(&text) {
                tracing::debug!(target: "codegg::tui::app", "send_prompt: routed observer input to project chat");
                return;
            }
            if let Some(placeholder) = self.observer.collaboration_input_placeholder() {
                self.messages_state.toasts.warning(placeholder.as_str());
            }
            self.prompt_state.prompt.clear();
            self.prompt_state.show_completions = false;
            return;
        }
        if self.handle_slash_command(&text) {
            tracing::debug!(target: "codegg::tui::app", "send_prompt: handled slash command, clearing prompt");
            self.prompt_state.prompt.clear();
            self.prompt_state.show_completions = false;
            return;
        }

        match crate::shell::classify_prompt_submission(&trimmed_text) {
            crate::shell::types::PromptSubmissionKind::HumanShell {
                command,
                promote_after,
            } => {
                tracing::debug!(target: "codegg::tui::app", "send_prompt: intercepted human shell command: {}", command);
                self.prompt_state.prompt.clear();
                self.prompt_state.show_completions = false;
                let cwd = match self.project_execution_context() {
                    Ok(context) => context.workspace_root,
                    Err(error) => {
                        self.messages_state.toasts.error(&error);
                        return;
                    }
                };
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(
                        tx,
                        TuiCommand::RunHumanShell {
                            command,
                            promote_after,
                            cwd,
                        },
                    );
                }
                return;
            }
            crate::shell::types::PromptSubmissionKind::Slash(s) => {
                let _ = &s;
                tracing::debug!(target: "codegg::tui::app", "send_prompt: slash command via classify: {}", s);
            }
            crate::shell::types::PromptSubmissionKind::Chat(_) => {}
        }

        // Capture the project/workspace route before mutating the visible
        // message state.  A no-session prompt must never fall back to the tab
        // that happens to be active when SessionCreate completes.
        let needs_session_create = !matches!(self.ui_state.mode, AppMode::RemoteCore { .. })
            && self.session_state.session.is_none();
        let pending_session_route = if needs_session_create {
            let context = match self.project_execution_context() {
                Ok(context) => context,
                Err(error) => {
                    self.messages_state.toasts.error(&error);
                    return;
                }
            };
            let tab_id = match self.active_tab_id() {
                Some(tab_id) => tab_id,
                None => {
                    self.messages_state
                        .toasts
                        .error("No active project tab; choose a project before sending");
                    return;
                }
            };
            let request_id = self.prompt_state.session_submit_request.begin();
            Some((
                context,
                crate::tui::app::state::UiRouteToken::new(
                    Some(tab_id),
                    self.active_project_id().map(str::to_string),
                    self.active_workspace_id().map(str::to_string),
                    self.active_session_id().map(str::to_string),
                    self.view_switch.active_view_epoch,
                    self.routing_registry.reconnect_epoch,
                    request_id,
                ),
                request_id,
            ))
        } else {
            None
        };

        if let Some(pos) = self
            .session_state
            .history
            .iter()
            .position(|e| e.text == trimmed_text)
        {
            self.session_state.history[pos].touch();
        } else {
            if self.session_state.history.len() >= 1000 {
                self.session_state.history.pop_front();
            }
            self.session_state
                .history
                .push_back(HistoryEntry::new(trimmed_text.clone()));
        }
        let history_vec: Vec<HistoryEntry> = self.session_state.history.iter().cloned().collect();
        let mut sorted = history_vec;
        sorted.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.session_state.history = sorted.into();
        self.session_state.history_pos = None;

        let is_retry = self.prompt_state.take_retry_without_message(&trimmed_text);
        if !is_retry {
            self.messages_state
                .messages
                .add_user_message(trimmed_text.clone(), Some(self.agent_state.plan_mode));
        }
        self.prompt_state.prompt.clear();
        self.prompt_state.show_completions = false;
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            self.send_remote_message(RemoteTuiMessage::Input {
                text: text.trim().to_string(),
            });
            self.prompt_state.pending_send = false;
        } else {
            self.prompt_state.pending_send = true;
            if let Some((context, route, request_id)) = pending_session_route {
                self.prompt_state.pending_session_submit =
                    Some(crate::tui::app::state::PendingSessionSubmit {
                        request_id,
                        prompt: trimmed_text.clone(),
                        context,
                        route,
                    });
                self.prompt_state.prompt.set_waiting(true);
            }
        }
        self.session_state.session_status = SessionStatus::Working;
        self.reset_live_token_estimate();

        // Navigate to session view when user sends a prompt
        let session_id = self.session_state.session.as_ref().map(|s| s.id.clone());
        if let Some(ref sid) = session_id {
            self.ui_state
                .routes
                .navigate_to(Route::Session(sid.clone()));
        } else {
            // Navigate to a placeholder session route - will be updated when real session is created
            self.ui_state
                .routes
                .navigate_to(Route::Session("pending".to_string()));
        }

        tracing::debug!(target: "codegg::tui::app", "send_prompt: completed - pending_send set to true, status=Working");
    }
}
