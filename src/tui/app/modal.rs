//! Modal and focus lifecycle integration.
//!
//! Modal construction, project-picker opening, and info-surface promotion all
//! use the M007 FocusManager owner. The compatibility Dialog discriminator is
//! updated only as a projection of that live modal stack.

use super::{App, Dialog};
use crate::agent::Agent;
use crate::config::schema::SessionTemplate;
use crate::tui::components::dialogs::confirm::ConfirmDialog;
use crate::tui::components::dialogs::import::ImportDialog;
use crate::tui::components::dialogs::keybind::KeybindDialog;
use crate::tui::components::dialogs::theme::ThemePickerDialog;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

/// Upper bound on bytes read from the tail of the daemon log. The daemon
/// rotates that file at 10 MB; this read is one bounded `seek` + `take`
/// regardless of how large the file grows, so no unbounded read is
/// reachable from the TUI event loop.
const LOG_TAIL_MAX_BYTES: u64 = 256 * 1024;
/// Upper bound on log lines rendered in the `/logs` window.
const LOG_TAIL_MAX_LINES: usize = 400;
/// Upper bound on bytes kept per rendered log line, so one pathological
/// line cannot dominate the window.
const LOG_LINE_MAX_BYTES: usize = 512;

/// Read at most [`LOG_TAIL_MAX_BYTES`] from the end of `path` and return
/// the lines it contains, oldest first.
///
/// A read that starts mid-file can cut the first record in half, so that
/// fragment is dropped. Undecodable bytes are replaced rather than
/// rejecting the whole tail.
fn read_log_tail(path: &Path) -> std::io::Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(LOG_TAIL_MAX_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    file.take(len - start).read_to_end(&mut buf)?;

    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let body: &[&str] = if start > 0 {
        lines.get(1..).unwrap_or(&[])
    } else {
        &lines[..]
    };
    Ok(body
        .iter()
        .take(LOG_TAIL_MAX_LINES)
        .map(|line| crate::util::truncate_prefix(line, LOG_LINE_MAX_BYTES).to_string())
        .collect())
}

impl App {
    #[allow(dead_code)]
    fn active_dialog_type(&self) -> crate::tui::components::component::DialogType {
        self.focus_manager.active_dialog_type()
    }

    pub(crate) fn open_info_dialog(
        &mut self,
        info_type: crate::tui::components::dialogs::info::InfoType,
        lines: Vec<String>,
    ) {
        use crate::tui::components::dialogs::info::InfoDialog;
        let dialog = InfoDialog::new(Arc::clone(&self.ui_state.theme), info_type, lines);
        let dialog_type = dialog.dialog_type_for_info_type();
        if self.focus_manager.dialog_mut_any::<InfoDialog>().is_some() {
            let _ = self
                .focus_manager
                .dialog_mut_any::<InfoDialog>()
                .map(|live| {
                    live.set_info_type(dialog.info_type());
                    live.set_content(dialog.content_lines().to_vec());
                    live.set_theme(&self.ui_state.theme);
                });
            self.ui_state.dialog = Dialog::from(dialog_type);
        } else {
            self.push_dialog(Dialog::from(dialog_type), Box::new(dialog));
        }
    }

    /// Show short text as a toast, or open a scrollable info dialog
    /// when the content is multi-line. Use this for command outputs
    /// whose length is data-dependent (lists, reports, structured
    /// results) to keep the toast column readable.
    pub(crate) fn show_short_or_info(
        &mut self,
        info_type: crate::tui::components::dialogs::info::InfoType,
        lines: Vec<String>,
    ) {
        const MAX_TOAST_LINES: usize = 3;
        if lines.len() <= MAX_TOAST_LINES {
            let joined = lines.join("\n");
            self.messages_state.toasts.info(&joined);
        } else {
            self.open_info_dialog(info_type, lines);
        }
    }

    /// Open the `/logs` window: a bounded tail of the user-scoped daemon
    /// log plus every toast notification this session has shown.
    ///
    /// The TUI process has no in-memory tracing capture and writes no log
    /// of its own by default, so the daemon log file is the only durable
    /// source. When it is missing or unreadable the window still opens
    /// with the notification history and says plainly what is missing —
    /// it never substitutes a fabricated source.
    pub(crate) fn open_logs_window(&mut self) {
        use crate::tui::components::dialogs::info::InfoDialog;
        let lines = self.logs_window_lines();
        self.open_info_dialog(crate::tui::components::dialogs::info::InfoType::Logs, lines);
        // The window is append-only; land on its newest entry so the
        // most recent log line is visible without scrolling.
        self.focus_manager
            .with_component_mut::<InfoDialog, _>(|dialog| dialog.scroll_to_end());
    }

    /// Build the `/logs` body: log source header, bounded daemon log
    /// tail, then the retained notification history oldest-first.
    fn logs_window_lines(&self) -> Vec<String> {
        use crate::tui::components::toast::MAX_TOAST_HISTORY;

        let log_path = codegg_client::LocalDaemonPaths::resolve().log_path;
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("== Daemon log: {} ==", log_path.display()));
        match read_log_tail(&log_path) {
            Ok(tail) if tail.is_empty() => {
                lines.push("(log file is empty)".to_string());
            }
            Ok(tail) => {
                lines.push(format!(
                    "(last {} line{} of at most {} KiB)",
                    tail.len(),
                    if tail.len() == 1 { "" } else { "s" },
                    LOG_TAIL_MAX_BYTES / 1024
                ));
                lines.extend(tail);
            }
            Err(err) => {
                lines.push(format!("(daemon log unavailable: {err})"));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "== Notifications (newest last, at most {MAX_TOAST_HISTORY} retained) =="
        ));
        let history: Vec<_> = self.messages_state.toasts.history().collect();
        if history.is_empty() {
            lines.push("(no notifications yet this session)".to_string());
        }
        for record in history.into_iter().rev() {
            let age = record.created_at.elapsed().as_secs();
            lines.push(format!(
                "[{} · {age}s ago] {}",
                record.level.label(),
                crate::util::truncate_prefix(&record.message, LOG_LINE_MAX_BYTES)
            ));
        }
        lines
    }

    pub(crate) fn open_ui_node_dialog(&mut self, title: String, body: codegg_protocol::ui::UiNode) {
        use crate::tui::components::dialogs::ui_node::UiNodeDialog;
        if self.focus_manager.has_component::<UiNodeDialog>() {
            let _ = self
                .focus_manager
                .with_component_mut::<UiNodeDialog, _>(|dialog| {
                    dialog.update_content(body);
                    dialog.set_title(title);
                    dialog.set_theme(&self.ui_state.theme);
                });
        } else {
            let dialog = UiNodeDialog::new(
                "stats".into(),
                title,
                body,
                Arc::clone(&self.ui_state.theme),
            );
            self.push_dialog(Dialog::Stats, Box::new(dialog));
        }
    }

    /// Open the project picker dialog. Creates a fresh picker state
    /// scoped to the current catalog snapshot. Does not require the
    /// catalog to be loaded — the picker shows an explicit loading or
    /// unsupported state until the first refresh completes.
    pub fn open_project_picker(&mut self) {
        let transport_local = matches!(
            self.ui_state.mode,
            crate::tui::app::state::AppMode::Embedded
        );
        let catalog_generation = self.project_catalog.list_request.request_id();
        let picker =
            crate::tui::app::state::ProjectPickerState::new(transport_local, catalog_generation);
        self.dialog_state.project_picker = Some(picker);
        self.push_dialog(
            Dialog::ProjectPicker,
            Box::new(
                crate::tui::components::dialogs::project_picker::ProjectPickerDialog::new(
                    Arc::clone(&self.ui_state.theme),
                ),
            ),
        );

        // Trigger a fresh catalog refresh so the picker has current data.
        if self.core_client.is_some() {
            self.refresh_project_catalog();
        }
    }

    /// Switch the active project tab to the one at the given visible
    /// index in the bounded tab strip. Indices wrap. No-op when the
    /// visible tab count differs from the underlying tab count (only
    /// possible during rapid switch transitions).
    pub fn select_project_tab_by_visible_index(&mut self, visible_index: usize) {
        let visible_count = self.project_tabs.ordered().len();
        if visible_count == 0 || visible_index >= visible_count {
            return;
        }
        let ordered = self.project_tabs.ordered();
        let Some(target) = ordered.get(visible_index) else {
            return;
        };
        let target_id = target.tab_id.clone();
        // Use NextProjectTab/PreviousProjectTab for cycling, but
        // also support direct jump via open_or_focus_project path.
        // Simplest: invoke the existing switch helper directly.
        crate::tui::commands::project_picker::switch_active_tab(self, &target_id);
    }

    pub fn open_dialog(&mut self, dialog: Dialog) {
        match dialog {
            Dialog::Session => {
                self.load_sessions_dialog();
                self.focus_manager
                    .push(Box::new(self.dialog_state.session_dialog.clone()));
            }
            Dialog::Model => {
                self.dialog_state
                    .model_dialog
                    .set_current(&self.agent_state.current_model);
                self.dialog_state.model_dialog.initialize_selection();
                self.focus_manager
                    .push(Box::new(self.dialog_state.model_dialog.clone()));
            }
            Dialog::Agent => {
                let visible: Vec<&Agent> = self
                    .agent_state
                    .agents
                    .iter()
                    .filter(|a| !a.hidden)
                    .collect();
                self.dialog_state.agent_dialog.set_agents(visible);
                self.dialog_state.agent_dialog.initialize_selection(
                    &self.agent_state.agents[self.agent_state.current_agent].name,
                );
                self.focus_manager
                    .push(Box::new(self.dialog_state.agent_dialog.clone()));
            }
            Dialog::Help => {
                // Always recreate to reflect current input mode
                let help_dialog = crate::tui::components::dialogs::help::HelpDialog::new_with_mode(
                    Arc::clone(&self.ui_state.theme),
                    self.ui_state.vim_mode,
                    self.ui_state.input_mode,
                );
                self.dialog_state.help_dialog = Some(help_dialog);
                if let Some(ref mut help_dialog) = self.dialog_state.help_dialog {
                    self.focus_manager.push(Box::new(help_dialog.clone()));
                }
            }
            Dialog::Context | Dialog::Cost | Dialog::Usage => {
                let info_type = match dialog {
                    Dialog::Context => crate::tui::components::dialogs::info::InfoType::Context,
                    Dialog::Cost => crate::tui::components::dialogs::info::InfoType::Cost,
                    Dialog::Usage => crate::tui::components::dialogs::info::InfoType::Usage,
                    _ => crate::tui::components::dialogs::info::InfoType::Context,
                };
                let lines = self.get_info_dialog_lines();
                self.open_info_dialog(info_type, lines);
            }
            Dialog::Tree => {
                self.focus_manager
                    .push(Box::new(self.dialog_state.tree_dialog.clone()));
            }
            Dialog::Theme => {
                if self.dialog_state.theme_picker.is_none() {
                    let picker = ThemePickerDialog::with_themes(
                        Arc::clone(&self.ui_state.theme),
                        self.theme_registry.all_tui_themes(),
                    );
                    self.dialog_state.theme_picker = Some(picker);
                }
                if let Some(ref mut picker) = self.dialog_state.theme_picker {
                    picker.set_theme(&self.ui_state.theme);
                    picker.initialize_selection();
                    self.focus_manager.push(Box::new(picker.clone()));
                }
            }
            Dialog::Question => {
                if let Some(ref mut qd) = self.dialog_state.question_dialog {
                    self.focus_manager.push(Box::new((*qd).clone()));
                }
            }
            Dialog::Permission => {
                if let Some(ref mut pd) = self.dialog_state.permission_dialog {
                    self.focus_manager.push(Box::new((*pd).clone()));
                }
            }
            Dialog::Mcp => {
                if self.dialog_state.mcp_dialog.is_none() {
                    self.dialog_state.mcp_dialog =
                        Some(crate::tui::components::dialogs::mcp::McpDialog::new(
                            Arc::clone(&self.ui_state.theme),
                        ));
                }
                if let Some(ref mut mcp_dialog) = self.dialog_state.mcp_dialog {
                    let servers: Vec<crate::tui::components::dialogs::mcp::McpServerInfo> = self
                        .session_state
                        .mcp_servers
                        .iter()
                        .map(
                            |(name, status)| crate::tui::components::dialogs::mcp::McpServerInfo {
                                name: name.clone(),
                                status: status.clone(),
                                status_error: None,
                                server_type: "unknown".to_string(),
                                tools: Vec::new(),
                                resources: Vec::new(),
                                has_oauth: false,
                            },
                        )
                        .collect();
                    mcp_dialog.set_servers(servers);
                    self.focus_manager.push(Box::new((*mcp_dialog).clone()));
                }
            }
            Dialog::Keybind => {
                if self.dialog_state.keybind_dialog.is_none() {
                    let mut dialog = KeybindDialog::new(Arc::clone(&self.ui_state.theme));
                    if let Some(keybinds) = &self.ui_state.keybinds {
                        dialog.set_bindings(keybinds.bindings.clone());
                    }
                    self.dialog_state.keybind_dialog = Some(dialog);
                }
                if let Some(ref kd) = self.dialog_state.keybind_dialog {
                    self.focus_manager.push(Box::new(kd.clone()));
                }
            }
            Dialog::Share => {
                if let Some(ref dialog) = self.dialog_state.share_dialog {
                    self.focus_manager.push(Box::new(dialog.clone()));
                }
            }
            Dialog::ProjectPicker => {
                self.open_project_picker();
            }
            Dialog::Import => {
                if self.dialog_state.import_dialog.is_none() {
                    self.dialog_state.import_dialog =
                        Some(ImportDialog::new(Arc::clone(&self.ui_state.theme)));
                }
                if let Some(ref mut import) = self.dialog_state.import_dialog {
                    self.focus_manager.push(Box::new(import.clone()));
                }
            }
            Dialog::Template => {
                if self.dialog_state.template_dialog.is_none() {
                    self.dialog_state.template_dialog = Some(
                        crate::tui::components::dialogs::template::TemplateDialog::new(Arc::clone(
                            &self.ui_state.theme,
                        )),
                    );
                }
                if let Some(config_watcher) = self.config_watcher.as_ref() {
                    if let Ok(config) = config_watcher.reload_now() {
                        if let Some(templates) = config.templates.as_ref() {
                            let template_list: Vec<(String, SessionTemplate)> = templates
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            if let Some(ref mut dialog) = self.dialog_state.template_dialog {
                                dialog.set_templates(template_list);
                            }
                        }
                    }
                }
                if let Some(ref mut dialog) = self.dialog_state.template_dialog {
                    dialog.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(dialog.clone()));
                }
            }
            Dialog::Connect => {
                if self.dialog_state.connect_dialog.is_none() {
                    // This shouldn't happen - open_connect_dialog() should always be called first
                    // Call it to ensure proper setup
                    self.open_connect_dialog();
                    return;
                }
                if let Some(ref mut connect_dialog) = self.dialog_state.connect_dialog {
                    connect_dialog.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(connect_dialog.clone()));
                }
            }
            Dialog::Diff => {
                if let Some(ref mut diff_dialog) = self.dialog_state.diff_dialog {
                    self.focus_manager.push(Box::new(diff_dialog.clone()));
                }
            }
            Dialog::Goto => {
                if self.dialog_state.goto_dialog.is_none() {
                    self.dialog_state.goto_dialog =
                        Some(crate::tui::components::dialogs::goto::GotoDialog::new(
                            self.messages_state.messages.messages.len(),
                        ));
                }
                if let Some(ref mut goto_dialog) = self.dialog_state.goto_dialog {
                    goto_dialog.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(goto_dialog.clone()));
                }
            }
            Dialog::Plan => {
                if let Some(ref mut plan_dialog) = self.dialog_state.plan_dialog {
                    self.focus_manager.push(Box::new(plan_dialog.clone()));
                }
            }
            Dialog::Review => {
                let changed_files = self.session_state.changed_files.clone();
                let items: Vec<crate::tui::components::dialogs::review::ReviewItem> = changed_files
                    .iter()
                    .map(|f| crate::tui::components::dialogs::review::ReviewItem {
                        path: f.path.to_string_lossy().into_owned(),
                        kind: f.action.clone(),
                        additions: 0,
                        deletions: 0,
                    })
                    .collect();
                if self.dialog_state.review_dialog.is_none() {
                    self.dialog_state.review_dialog =
                        Some(crate::tui::components::dialogs::review::ReviewDialog::new(
                            Arc::clone(&self.ui_state.theme),
                        ));
                }
                if let Some(ref mut review_dialog) = self.dialog_state.review_dialog {
                    review_dialog.set_items(items);
                    review_dialog.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(review_dialog.clone()));
                }
            }
            Dialog::Confirm => {
                self.focus_manager.push(Box::new(ConfirmDialog::new(
                    "Confirm".to_string(),
                    "Are you sure?".to_string(),
                )));
            }
            Dialog::ResearchBrowser => {
                if self.dialog_state.research_browser.is_none() {
                    self.dialog_state.research_browser = Some(
                        crate::tui::components::dialogs::research::ResearchBrowserDialog::new(
                            Arc::clone(&self.ui_state.theme),
                        ),
                    );
                }
                if let Some(ref mut browser) = self.dialog_state.research_browser {
                    browser.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(browser.clone()));
                }
            }
            Dialog::SecurityReview => {
                if self.dialog_state.security_review_dialog.is_none() {
                    let mut dialog =
                        crate::tui::components::dialogs::security_review::SecurityReviewDialog::new(
                            Arc::clone(&self.ui_state.theme),
                        );
                    if let Some(ref receipt) = self.latest_security_review {
                        dialog.set_receipt(Some(receipt.clone()));
                    }
                    self.dialog_state.security_review_dialog = Some(dialog);
                } else if let Some(ref mut dialog) = self.dialog_state.security_review_dialog {
                    if let Some(ref receipt) = self.latest_security_review {
                        dialog.set_receipt(Some(receipt.clone()));
                    }
                }
                if let Some(ref mut dialog) = self.dialog_state.security_review_dialog {
                    dialog.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(dialog.clone()));
                }
            }
            Dialog::SourcePreview => {}
            Dialog::RunDetail => {}
            Dialog::TaskSchedule => {
                // Mounted directly by the sheet-prefetch completion with
                // a seeded snapshot; reopening here without a draft
                // would show a stale sheet, so this arm stays a no-op.
            }
            Dialog::TaskView => {
                // Mounted directly by `open_task_view`/`refresh_task_view`
                // with the live projection; see `work_orders.rs`.
            }
            Dialog::WorkspaceDashboard => {
                // Mounted directly by `open_workspace_dashboard` with a
                // fresh generation; see `workspace_dashboard.rs`.
            }
            Dialog::ConnectionSelection => {
                if let Some(ref dialog) = self.dialog_state.connection_selection_dialog {
                    self.focus_manager.push(Box::new(dialog.clone()));
                }
            }
            Dialog::Plugin => {
                if let Some(spec) = self.plugin_ui_state.get_dialog("active") {
                    let lines =
                        crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                            &spec.body,
                        );
                    let dialog = crate::tui::components::dialogs::plugin::PluginDialog::new(
                        spec.id.clone(),
                        spec.title.clone(),
                        lines,
                        Arc::clone(&self.ui_state.theme),
                    );
                    self.focus_manager.push(Box::new(dialog));
                }
            }
            _ => {}
        }
        self.ui_state.dialog = dialog;
    }
}
