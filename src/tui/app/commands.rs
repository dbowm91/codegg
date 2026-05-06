use super::App;

use super::components::dialogs::command::CommandPalette;
use crate::tui::command::Command;
use std::sync::Arc;

#[cfg(feature = "debug-logging")]
use std::fs::OpenOptions;

#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open("codegg_debug.log")
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, format!("[TUI-DEBUG] {}\n", format!($($arg)*)).as_bytes())
            });
    };
}

#[cfg(not(feature = "debug-logging"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

impl App {
    pub fn on_paste(&mut self, text: String) {
        if self.ui_state.command_mode {
            self.prompt_state.prompt.paste(text);
            let query = self.prompt_state.prompt.get_text();
            self.dialog_state.command_palette.set_query(&query);
        } else if self.ui_state.dialog.is_open() {
            match &mut self.ui_state.dialog {
                super::Dialog::Session => {
                    self.dialog_state.session_dialog.filter.push_str(&text);
                }
                super::Dialog::Connect => {
                    if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                        if cd.step
                            == crate::tui::components::dialogs::connect::ConnectStep::EnterApiKey
                        {
                            cd.api_key_input.push_str(&text);
                            cd.cursor_pos = cd.api_key_input.len();
                        }
                    }
                }
                _ => {
                    self.prompt_state.prompt.paste(text);
                }
            }
        } else {
            self.prompt_state.prompt.paste(text);
        }
    }

    pub fn on_resize(&mut self) {
        self.ui_state.auto_scroll = true;
    }

    fn handle_slash_command(&mut self, text: &str) -> bool {
        if !text.starts_with('/') {
            return false;
        }

        let cmd = text.trim();
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let cmd_name = parts[0].to_lowercase();

        match cmd_name.as_str() {
            "/help" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Help);
            }
            "/tree" => {
                self.ui_state.command_mode = false;
                self.open_tree_dialog();
            }
            "/model" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Model);
            }
            "/agent" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Agent);
            }
            "/clear" | "/new" => {
                self.clear_session();
            }
            "/compact" => {
                self.messages_state
                    .toasts
                    .info("Compaction triggered - reducing context");
            }
            "/connect" => {
                self.ui_state.command_mode = false;
                self.open_connect_dialog();
            }
            "/status" => {
                self.messages_state.toasts.info(&format!(
                    "status: {:?} | tokens: {}↑ {}↓ | model: {}",
                    self.session_state.session_status,
                    self.session_state.token_in,
                    self.session_state.token_out,
                    self.agent_state
                        .current_model
                        .split('/')
                        .next_back()
                        .unwrap_or(&self.agent_state.current_model)
                ));
            }
            "/context" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Context);
            }
            "/cost" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Cost);
            }
            "/usage" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Usage);
            }
            "/themes" => {
                self.open_dialog(super::Dialog::Theme);
            }
            "/tui" => {
                self.toggle_fullscreen();
            }
            "/sessions" => {
                self.open_dialog(super::Dialog::Session);
            }
            "/goto" => {
                let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
                let msg_count = self.messages_state.messages.items.len();
                let mut dialog = crate::tui::components::dialogs::goto::GotoDialog::new(msg_count);
                dialog.set_theme(&self.ui_state.theme);
                if parts.len() > 1 {
                    for c in parts[1].chars() {
                        dialog.append_char(c);
                    }
                    if dialog.is_valid() {
                        if let Some(idx) = dialog.get_index() {
                            self.messages_state.messages.sel_msg = Some(idx);
                            return true;
                        }
                    }
                }
                self.dialog_state.goto_dialog = Some(dialog);
                self.open_dialog(super::Dialog::Goto);
            }
            "/share" => {
                if let Some(ref session) = self.session_state.session {
                    if let Some(ref existing) = session.share_url {
                        let mut dialog = crate::tui::components::dialogs::share::ShareDialog::new(Arc::clone(&self.ui_state.theme));
                        dialog.set_theme(&self.ui_state.theme);
                        dialog.set_url(existing.clone());
                        self.dialog_state.share_dialog = Some(dialog);
                        self.open_dialog(super::Dialog::Share);
                    } else if self.tui_cmd_tx.is_some() {
                        let session_id = session.id.clone();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(super::TuiCommand::ShareSession { session_id });
                        }
                    } else {
                        self.messages_state
                            .toasts
                            .error("Session store not available");
                    }
                } else {
                    self.messages_state
                        .toasts
                        .info("No active session to share");
                }
            }
            "/unshare" => {
                if self.session_state.session.is_some() {
                    self.messages_state.toasts.info("Session unshared");
                } else {
                    self.messages_state.toasts.info("No active session");
                }
            }
            "/rename" => {
                self.messages_state
                    .toasts
                    .info("Use /sessions to rename - select and press Enter");
            }
            "/timeline" => {
                self.show_timeline();
            }
            "/undo" => {
                if self.messages_state.messages.undo() {
                    self.messages_state.toasts.info("Undid last message");
                } else {
                    self.messages_state.toasts.info("Nothing to undo");
                }
            }
            "/redo" => {
                if self.messages_state.messages.redo() {
                    self.messages_state.toasts.info("Redid message");
                } else {
                    self.messages_state.toasts.info("Nothing to redo");
                }
            }
            "/export" => {
                self.messages_state
                    .toasts
                    .info("Exporting session - copy to clipboard");
            }
            "/import" => {
                self.dialog_state.import_dialog =
                    Some(crate::tui::components::dialogs::import::ImportDialog::new(Arc::clone(&self.ui_state.theme)));
                self.open_dialog(super::Dialog::Import);
            }
            "/timestamps" => {
                self.ui_state.show_timestamps = !self.ui_state.show_timestamps;
                let msg = if self.ui_state.show_timestamps {
                    "timestamps shown"
                } else {
                    "timestamps hidden"
                };
                self.messages_state.toasts.info(msg);
            }
            "/thinking" => {
                self.ui_state.show_thinking = !self.ui_state.show_thinking;
                let msg = if self.ui_state.show_thinking {
                    "thinking shown"
                } else {
                    "thinking hidden"
                };
                self.messages_state.toasts.info(msg);
            }
            "/models-refresh" | "/refresh-models" => {
                self.messages_state
                    .toasts
                    .info("Refreshing model cache in background...");
                let pool = self.session_store.as_ref().map(|s| s.pool().clone());
                if let Some(pool) = pool {
                    tokio::spawn(async move {
                        let config = crate::config::schema::Config::load().unwrap_or_default();
                        let mut registry = crate::provider::ProviderRegistry::new();
                        crate::provider::register_builtin_with_config(&mut registry, &config);
                        let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                            std::path::PathBuf::new(),
                        )
                        .with_pool(pool);
                        discovery.refresh(&registry).await;
                        tracing::info!("Model cache refreshed from providers");
                    });
                } else {
                    self.messages_state
                        .toasts
                        .warning("No database connection for refresh");
                }
            }
            "/variants" => {
                let model = &self.agent_state.current_model;
                let base = model.split('/').next_back().unwrap_or(model);
                self.messages_state
                    .toasts
                    .info(&format!("Variants for {}: default (no suffix)", base));
            }
            "/tasks" => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(super::TuiCommand::ListTasks);
                } else {
                    self.messages_state.toasts.info("No background tasks");
                }
            }
            "/task-del" => {
                let query = self.dialog_state.command_palette.query.clone();
                let id = query.trim();
                if id.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /task-del <id> (use /tasks to see IDs)");
                } else if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(super::TuiCommand::DeleteTask { id: id.to_string() });
                } else {
                    self.messages_state.toasts.warning("Scheduler not available");
                }
            }
            _ => return false,
        }

        true
    }

    pub fn on_paste(&mut self, text: String) {
        if self.ui_state.command_mode {
            self.prompt_state.prompt.paste(text);
            let query = self.prompt_state.prompt.get_text();
            self.dialog_state.command_palette.set_query(&query);
        } else if self.ui_state.dialog.is_open() {
            match &mut self.ui_state.dialog {
                super::Dialog::Session => {
                    self.dialog_state.session_dialog.filter.push_str(&text);
                }
                super::Dialog::Connect => {
                    if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                        if cd.step
                            == crate::tui::components::dialogs::connect::ConnectStep::EnterApiKey
                        {
                            cd.api_key_input.push_str(&text);
                            cd.cursor_pos = cd.api_key_input.len();
                        }
                    }
                }
                _ => {
                    self.prompt_state.prompt.paste(text);
                }
            }
        } else {
            self.prompt_state.prompt.paste(text);
        }
    }

    pub fn on_resize(&mut self) {
        self.ui_state.auto_scroll = true;
    }

    fn handle_slash_command(&mut self, text: &str) -> bool {
        if !text.starts_with('/') {
            return false;
        }

        let cmd = text.trim();
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let cmd_name = parts[0].to_lowercase();

        match cmd_name.as_str() {
            "/help" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Help);
            }
            "/tree" => {
                self.ui_state.command_mode = false;
                self.open_tree_dialog();
            }
            "/model" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Model);
            }
            "/agent" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Agent);
            }
            "/clear" | "/new" => {
                self.clear_session();
            }
            "/compact" => {
                self.messages_state
                    .toasts
                    .info("Compaction triggered - reducing context");
            }
            "/connect" => {
                self.ui_state.command_mode = false;
                self.open_connect_dialog();
            }
            "/status" => {
                self.messages_state.toasts.info(&format!(
                    "status: {:?} | tokens: {}↑ {}↓ | model: {}",
                    self.session_state.session_status,
                    self.session_state.token_in,
                    self.session_state.token_out,
                    self.agent_state
                        .current_model
                        .split('/')
                        .next_back()
                        .unwrap_or(&self.agent_state.current_model)
                ));
            }
            "/context" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Context);
            }
            "/cost" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Cost);
            }
            "/usage" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Usage);
            }
            "/themes" => {
                self.open_dialog(super::Dialog::Theme);
            }
            "/tui" => {
                self.toggle_fullscreen();
            }
            "/sessions" => {
                self.open_dialog(super::Dialog::Session);
            }
            "/goto" => {
                let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
                let msg_count = self.messages_state.messages.items.len();
                let mut dialog = crate::tui::components::dialogs::goto::GotoDialog::new(msg_count);
                dialog.set_theme(&self.ui_state.theme);
                if parts.len() > 1 {
                    for c in parts[1].chars() {
                        dialog.append_char(c);
                    }
                    if dialog.is_valid() {
                        if let Some(idx) = dialog.get_index() {
                            self.messages_state.messages.sel_msg = Some(idx);
                            return true;
                        }
                    }
                }
                self.dialog_state.goto_dialog = Some(dialog);
                self.open_dialog(super::Dialog::Goto);
            }
            "/share" => {
                if let Some(ref session) = self.session_state.session {
                    if let Some(ref existing) = session.share_url {
                        let mut dialog = crate::tui::components::dialogs::share::ShareDialog::new(Arc::clone(&self.ui_state.theme));
                        dialog.set_theme(&self.ui_state.theme);
                        dialog.set_url(existing.clone());
                        self.dialog_state.share_dialog = Some(dialog);
                        self.open_dialog(super::Dialog::Share);
                    } else if self.tui_cmd_tx.is_some() {
                        let session_id = session.id.clone();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(super::TuiCommand::ShareSession { session_id });
                        }
                    } else {
                        self.messages_state
                            .toasts
                            .error("Session store not available");
                    }
                } else {
                    self.messages_state
                        .toasts
                        .info("No active session to share");
                }
            }
            "/unshare" => {
                if self.session_state.session.is_some() {
                    self.messages_state.toasts.info("Session unshared");
                } else {
                    self.messages_state.toasts.info("No active session");
                }
            }
            "/rename" => {
                self.messages_state
                    .toasts
                    .info("Use /sessions to rename - select and press Enter");
            }
            "/timeline" => {
                self.show_timeline();
            }
            "/undo" => {
                if self.messages_state.messages.undo() {
                    self.messages_state.toasts.info("Undid last message");
                } else {
                    self.messages_state.toasts.info("Nothing to undo");
                }
            }
            "/redo" => {
                if self.messages_state.messages.redo() {
                    self.messages_state.toasts.info("Redid message");
                } else {
                    self.messages_state.toasts.info("Nothing to redo");
                }
            }
            "/export" => {
                self.messages_state
                    .toasts
                    .info("Exporting session - copy to clipboard");
            }
            "/import" => {
                self.dialog_state.import_dialog =
                    Some(crate::tui::components::dialogs::import::ImportDialog::new(Arc::clone(&self.ui_state.theme)));
                self.open_dialog(super::Dialog::Import);
            }
            "/timestamps" => {
                self.ui_state.show_timestamps = !self.ui_state.show_timestamps;
                let msg = if self.ui_state.show_timestamps {
                    "timestamps shown"
                } else {
                    "timestamps hidden"
                };
                self.messages_state.toasts.info(msg);
            }
            "/thinking" => {
                self.ui_state.show_thinking = !self.ui_state.show_thinking;
                let msg = if self.ui_state.show_thinking {
                    "thinking shown"
                } else {
                    "thinking hidden"
                };
                self.messages_state.toasts.info(msg);
            }
            "/models-refresh" | "/refresh-models" => {
                self.messages_state
                    .toasts
                    .info("Refreshing model cache in background...");
                let pool = self.session_store.as_ref().map(|s| s.pool().clone());
                if let Some(pool) = pool {
                    tokio::spawn(async move {
                        let config = crate::config::schema::Config::load().unwrap_or_default();
                        let mut registry = crate::provider::ProviderRegistry::new();
                        crate::provider::register_builtin_with_config(&mut registry, &config);
                        let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                            std::path::PathBuf::new(),
                        )
                        .with_pool(pool);
                        discovery.refresh(&registry).await;
                        tracing::info!("Model cache refreshed from providers");
                    });
                } else {
                    self.messages_state
                        .toasts
                        .warning("No database connection for refresh");
                }
            }
            "/variants" => {
                let model = &self.agent_state.current_model;
                let base = model.split('/').next_back().unwrap_or(model);
                self.messages_state
                    .toasts
                    .info(&format!("Variants for {}: default (no suffix)", base));
            }
            "/tasks" => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(super::TuiCommand::ListTasks);
                } else {
                    self.messages_state.toasts.info("No background tasks");
                }
            }
            _ => return false,
        }

        true
    }

    fn execute_command(&mut self, cmd: &Command) {
        if let Some(dialog) = &cmd.dialog {
            self.ui_state.command_mode = false;
            self.open_dialog(dialog.clone());
            return;
        }
        match cmd.name.as_str() {
            "/exit" | "/quit" | "/q" => {
                self.ui_state.running = false;
                let _ = self.ui_state.shutdown_tx.take().map(|tx| tx.send(()));
            }
            "/help" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Help);
            }
            "/tree" => {
                self.ui_state.command_mode = false;
                self.open_tree_dialog();
            }
            "/model" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Model);
            }
            "/agent" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Agent);
            }
            "/clear" | "/new" => {
                self.clear_session();
            }
            "/compact" => {
                self.messages_state
                    .toasts
                    .info("Compaction triggered - reducing context");
            }
            "/connect" => {
                self.ui_state.command_mode = false;
                self.open_connect_dialog();
            }
            "/status" => {
                self.messages_state.toasts.info(&format!(
                    "status: {:?} | tokens: {}↑ {}↓ | model: {}",
                    self.session_state.session_status,
                    self.session_state.token_in,
                    self.session_state.token_out,
                    self.agent_state
                        .current_model
                        .split('/')
                        .next_back()
                        .unwrap_or(&self.agent_state.current_model)
                ));
            }
            "/context" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Context);
            }
            "/cost" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Cost);
            }
            "/usage" => {
                self.ui_state.command_mode = false;
                self.open_dialog(super::Dialog::Usage);
            }
            "/themes" => {
                self.open_dialog(super::Dialog::Theme);
            }
            "/tui" => {
                self.toggle_fullscreen();
            }
            "/sessions" => {
                self.open_dialog(super::Dialog::Session);
            }
            "/goto" => {
                let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
                let msg_count = self.messages_state.messages.items.len();
                let mut dialog = crate::tui::components::dialogs::goto::GotoDialog::new(msg_count);
                dialog.set_theme(&self.ui_state.theme);
                if parts.len() > 1 {
                    for c in parts[1].chars() {
                        dialog.append_char(c);
                    }
                    if dialog.is_valid() {
                        if let Some(idx) = dialog.get_index() {
                            self.messages_state.messages.sel_msg = Some(idx);
                        }
                    }
                }
                self.dialog_state.goto_dialog = Some(dialog);
                self.open_dialog(super::Dialog::Goto);
            }
            "/share" => {
                if let Some(ref session) = self.session_state.session {
                    if let Some(ref existing) = session.share_url {
                        let mut dialog = crate::tui::components::dialogs::share::ShareDialog::new(Arc::clone(&self.ui_state.theme));
                        dialog.set_theme(&self.ui_state.theme);
                        dialog.set_url(existing.clone());
                        self.dialog_state.share_dialog = Some(dialog);
                        self.open_dialog(super::Dialog::Share);
                    } else if self.tui_cmd_tx.is_some() {
                        let session_id = session.id.clone();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(super::TuiCommand::ShareSession { session_id });
                        }
                    } else {
                        self.messages_state
                            .toasts
                            .error("Session store not available");
                    }
                } else {
                    self.messages_state
                        .toasts
                        .info("No active session to share");
                }
            }
            "/unshare" => {
                if self.session_state.session.is_some() {
                    self.messages_state.toasts.info("Session unshared");
                } else {
                    self.messages_state.toasts.info("No active session");
                }
            }
            "/rename" => {
                self.messages_state
                    .toasts
                    .info("Use /sessions to rename - select and press Enter");
            }
            "/timeline" => {
                self.show_timeline();
            }
            "/undo" => {
                if self.messages_state.messages.undo() {
                    self.messages_state.toasts.info("Undid last message");
                } else {
                    self.messages_state.toasts.info("Nothing to undo");
                }
            }
            "/redo" => {
                if self.messages_state.messages.redo() {
                    self.messages_state.toasts.info("Redid message");
                } else {
                    self.messages_state.toasts.info("Nothing to redo");
                }
            }
            "/export" => {
                self.messages_state
                    .toasts
                    .info("Exporting session - copy to clipboard");
            }
            "/import" => {
                self.dialog_state.import_dialog =
                    Some(crate::tui::components::dialogs::import::ImportDialog::new(Arc::clone(&self.ui_state.theme)));
                self.open_dialog(super::Dialog::Import);
            }
            "/timestamps" => {
                self.ui_state.show_timestamps = !self.ui_state.show_timestamps;
                let msg = if self.ui_state.show_timestamps {
                    "timestamps shown"
                } else {
                    "timestamps hidden"
                };
                self.messages_state.toasts.info(msg);
            }
            "/thinking" => {
                self.ui_state.show_thinking = !self.ui_state.show_thinking;
                let msg = if self.ui_state.show_thinking {
                    "thinking shown"
                } else {
                    "thinking hidden"
                };
                self.messages_state.toasts.info(msg);
            }
            "/variants" => {
                let model = &self.agent_state.current_model;
                let base = model.split('/').next_back().unwrap_or(model);
                self.messages_state
                    .toasts
                    .info(&format!("Variants for {}: default (no suffix)", base));
            }
            "/tasks" => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(super::TuiCommand::ListTasks);
                } else {
                    self.messages_state.toasts.info("No background tasks");
                }
            }
            _ => {}
        }
    }
}