//! Dialog state module.
//!
//! Contains all dialog instances. Dialogs are either always-instantiated
//! (for frequently used dialogs) or created on-demand (for rare dialogs).

/// Holds all dialog instances.
///
/// ## Dialog Patterns
///
/// - **Always instantiated**: Model, Agent, Session, Tree, CommandPalette
///   - Created once in `App::with_config()`
///   - Reset/reused when opened
///
/// - **On-demand**: Theme, Question, Permission, Keybind, Mcp, Share, Import
///   - Created when opened via `DialogState::some_dialog = Some(DialogType::new())`
///   - Set to `None` when closed
///
/// ## Showing a Dialog
///
/// ```rust,ignore
/// // Set the dialog type in ui_state
/// app.ui_state.dialog = Dialog::MyDialog;
///
/// // Initialize dialog state if on-demand
/// app.dialog_state.my_dialog = Some(MyDialog::new());
/// ```
pub struct DialogState {
    pub model_dialog: crate::tui::components::dialogs::model::ModelDialog,
    pub agent_dialog: crate::tui::components::dialogs::agent::AgentDialog,
    pub session_dialog: crate::tui::components::dialogs::session::SessionDialog,
    pub tree_dialog: crate::tui::components::dialogs::tree::TreeDialog,
    pub theme_picker: Option<crate::tui::components::dialogs::theme::ThemePickerDialog>,
    pub question_dialog: Option<crate::tui::components::dialogs::question::QuestionDialog>,
    pub question_session_id: Option<String>,
    pub command_palette: crate::tui::components::dialogs::command::CommandPalette,
    pub permission_dialog: Option<crate::tui::components::dialogs::permission::PermissionDialog>,
    pub permission_perm_id: Option<String>,
    pub keybind_dialog: Option<crate::tui::components::dialogs::keybind::KeybindDialog>,
    pub mcp_dialog: Option<crate::tui::components::dialogs::mcp::McpDialog>,
    pub share_dialog: Option<crate::tui::components::dialogs::share::ShareDialog>,
    pub import_dialog: Option<crate::tui::components::dialogs::import::ImportDialog>,
    pub template_dialog: Option<crate::tui::components::dialogs::template::TemplateDialog>,
    pub connect_dialog: Option<crate::tui::components::dialogs::connect::ConnectDialog>,
    pub goto_dialog: Option<crate::tui::components::dialogs::goto::GotoDialog>,
    pub plan_dialog: Option<crate::tui::components::dialogs::plan::PlanDialog>,
    pub diff_dialog: Option<crate::tui::components::dialogs::diff::DiffDialog>,
    pub review_dialog: Option<crate::tui::components::dialogs::review::ReviewDialog>,
    pub security_review_dialog:
        Option<crate::tui::components::dialogs::security_review::SecurityReviewDialog>,
    pub source_preview_dialog:
        Option<crate::tui::components::dialogs::source_preview::SourcePreviewDialog>,
    pub research_browser: Option<crate::tui::components::dialogs::research::ResearchBrowserDialog>,
    pub help_dialog: Option<crate::tui::components::dialogs::help::HelpDialog>,
    pub info_dialog: Option<crate::tui::components::dialogs::info::InfoDialog>,
    pub pending_delete_session: Option<String>,
    pub pending_archive_session: Option<(String, bool)>,
    pub pending_bulk_delete: Option<usize>,
    pub pending_bulk_delete_ids: Option<Vec<String>>,
    pub pending_bulk_archive: Option<(usize, bool)>,
    pub pending_bulk_archive_ids: Option<Vec<String>>,
    pub pending_shell_command: Option<(String, bool)>,
}
