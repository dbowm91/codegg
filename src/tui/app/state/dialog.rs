//! Dialog state module.
//!
//! Contains non-visual dialog domain state.
//!
//! Live dialog components are owned exclusively by `FocusManager`. This
//! state holds request metadata, pending authorization, and the small amount
//! of presentation-independent state needed by dialog workflows.

pub struct DialogState {
    // Transitional seeds for stateful dialogs whose async reducers are being
    // moved to FocusManager-owned instances. They are not render/input
    // authorities; all mounted components live in FocusManager.
    pub model_dialog: crate::tui::components::dialogs::model::ModelDialog,
    pub agent_dialog: crate::tui::components::dialogs::agent::AgentDialog,
    pub session_dialog: crate::tui::components::dialogs::session::SessionDialog,
    pub tree_dialog: crate::tui::components::dialogs::tree::TreeDialog,
    pub theme_picker: Option<crate::tui::components::dialogs::theme::ThemePickerDialog>,
    pub question_dialog: Option<crate::tui::components::dialogs::question::QuestionDialog>,
    /// Command-mode palette state is a prompt completion surface, not a
    /// modal component and is not placed on the focus stack.
    pub command_palette: crate::tui::components::dialogs::command::CommandPalette,
    pub question_session_id: Option<String>,
    pub permission_perm_id: Option<String>,
    pub permission_dialog: Option<crate::tui::components::dialogs::permission::PermissionDialog>,
    pub keybind_dialog: Option<crate::tui::components::dialogs::keybind::KeybindDialog>,
    pub mcp_dialog: Option<crate::tui::components::dialogs::mcp::McpDialog>,
    pub share_dialog: Option<crate::tui::components::dialogs::share::ShareDialog>,
    pub import_dialog: Option<crate::tui::components::dialogs::import::ImportDialog>,
    pub template_dialog: Option<crate::tui::components::dialogs::template::TemplateDialog>,
    pub connect_dialog: Option<crate::tui::components::dialogs::connect::ConnectDialog>,
    pub connection_selection_dialog:
        Option<crate::tui::components::dialogs::connection_selection::ConnectionSelectionDialog>,
    pub goto_dialog: Option<crate::tui::components::dialogs::goto::GotoDialog>,
    pub plan_dialog: Option<crate::tui::components::dialogs::plan::PlanDialog>,
    pub diff_dialog: Option<crate::tui::components::dialogs::diff::DiffDialog>,
    pub review_dialog: Option<crate::tui::components::dialogs::review::ReviewDialog>,
    pub security_review_dialog:
        Option<crate::tui::components::dialogs::security_review::SecurityReviewDialog>,
    pub research_browser: Option<crate::tui::components::dialogs::research::ResearchBrowserDialog>,
    pub help_dialog: Option<crate::tui::components::dialogs::help::HelpDialog>,
    /// Currently viewed interactive terminal handle (for terminal dialog
    /// action shortcuts and key routing).
    pub terminal_detail_handle: Option<String>,
    pub pending_delete_session: Option<String>,
    pub pending_archive_session: Option<(String, bool)>,
    pub pending_bulk_delete: Option<usize>,
    pub pending_bulk_delete_ids: Option<Vec<String>>,
    pub pending_bulk_archive: Option<(usize, bool)>,
    pub pending_bulk_archive_ids: Option<Vec<String>>,
    pub pending_shell_command: Option<(String, bool, std::path::PathBuf)>,
    pub pending_connection_lifecycle:
        Option<(crate::tui::app::ConnectionLifecycleAction, String, u64)>,
    /// Currently viewed shell detail command ID (for shell detail dialog action shortcuts).
    pub shell_detail_id: Option<u64>,
    /// Async request state for import preview/confirm operations.
    pub import_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for research browser operations (list, load run, load section).
    pub research_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for session reload operations.
    pub session_reload_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for task list operations.
    pub task_list_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for task delete operations.
    pub task_delete_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for worktree list operations.
    pub worktree_list_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for template creation operations.
    pub template_create_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for session mutation requests. Stale completions
    /// with a mismatched id are silently ignored.
    pub session_mutation_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for session message loading.
    pub session_messages_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for test run operations. Stale completions
    /// with a mismatched id are silently ignored.
    pub test_run_request: crate::tui::app::state::AsyncUiRequestState,
    /// Async request state for interactive terminal operations
    /// (create/list/attach/resume/input/resize/detach/terminate/remove).
    /// Stale completions with a mismatched id are silently ignored.
    pub terminal_request: crate::tui::app::state::AsyncUiRequestState,
    /// Project picker dialog state (Milestone 2).
    pub project_picker: Option<crate::tui::app::state::ProjectPickerState>,
}
