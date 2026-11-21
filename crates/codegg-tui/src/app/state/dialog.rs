//! Dialog state module.
//!
//! Contains non-visual dialog domain state.

pub struct DialogState {
    pub job_detail_request: crate::app::state::async_request::AsyncUiRequestState,
    pub project_init_draft: Option<codegg::protocol::core::ProjectInitDraftDto>,
    // Transitional seeds for stateful dialogs whose async reducers are being
    // moved to FocusManager-owned instances. They are not render/input
    // authorities; all mounted components live in FocusManager.
    pub model_dialog: crate::components::dialogs::model::ModelDialog,
    pub agent_dialog: crate::components::dialogs::agent::AgentDialog,
    pub session_dialog: crate::components::dialogs::session::SessionDialog,
    pub tree_dialog: crate::components::dialogs::tree::TreeDialog,
    pub theme_picker: Option<crate::components::dialogs::theme::ThemePickerDialog>,
    pub question_dialog: Option<crate::components::dialogs::question::QuestionDialog>,
    /// Command-mode palette state is a prompt completion surface, not a
    /// modal component and is not placed on the focus stack.
    pub command_palette: crate::components::dialogs::command::CommandPalette,
    pub question_session_id: Option<String>,
    pub permission_perm_id: Option<String>,
    pub permission_dialog: Option<crate::components::dialogs::permission::PermissionDialog>,
    /// Approvals that arrived while another one was on screen.
    ///
    /// A batch can ask for several approvals at once. `show_permission_dialog`
    /// used to assign straight into `permission_dialog`, so each new request
    /// silently destroyed the one the user was looking at: its `perm_id` went
    /// with it, leaving a daemon-side waiter parked on a human answer that no
    /// longer had any UI pointing at it. That is the "the box disappeared and
    /// I can never approve it" symptom — the tool eventually timed out denied
    /// for a reason the user never saw.
    pub permission_queue: std::collections::VecDeque<PendingPermission>,
    pub keybind_dialog: Option<crate::components::dialogs::keybind::KeybindDialog>,
    pub mcp_dialog: Option<crate::components::dialogs::mcp::McpDialog>,
    pub share_dialog: Option<crate::components::dialogs::share::ShareDialog>,
    pub import_dialog: Option<crate::components::dialogs::import::ImportDialog>,
    pub template_dialog: Option<crate::components::dialogs::template::TemplateDialog>,
    pub connect_dialog: Option<crate::components::dialogs::connect::ConnectDialog>,
    pub connection_selection_dialog:
        Option<crate::components::dialogs::connection_selection::ConnectionSelectionDialog>,
    pub goto_dialog: Option<crate::components::dialogs::goto::GotoDialog>,
    /// `/approval` / `/sandbox` fast-select seed. Mounted onto the focus
    /// stack by `open_policy_selector`; see `policy_selector.rs`.
    pub policy_selector: Option<crate::components::dialogs::policy_selector::PolicySelectorDialog>,
    pub plan_dialog: Option<crate::components::dialogs::plan::PlanDialog>,
    pub diff_dialog: Option<crate::components::dialogs::diff::DiffDialog>,
    pub review_dialog: Option<crate::components::dialogs::review::ReviewDialog>,
    pub security_review_dialog:
        Option<crate::components::dialogs::security_review::SecurityReviewDialog>,
    pub research_browser: Option<crate::components::dialogs::research::ResearchBrowserDialog>,
    pub help_dialog: Option<crate::components::dialogs::help::HelpDialog>,
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
    pub pending_connection_lifecycle: Option<(crate::app::ConnectionLifecycleAction, String, u64)>,
    /// M007: confirmed-but-not-yet-applied approval/sandbox change
    /// awaiting explicit confirmation dialog(s). Bound to the preference
    /// revision the user saw (CAS); cancelling clears it and changes
    /// nothing.
    pub pending_policy_confirm: Option<crate::commands::policy::PendingPolicyConfirm>,
    /// Currently viewed shell detail command ID (for shell detail dialog action shortcuts).
    pub shell_detail_id: Option<u64>,
    /// Async request state for import preview/confirm operations.
    pub import_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for research browser operations (list, load run, load section).
    pub research_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for session reload operations.
    pub session_reload_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for single-session metadata refresh
    /// (`CoreEvent::SessionUpdated`, e.g. daemon-side auto-title).
    pub session_refresh_request: crate::app::state::AsyncUiRequestState,
    /// Session activation read. The selected session is rebound only after
    /// the daemon returns its current canonical workspace binding.
    pub session_activation_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for task list operations.
    pub task_list_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for task delete operations.
    pub task_delete_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: scheduling-sheet prefetch (lanes,
    /// summary, capabilities, Task-model preference) before the sheet
    /// opens. Stale completions are dropped at apply time.
    pub task_sheet_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: `WorkOrderCreate` continuation. A late
    /// success may have committed daemon state; it is never undone —
    /// the projection refreshes when next foregrounded.
    pub work_order_create_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: Task view list refresh.
    pub work_order_list_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: CAS lane reorder.
    pub work_order_reorder_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: cancel/resume/update mutations.
    pub work_order_mutation_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: lazy single-row occurrence detail.
    pub work_order_occurrence_request: crate::app::state::AsyncUiRequestState,
    /// Project Work Orders M003: Task-model preference read/persist.
    pub task_model_pref_request: crate::app::state::AsyncUiRequestState,
    /// C001: one-time trigger bearer display (transient, never persisted
    /// or logged; cleared on close/switch/reconnect/authority loss).
    pub trigger_secret: Option<crate::app::state::OneTimeTriggerSecret>,
    /// M003: one-time device-token plaintext display (transient, never
    /// persisted or logged; cleared on close/switch/reconnect/authority
    /// loss). Token metadata stays listable; the credential is never
    /// re-readable.
    pub team_token_secret: Option<crate::app::state::OneTimeDeviceToken>,
    /// M003: `TeamMembership*`/`TeamPrincipal*`/`TeamToken*`/chat-policy
    /// continuations for the `/team` administration surface. Stale
    /// completions (newer request, tab switch, reconnect) are dropped at
    /// apply time.
    pub team_request: crate::app::state::AsyncUiRequestState,
    /// M004: `SessionControl*` continuations for the `/control` shared-
    /// session surface. Stale completions are dropped at apply time.
    pub control_request: crate::app::state::AsyncUiRequestState,
    /// M004: last-known controller lease for the active session
    /// `(session_id, controller)`. Display cache only; authority stays
    /// daemon-side. `None` controller means the last fetch observed no
    /// active lease (idle or released). Refresh with `/control`.
    pub control_last: Option<(String, Option<codegg::protocol::core::SessionControllerDto>)>,
    /// C001: `WorkOrderTriggerCreate` continuation (WorkOrder+trigger
    /// chain, retry, and rotation creates).
    pub trigger_create_request: crate::app::state::AsyncUiRequestState,
    /// C001: trigger metadata/revoke continuations (list/get/revoke).
    pub trigger_manage_request: crate::app::state::AsyncUiRequestState,
    /// C001: last-known trigger metadata per WorkOrder (M005 DTOs only,
    /// never secrets). Drives Task-view status/retry/revoke/rotate.
    pub trigger_metadata: std::collections::HashMap<
        String,
        Vec<codegg::protocol::work_order::TaskTriggerMetadataDto>,
    >,
    /// C001: setup-incomplete diagnostics per WorkOrder (retry path
    /// without deleting the WorkOrder).
    pub trigger_setup_error: std::collections::HashMap<String, String>,
    /// C001: user-requested trigger setups awaiting metadata
    /// reconciliation (ambiguous-timeout safe: list first, create only
    /// when no active trigger exists).
    pub trigger_setup_pending: std::collections::HashSet<String>,
    /// Project Work Orders M003: open scheduling-sheet draft (`None`
    /// = sheet closed). The editable prompt text stays in the prompt
    /// widget until `WorkOrderCreate` succeeds; failure restores it
    /// exactly once.
    pub task_schedule_draft: Option<crate::app::state::TaskScheduleDraft>,
    /// Project Work Orders M003: cached project Task view projection.
    /// Daemon-owned truth; this is a bounded display cache only.
    pub task_view: crate::app::state::TaskViewState,
    /// Async request state for worktree list operations.
    pub worktree_list_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for template creation operations.
    pub template_create_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for user-initiated new-session creation (Ctrl+N,
    /// `/new`). Stale completions with a mismatched id are ignored.
    pub new_session_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for session mutation requests. Stale completions
    /// with a mismatched id are silently ignored.
    pub session_mutation_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for session message loading.
    pub session_messages_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for test run operations. Stale completions
    /// with a mismatched id are silently ignored.
    pub test_run_request: crate::app::state::AsyncUiRequestState,
    /// Async request state for interactive terminal operations
    /// (create/list/attach/resume/input/resize/detach/terminate/remove).
    /// Stale completions with a mismatched id are silently ignored.
    pub terminal_request: crate::app::state::AsyncUiRequestState,
    /// Project picker dialog state (Milestone 2).
    pub project_picker: Option<crate::app::state::ProjectPickerState>,
    /// Project Work Orders M004: cached global Workspace dashboard
    /// projection (`None` = dashboard closed). Daemon-owned truth;
    /// this is a bounded display cache only. Entering/leaving never
    /// cancels daemon work.
    pub workspace_dashboard: Option<crate::app::state::WorkspaceDashboardState>,
    /// M004: `/queue` delete continuation. Stale completions are dropped at
    /// apply time, and `cancel()` on close invalidates any delete still in
    /// flight so a late answer cannot mutate a projection nobody is looking at.
    pub queue_delete_request: crate::app::state::AsyncUiRequestState,
    /// M005. One generation per mutation kind rather than one shared slot: a
    /// delete racing an edit must not silently cancel the edit, and a single
    /// state would make whichever key was pressed last the only one that could
    /// ever land.
    pub queue_add_request: crate::app::state::AsyncUiRequestState,
    pub queue_edit_request: crate::app::state::AsyncUiRequestState,
    pub queue_reorder_request: crate::app::state::AsyncUiRequestState,
    /// M006: the in-flight send-now. Its own generation, so a slow steer
    /// cannot be completed by — or overwrite — a later one.
    pub queue_send_request: crate::app::state::AsyncUiRequestState,
}

/// An approval the user has not answered yet, held so a later request cannot
/// evict it.
#[derive(Debug, Clone)]
pub struct PendingPermission {
    pub perm_id: String,
    pub request: codegg::permission::PermissionRequest,
}
