//! Synchronous component and user-intent routing.
//!
//! process_msg is the adapter from component/user intent (TuiMsg) to
//! immediate UI state changes or explicit runtime effects (TuiCommand).
//! Async completions never enter through this path; they are applied by the
//! runtime dispatch boundary.

use super::*;

impl App {
    pub fn process_msg(&mut self, msg: TuiMsg) {
        tracing::debug!(target: "codegg::tui::app", "process_msg: {:?}", msg);
        match msg {
            TuiMsg::SubmitPrompt => self.send_prompt(),
            TuiMsg::NavigateUp => self.navigate_up(),
            TuiMsg::NavigateDown => self.navigate_down(),
            TuiMsg::CycleAgent => self.cycle_agent(),
            TuiMsg::OpenModelDialog => self.open_dialog(Dialog::Model),
            TuiMsg::OpenAgentDialog => self.open_dialog(Dialog::Agent),
            TuiMsg::OpenSessionDialog => self.open_dialog(Dialog::Session),
            TuiMsg::OpenHelpDialog => self.open_dialog(Dialog::Help),
            TuiMsg::OpenTreeDialog => {
                self.enqueue_tui_command(TuiCommand::OpenTreeDialog);
            }
            TuiMsg::OpenThemeDialog => self.open_dialog(Dialog::Theme),
            TuiMsg::OpenShareDialog => {
                if let Some(ref session) = self.session_state.session {
                    let session_id = session.id.clone();
                    self.enqueue_tui_command(TuiCommand::ShareSession { session_id });
                }
            }

            TuiMsg::OpenImportDialog => self.open_dialog(Dialog::Import),
            TuiMsg::OpenDiffDialog {
                old_content,
                new_content,
                title,
            } => {
                self.enqueue_tui_command(TuiCommand::OpenDiffDialog {
                    old_content,
                    new_content,
                    title,
                });
            }
            TuiMsg::SelectModel { model } => {
                self.agent_state.current_model = model.clone();
                self.dialog_state
                    .model_dialog
                    .set_current(&self.agent_state.current_model);
                if let Some(idx) = self.agent_state.models.iter().position(|m| m == &model) {
                    self.agent_state.model_idx = idx;
                }
                if let Some(tab) = self.project_tabs.active_mut() {
                    tab.model = model.clone();
                }
                self.persist_model_selection(&model);
                self.close_dialog();
            }
            TuiMsg::SelectAgent { agent_name } => {
                if let Some(idx) = self
                    .agent_state
                    .agents
                    .iter()
                    .position(|a| a.name == agent_name)
                {
                    self.agent_state.current_agent = idx;
                }
                if let Some(tab) = self.project_tabs.active_mut() {
                    tab.agent = agent_name.clone();
                }
                self.close_dialog();
            }
            TuiMsg::SelectSession(session) => {
                // Cancel research and memory tasks from the previous session.
                use crate::tui::task_lifecycle::TuiTaskKind;
                self.task_registry.cancel_kind(TuiTaskKind::Research);
                self.task_registry.cancel_kind(TuiTaskKind::Memory);
                self.task_registry.cancel_kind(TuiTaskKind::GitStatus);
                self.set_session(*session);
                self.close_dialog();
                // Refresh sidebar git status for the new project.
                crate::tui::commands::git_sidebar::start_refresh_git_sidebar(self);
            }
            TuiMsg::SubmitConnect => {
                self.handle_connect_send();
            }
            TuiMsg::CloseDialog => {
                self.close_dialog();
            }
            TuiMsg::ConfirmResult(confirmed) => {
                self.close_dialog();
                if confirmed == Some(true) {
                    if let Some((action, connection_id, expected_revision)) =
                        self.dialog_state.pending_connection_lifecycle.take()
                    {
                        let _ = self.enqueue_tui_command(TuiCommand::ConnectionLifecycle {
                            action,
                            connection_id,
                            expected_revision,
                        });
                    } else if let Some(session_id) = self.dialog_state.pending_delete_session.take()
                    {
                        let undo_id = session_id.clone();
                        if self.enqueue_tui_command(TuiCommand::DeleteSession { session_id }) {
                            self.undo_session_id = Some(undo_id);
                            self.undo_until =
                                Some(Instant::now() + std::time::Duration::from_secs(30));
                            self.status_bar
                                .set_undo_message("Session deleted — press U to undo");
                        }
                    } else if let Some((session_id, unarchive)) =
                        self.dialog_state.pending_archive_session.take()
                    {
                        self.enqueue_tui_command(TuiCommand::ArchiveSession {
                            session_id,
                            unarchive,
                        });
                    } else if let Some(_count) = self.dialog_state.pending_bulk_delete.take() {
                        let ids = self
                            .dialog_state
                            .pending_bulk_delete_ids
                            .take()
                            .unwrap_or_default();
                        if self.enqueue_tui_command(TuiCommand::BulkDelete { session_ids: ids }) {
                            self.dialog_state.session_dialog.toggle_bulk_mode();
                        }
                    } else if let Some((_count, unarchive)) =
                        self.dialog_state.pending_bulk_archive.take()
                    {
                        let ids = self
                            .dialog_state
                            .pending_bulk_archive_ids
                            .take()
                            .unwrap_or_default();
                        if self.enqueue_tui_command(TuiCommand::BulkArchive {
                            session_ids: ids,
                            unarchive,
                        }) {
                            self.dialog_state.session_dialog.toggle_bulk_mode();
                        }
                    } else if let Some((command, promote_after, cwd)) =
                        self.dialog_state.pending_shell_command.take()
                    {
                        self.enqueue_tui_command(TuiCommand::RunHumanShell {
                            command,
                            promote_after,
                            cwd,
                        });
                    }
                } else {
                    self.dialog_state.pending_delete_session = None;
                    self.dialog_state.pending_archive_session = None;
                    self.dialog_state.pending_bulk_delete = None;
                    self.dialog_state.pending_bulk_delete_ids = None;
                    self.dialog_state.pending_bulk_archive = None;
                    self.dialog_state.pending_bulk_archive_ids = None;
                    self.dialog_state.pending_shell_command = None;
                    self.dialog_state.pending_connection_lifecycle = None;
                }
            }
            TuiMsg::McpAction {
                server_name,
                action,
            } => {
                match action.as_str() {
                    "Configure OAuth" => {
                        self.messages_state.toasts.info(&format!(
                            "OAuth for {} - configure in .codegg/mcp.json",
                            server_name
                        ));
                    }
                    "Browse Resources" => {
                        if let Some(ref mut mcp) = self.dialog_state.mcp_dialog {
                            mcp.browse_mode = BrowseMode::Resources { selected: 0 };
                            mcp.action_mode = false;
                        }
                        self.close_dialog();
                        return;
                    }
                    "Disconnect" => {
                        self.messages_state
                            .toasts
                            .info(&format!("Disconnecting {}...", server_name));
                    }
                    "Reconnect" => {
                        self.messages_state
                            .toasts
                            .info(&format!("Reconnecting {}...", server_name));
                    }
                    "Connect" => {
                        self.messages_state
                            .toasts
                            .info(&format!("Connecting {}...", server_name));
                    }
                    "Remove" => {
                        self.messages_state
                            .toasts
                            .info(&format!("Removing {}...", server_name));
                    }
                    "Wait" => {
                        self.messages_state
                            .toasts
                            .info("Server is connecting, please wait...");
                    }
                    "Configure" => {
                        self.messages_state.toasts.info(&format!(
                            "Configure {} - edit .codegg/mcp.json",
                            server_name
                        ));
                    }
                    _ => {}
                }
                self.close_dialog();
            }
            TuiMsg::KeybindChanged {
                action: _,
                binding: _,
            } => {
                if let Some(ref kd) = self.dialog_state.keybind_dialog {
                    if let Some(keybinds) = &mut self.ui_state.keybinds {
                        keybinds.bindings = kd.bindings.clone();
                    } else {
                        self.ui_state.keybinds = Some(crate::tui::input::KeybindConfig {
                            bindings: kd.bindings.clone(),
                        });
                    }
                }
                self.close_dialog();
            }
            TuiMsg::ConfirmDeleteSession { session_id } => {
                let msg = "Delete this session? This cannot be undone.".to_string();
                self.dialog_state.pending_delete_session = Some(session_id.clone());
                self.push_dialog(
                    Dialog::Confirm,
                    Box::new(ConfirmDialog::new("Delete Session".to_string(), msg)),
                );
            }
            TuiMsg::ConfirmArchiveSession {
                session_id,
                unarchive,
            } => {
                let (title, msg) = if unarchive {
                    ("Unarchive Session", "Unarchive this session?")
                } else {
                    ("Archive Session", "Archive this session?")
                };
                self.dialog_state.pending_archive_session = Some((session_id.clone(), unarchive));
                self.push_dialog(
                    Dialog::Confirm,
                    Box::new(ConfirmDialog::new(title.to_string(), msg.to_string())),
                );
            }
            TuiMsg::ConfirmBulkDelete { count, session_ids } => {
                let msg = format!("Delete {} selected sessions? This cannot be undone.", count);
                self.dialog_state.pending_bulk_delete = Some(count);
                self.dialog_state.pending_bulk_delete_ids = Some(session_ids);
                self.push_dialog(
                    Dialog::Confirm,
                    Box::new(ConfirmDialog::new("Delete Sessions".to_string(), msg)),
                );
            }
            TuiMsg::ConfirmBulkArchive {
                count,
                unarchive,
                session_ids,
            } => {
                let (title, msg) = if unarchive {
                    (
                        "Unarchive Sessions",
                        format!("Unarchive {} selected sessions?", count),
                    )
                } else {
                    (
                        "Archive Sessions",
                        format!("Archive {} selected sessions?", count),
                    )
                };
                self.dialog_state.pending_bulk_archive = Some((count, unarchive));
                self.dialog_state.pending_bulk_archive_ids = Some(session_ids);
                self.push_dialog(
                    Dialog::Confirm,
                    Box::new(ConfirmDialog::new(title.to_string(), msg)),
                );
            }
            TuiMsg::SelectTheme { theme_name } => {
                if let Some(theme) = self.theme_registry.get_tui(&theme_name) {
                    self.ui_state.theme = Arc::new(theme);
                    self.persist_theme_selection(&theme_name);
                    self.messages_state
                        .toasts
                        .info(&format!("Theme: {}", theme_name));
                } else {
                    self.messages_state
                        .toasts
                        .error(&format!("Unknown theme: {}", theme_name));
                }
                self.dialog_state.theme_picker = None;
                self.close_dialog();
            }
            TuiMsg::ThemePreviewChanged { theme_id } => {
                // Live preview only. Don't persist; don't change the
                // picker's `original_id` (already captured on the first
                // navigation). Don't close anything.
                if let Some(theme) = self.theme_registry.get_tui(&theme_id) {
                    self.ui_state.theme = Arc::new(theme);
                }
            }
            TuiMsg::ThemeCommit { theme_id } => {
                if let Some(theme) = self.theme_registry.get_tui(&theme_id) {
                    self.ui_state.theme = Arc::new(theme);
                    self.persist_theme_selection(&theme_id);
                    self.messages_state
                        .toasts
                        .info(&format!("Theme: {}", theme_id));
                } else {
                    self.messages_state
                        .toasts
                        .error(&format!("Unknown theme: {}", theme_id));
                }
                self.dialog_state.theme_picker = None;
                self.close_dialog();
            }
            TuiMsg::ThemeRevert => {
                // Revert the live theme to the one that was active when
                // the picker opened. Falls back to the default id if the
                // picker's `original_id` is somehow missing.
                let target = self
                    .dialog_state
                    .theme_picker
                    .as_ref()
                    .and_then(|p| p.preview_original_id())
                    .unwrap_or_else(|| crate::theme::registry::DEFAULT_THEME_ID.to_string());
                if let Some(theme) = self.theme_registry.get_tui(&target) {
                    self.ui_state.theme = Arc::new(theme);
                }
                self.dialog_state.theme_picker = None;
                self.close_dialog();
            }
            TuiMsg::SelectTreeSession { session_id: _ } => {
                self.close_dialog();
            }
            TuiMsg::ForkTreeSession { session_id } => {
                self.close_dialog();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(tx, TuiCommand::ForkSession { session_id });
                }
            }
            TuiMsg::ForkSession { session_id } => {
                self.close_dialog();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(tx, TuiCommand::ForkSession { session_id });
                }
            }
            TuiMsg::SubmitImportPreview => {
                self.handle_import_send();
            }
            TuiMsg::ConfirmImport => {
                self.handle_import_send();
            }
            TuiMsg::SubmitSelectionUpdate {
                session_id,
                connection_id,
                connection_revision,
                model_id,
                catalog_revision,
            } => {
                crate::tui::commands::session_selection::start_selection_update(
                    self,
                    session_id,
                    connection_id,
                    model_id,
                    connection_revision,
                    catalog_revision,
                );
            }
            TuiMsg::OpenConnectionRotation {
                connection_id,
                expected_revision,
            } => {
                self.open_connection_rotation_dialog(connection_id, expected_revision);
            }
            TuiMsg::ConnectionLifecycle {
                action,
                connection_id,
                expected_revision,
            } => {
                if matches!(
                    action,
                    ConnectionLifecycleAction::Delete | ConnectionLifecycleAction::Purge
                ) {
                    let (title, message) = match action {
                        ConnectionLifecycleAction::Delete => (
                            "Delete Provider Connection",
                            "Tombstone this connection? It will stop being selectable.",
                        ),
                        ConnectionLifecycleAction::Purge => (
                            "Purge Provider Connection",
                            "Permanently purge this tombstoned connection and its history?",
                        ),
                        other => {
                            tracing::warn!(
                                ?other,
                                "unexpected ConnectionLifecycleAction in match arm"
                            );
                            return;
                        }
                    };
                    self.dialog_state.pending_connection_lifecycle =
                        Some((action, connection_id, expected_revision));
                    self.push_dialog(
                        Dialog::Confirm,
                        Box::new(ConfirmDialog::new(title.to_string(), message.to_string())),
                    );
                } else {
                    let _ = self.enqueue_tui_command(TuiCommand::ConnectionLifecycle {
                        action,
                        connection_id,
                        expected_revision,
                    });
                }
            }
            TuiMsg::SubmitPermission { choice_index } => {
                let choice = match choice_index {
                    0 => crate::permission::PermissionChoice::AllowOnce,
                    1 => crate::permission::PermissionChoice::AlwaysAllow,
                    2 => crate::permission::PermissionChoice::DenyOnce,
                    3 => crate::permission::PermissionChoice::AlwaysDeny,
                    _ => return,
                };
                if let Some(ref perm_id) = self.dialog_state.permission_perm_id {
                    let perm_id = perm_id.clone();
                    if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
                        let choice = match choice {
                            crate::permission::PermissionChoice::AllowOnce => "allow",
                            crate::permission::PermissionChoice::AlwaysAllow => "always_allow",
                            crate::permission::PermissionChoice::DenyOnce => "deny",
                            crate::permission::PermissionChoice::AlwaysDeny => "always_deny",
                        };
                        self.send_remote_message(RemoteTuiMessage::PermissionResponse {
                            id: perm_id,
                            choice: choice.to_string(),
                        });
                    } else {
                        self.send_local_permission_response(perm_id, choice);
                    }
                }
                self.dialog_state.permission_dialog = None;
                self.dialog_state.permission_perm_id = None;
                self.close_dialog();
            }
            TuiMsg::SubmitQuestionAnswers { answers_json } => {
                if let Some(session_id) = self.dialog_state.question_session_id.take() {
                    let answers = answers_json.clone();
                    if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
                        let answers = serde_json::from_str::<serde_json::Value>(&answers)
                            .unwrap_or(serde_json::Value::String(answers));
                        self.send_remote_message(RemoteTuiMessage::QuestionResponse {
                            id: session_id,
                            answers,
                        });
                    } else {
                        self.send_local_question_response(session_id, answers);
                    }
                }
                self.dialog_state.question_dialog = None;
                self.dialog_state.question_session_id = None;
                self.close_dialog();
            }
            TuiMsg::SelectTemplate { key, template } => {
                self.apply_template(key, *template);
                self.dialog_state.template_dialog = None;
                self.close_dialog();
            }
            TuiMsg::GotoMessage { index } => {
                self.messages_state.messages.select_index(index);
                self.dialog_state.goto_dialog = None;
                self.close_dialog();
            }
            TuiMsg::CopyShareUrl => {
                if let Some(ref mut share) = self.dialog_state.share_dialog {
                    if share.copy_url() {
                        self.messages_state.toasts.info("URL copied to clipboard!");
                    } else {
                        self.messages_state.toasts.error("Failed to copy URL");
                    }
                }
                self.dialog_state.share_dialog = None;
                self.close_dialog();
            }
            TuiMsg::ToggleSidebar => self.toggle_sidebar(),
            TuiMsg::ToggleFullscreen => self.toggle_fullscreen(),
            TuiMsg::ToggleReasoning => self.toggle_reasoning(),
            TuiMsg::ToggleTts => self.toggle_tts(),
            TuiMsg::CycleModelForward => self.cycle_model_forward(),
            TuiMsg::CycleModelBackward => self.cycle_model_backward(),
            TuiMsg::ClearSession => self.clear_session(),
            TuiMsg::NewSession => self.new_session(),
            TuiMsg::CloseSession => self.close_session(),
            TuiMsg::CharInput(c) => self.on_char(c),
            TuiMsg::Backspace => self.prompt_state.prompt.backspace(),
            TuiMsg::Delete => self.prompt_state.prompt.delete(),
            TuiMsg::CursorLeft => self.prompt_state.prompt.cursor_left(),
            TuiMsg::CursorRight => self.prompt_state.prompt.cursor_right(),
            TuiMsg::CursorHome => self.prompt_state.prompt.cursor_home(),
            TuiMsg::CursorEnd => self.prompt_state.prompt.cursor_end(),
            TuiMsg::PageUp => self.messages_state.messages.scroll_page_up(),
            TuiMsg::PageDown => self.messages_state.messages.scroll_page_down(),
            TuiMsg::Search => {
                if self.messages_state.messages.is_searching() {
                    self.messages_state.messages.clear_search();
                } else {
                    self.ui_state.command_mode = true;
                    self.prompt_state.prompt.insert_char('/');
                    self.prompt_state.prompt.set_cursor(1);
                    self.dialog_state.command_palette.set_query("/search ");
                    self.messages_state.messages.search_visible = true;
                }
            }
            TuiMsg::SearchNext => self.messages_state.messages.search_next(),
            TuiMsg::SearchPrev => self.messages_state.messages.search_prev(),
            TuiMsg::ClearSearch => self.messages_state.messages.clear_search(),
            TuiMsg::FocusPrompt => self.prompt_state.prompt.focus(),
            TuiMsg::StashPrompt => self.stash_prompt(),
            TuiMsg::RestorePrompt => self.restore_prompt(),
            TuiMsg::CopyMessage => self.copy_message(),
            TuiMsg::Quit => self.quit(),
            TuiMsg::ExternalEditor => self.open_external_editor(),
            TuiMsg::OpenProjectPicker => self.open_project_picker(),
            TuiMsg::NextProjectTab => {
                if let Some(tx) = self.tui_cmd_tx.clone() {
                    let _ = send_tui(&tx, TuiCommand::NextProjectTab);
                }
            }
            TuiMsg::PreviousProjectTab => {
                if let Some(tx) = self.tui_cmd_tx.clone() {
                    let _ = send_tui(&tx, TuiCommand::PreviousProjectTab);
                }
            }
            TuiMsg::CloseProjectTab => {
                if let Some(tx) = self.tui_cmd_tx.clone() {
                    let _ = send_tui(&tx, TuiCommand::CloseProjectTab);
                }
            }
            TuiMsg::SelectProjectTabByIndex { index } => {
                self.select_project_tab_by_visible_index(index);
            }
            TuiMsg::UndoDelete => {
                if let Some(session_id) = self.undo_session_id.take() {
                    if let Some(ref tx) = self.tui_cmd_tx {
                        let _ = send_tui(tx, TuiCommand::UndoDelete { session_id });
                    }
                    self.undo_until = None;
                }
            }
            TuiMsg::ReviewOpenDiff { path } => {
                self.handle_diff_command(Some(&path));
            }
            TuiMsg::ResearchOpenRun { run_id } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(tx, TuiCommand::ResearchLoadRun { run_id });
                }
            }
            TuiMsg::ResearchRefreshRuns => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(tx, TuiCommand::ResearchListRuns);
                }
            }
            TuiMsg::ResearchLoadSection { run_id, section } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(tx, TuiCommand::ResearchLoadSection { run_id, section });
                }
            }
            TuiMsg::OpenSourcePreview {
                path,
                line,
                origin_label,
            } => {
                use crate::tui::components::dialogs::source_preview::SourcePreviewDialog;
                let dialog = match origin_label {
                    Some(label) => SourcePreviewDialog::with_origin(
                        Arc::clone(&self.ui_state.theme),
                        path,
                        line,
                        label,
                    ),
                    None => SourcePreviewDialog::new(Arc::clone(&self.ui_state.theme), path, line),
                };
                self.push_dialog(Dialog::SourcePreview, Box::new(dialog));
            }
            TuiMsg::OpenRunDetail { run_id } => {
                use crate::tui::components::dialogs::run_detail::RunDetailDialog;
                use codegg_core::run_store::RunDetailView;
                if let Some(ref run_store) = self.run_store {
                    let run_id_val = codegg_core::run_store::RunId(run_id.clone());
                    let run_store = Arc::clone(run_store);
                    let theme = Arc::clone(&self.ui_state.theme);
                    let tx = self.tui_cmd_tx.clone();
                    tokio::spawn(async move {
                        match run_store.get_run(&run_id_val).await {
                            Ok(Some(manifest)) => {
                                let detail = RunDetailView::from_manifest(&manifest);
                                let dialog = RunDetailDialog::new(detail, Arc::clone(&theme));
                                if let Some(ref tx) = tx {
                                    let _ =
                                        send_tui(tx, TuiCommand::OpenRunDetailLoaded { dialog });
                                }
                            }
                            Ok(None) => {
                                if let Some(ref tx) = tx {
                                    let _ = send_tui(
                                        tx,
                                        TuiCommand::OpenRunDetailError {
                                            error: format!("Run not found: {}", run_id),
                                        },
                                    );
                                }
                            }
                            Err(e) => {
                                if let Some(ref tx) = tx {
                                    let _ = send_tui(
                                        tx,
                                        TuiCommand::OpenRunDetailError {
                                            error: format!("Failed to load run: {}", e),
                                        },
                                    );
                                }
                            }
                        }
                    });
                } else {
                    self.messages_state.toasts.error("Run store not available");
                }
            }
            TuiMsg::SecurityReviewJump { path, line } => {
                // Read-only: copy the file path to the clipboard and
                // surface a toast. The file is never opened or mutated.
                let mut text = path.clone();
                if let Some(l) = line {
                    text.push_str(&format!(":{l}"));
                }
                match crate::util::clipboard::copy_to_clipboard(&text) {
                    Ok(()) => self
                        .messages_state
                        .toasts
                        .info(&format!("Copied {text} to clipboard (file not opened)")),
                    Err(_) => self
                        .messages_state
                        .toasts
                        .info(&format!("Jump: {text} (clipboard unavailable)")),
                }
            }
            TuiMsg::ShellInclude { id, mode } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = send_tui(
                        tx,
                        TuiCommand::ShellInclude {
                            id: id.parse().unwrap_or(0),
                            mode,
                            question: None,
                        },
                    );
                }
            }
            TuiMsg::ShellAsk { id } => {
                self.prompt_state.prompt.clear();
                self.prompt_state
                    .prompt
                    .set_text(format!("/shell-ask {}", id));
                self.ui_state.command_mode = true;
            }
            TuiMsg::ShellRerun { id } => {
                self.enqueue_tui_command(TuiCommand::ShellRerun {
                    id: id.parse().unwrap_or(0),
                });
            }
            TuiMsg::ShellKill { id } => {
                self.enqueue_tui_command(TuiCommand::ShellKill {
                    id: id.parse().unwrap_or(0),
                });
            }
            TuiMsg::RunRerun { run_id } => {
                crate::tui::commands::run_rerun::start_run_rerun(self, run_id);
            }
            TuiMsg::RunPromote { run_id } => {
                self.messages_state
                    .toasts
                    .info(&format!("Run {} promoted to context", run_id));
            }
            TuiMsg::RunCopyId { run_id } => {
                #[cfg(feature = "arboard")]
                {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(run_id.clone());
                        self.messages_state
                            .toasts
                            .info("Run ID copied to clipboard");
                    } else {
                        self.messages_state
                            .toasts
                            .info(&format!("Run ID: {}", run_id));
                    }
                }
                #[cfg(not(feature = "arboard"))]
                {
                    self.messages_state
                        .toasts
                        .info(&format!("Run ID: {}", run_id));
                }
            }
            _ => {}
        }
    }
}
