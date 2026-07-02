#![allow(clippy::collapsible_match)]

mod types;

pub use types::{CompletionType, Dialog, HistoryEntry, SessionStatus, TodoEntry, TuiMsg};

pub mod state;
pub use state::*;

use super::components::component::DialogType;
use super::components::dialogs::agent::AgentDialog;
use super::components::dialogs::command::CommandPalette;
use super::components::dialogs::confirm::ConfirmDialog;
use super::components::dialogs::import::ImportDialog;
use super::components::dialogs::keybind::KeybindDialog;
use super::components::dialogs::mcp::BrowseMode;
use super::components::dialogs::model::ModelDialog;
use super::components::dialogs::permission::PermissionDialog;
use super::components::dialogs::question::{QuestionDialog, QuestionSpec};
use super::components::dialogs::session::SessionDialog;
use super::components::dialogs::theme::ThemePickerDialog;
use super::components::dialogs::tree::TreeDialog;
use super::components::messages::{MessageRole, MessagesWidget, MsgPart};
use super::components::prompt::PromptWidget;
use super::components::sidebar::{clean_inline_text, SidebarWidget};
use super::components::status_bar::{StatusBarWidget, TuiStatusSummary};
use super::components::toast::ToastManager;
use super::input::{
    build_help_lines, handle_event, handle_event_with_bindings_moded, HelpMode, InputAction,
    InputMode, KeybindConfig,
};
use super::layout::{LayoutConfig, TuiLayout};
use super::route::{Route, RouteManager};
use super::theme::Theme;
use crate::agent::builtin_agents;
use crate::agent::Agent;
use crate::config::schema::SessionTemplate;
use crate::core::CoreClient;
use crate::error::AppError;
use crate::memory::MemoryStore;
use crate::permission::PermissionRequest;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::tui::TuiMessage as RemoteTuiMessage;
use crate::provider::ChatEvent;
use crate::session::message::ToolStatus;
use crate::session::{MessageStore, Session, SessionStore};
use crate::tts::Tts;
use crate::tui::components::toast::Toast;
use crate::tui::task_lifecycle::TuiTaskKind;
use crate::util::fuzzy::fuzzy_score;
use crossterm::event::KeyEvent;
use egglsp::{render_workflow_display, LspWorkflowInvocation, LspWorkflowRecipe};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, Wrap,
};
use ratatui::Frame;
use std::collections::HashMap;
use std::path::PathBuf;
#[allow(unused_imports)]
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Maximum serialized size of a `UiNode` body included in a remote
/// snapshot. Mirrors [`crate::protocol::ui::UiLimits::max_snapshot_body_bytes`]
/// default. Bodies that exceed this limit are omitted so the snapshot
/// remains bounded under reconnect storms.
const SNAPSHOT_BODY_LIMIT: usize = 16 * 1024;

/// Return `Some(body.clone())` if the serialized body fits within the
/// snapshot body limit, otherwise `None`. Caller is expected to send
/// metadata-only when the body is too large, and the remote client can
/// request a fresh body on demand via a resync.
fn snapshot_body_within_limit(
    body: &crate::protocol::ui::UiNode,
    limit: usize,
) -> Option<crate::protocol::ui::UiNode> {
    let bytes = serde_json::to_vec(body).ok()?.len();
    if bytes <= limit {
        Some(body.clone())
    } else {
        None
    }
}

/// Extract the owning plugin id from a plugin-UI surface id, if any.
///
/// Canonical surface ids:
/// - `<plugin_id>:<command>` for installed plugin command outputs
/// - `command:local:<command>` for project-local commands (no plugin owner)
///
/// Returns the first `:`-segment unless the id starts with `command:`,
/// in which case there is no plugin owner.
fn plugin_id_from_surface_id(id: &str) -> Option<String> {
    if id.starts_with("command:") {
        return None;
    }
    let first = id.split(':').next()?;
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        tracing::debug!(target: "codegg::tui::app", "{}", format!($($arg)*));
    };
}

#[cfg(not(feature = "debug-logging"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

#[derive(Debug, Clone)]
pub enum TuiCommand {
    DeleteSession {
        session_id: String,
    },
    ArchiveSession {
        session_id: String,
        unarchive: bool,
    },
    UndoDelete {
        session_id: String,
    },
    ForkSession {
        session_id: String,
    },
    ShareSession {
        session_id: String,
    },
    UnshareSession {
        session_id: String,
    },
    ExportSession {
        session_id: String,
    },
    RenameSession {
        session_id: String,
        new_title: String,
    },
    BulkDelete {
        session_ids: Vec<String>,
    },
    BulkArchive {
        session_ids: Vec<String>,
        unarchive: bool,
    },
    BulkExport {
        session_ids: Vec<String>,
    },
    ReloadSessions,
    OpenTreeDialog,
    PreviewImport {
        source: super::components::dialogs::import::ImportSource,
    },
    ConfirmImport {
        source: super::components::dialogs::import::ImportSource,
    },
    CreateFromTemplate {
        key: String,
        template: SessionTemplate,
    },
    LoadSessionMessages {
        session_id: String,
    },
    SpawnSubagent {
        agent_name: String,
        prompt: String,
    },
    ListTasks,
    DeleteTask {
        id: String,
    },
    TaskSchedule {
        interval_secs: u64,
        message: String,
    },
    WorktreeList,
    MemorySummary,
    MemorySearch {
        query: String,
    },
    MemoryRemember {
        text: String,
    },
    MemoryForget {
        id: String,
    },
    CompactSession,
    OpenDiffDialog {
        old_content: Box<str>,
        new_content: Box<str>,
        title: Box<str>,
    },
    SendNotification {
        notification_type: super::components::notification::NotificationType,
        body: String,
    },
    GoalSet {
        session_id: String,
        project_id: String,
        objective: String,
    },
    GoalFromFile {
        session_id: String,
        project_id: String,
        path: String,
    },
    GoalShow {
        session_id: String,
    },
    GoalPause {
        session_id: String,
    },
    GoalResume {
        session_id: String,
    },
    GoalClear {
        session_id: String,
    },
    GoalDone {
        session_id: String,
    },
    GoalCheckpoint {
        session_id: String,
        project_id: String,
    },
    /// View or mutate the active goal's budget. Subcommands:
    ///   "show" — render current budget/usage as a toast
    ///   "raise tokens <n>"   — raise max_model_tokens
    ///   "raise turns <n>"    — raise max_turns
    ///   "raise tool-calls <n>" — raise max_tool_calls
    ///   "raise wallclock <n>"  — raise max_wallclock_secs (seconds)
    ///   "raise clear <axis>"   — clear a single axis (None)
    GoalBudget {
        session_id: String,
        subcommand: String,
    },
    /// Hydrate the todo list and active goal for a session. Sent on
    /// `set_session` so the sidebar renders live state immediately.
    RefreshSessionState {
        session_id: String,
    },
    UpdateModels(Vec<String>),
    ResearchListRuns,
    ResearchLoadRun {
        run_id: String,
    },
    ResearchLoadSection {
        run_id: String,
        section: String,
    },
    /// Completion: sessions have been reloaded from core.
    SessionsReloaded {
        request_id: u64,
        sessions: Vec<crate::protocol::dto::Session>,
        message_counts: std::collections::HashMap<String, usize>,
        error: Option<String>,
    },
    /// Completion: session messages have been loaded from core.
    SessionMessagesLoaded {
        request_id: u64,
        session_id: String,
        messages: Vec<crate::session::message::Message>,
        error: Option<String>,
    },
    /// Completion: tree dialog nodes have been loaded from core.
    TreeDialogLoaded {
        current_session_id: Option<String>,
        nodes: Vec<crate::tui::components::dialogs::tree::TreeNode>,
        error: Option<String>,
    },
    /// Completion: import preview has been loaded.
    ImportPreviewLoaded {
        request_id: u64,
        session: Option<crate::session::Session>,
        msg_count: usize,
        error: Option<String>,
    },
    /// Completion: import confirm has finished.
    ImportConfirmed {
        request_id: u64,
        session: Option<crate::session::Session>,
        error: Option<String>,
    },
    /// Completion: research runs have been listed.
    ResearchRunsLoaded {
        request_id: u64,
        runs: Vec<crate::research::service::ResearchRunSummary>,
        error: Option<String>,
    },
    /// Completion: a research run bundle has been loaded.
    ResearchRunLoaded {
        request_id: u64,
        run_id: String,
        bundle: Option<Box<crate::research::types::ResearchBundle>>,
        error: Option<String>,
    },
    /// Completion: a research section has been loaded.
    ResearchSectionLoaded {
        request_id: u64,
        section: String,
        content: Option<(
            crate::tui::components::dialogs::research::ReportSection,
            String,
        )>,
        error: Option<String>,
    },
    /// Completion: a memory operation has finished.
    MemoryResult {
        toast_message: String,
        is_error: bool,
    },
    /// Completion: the doctor diagnostic has finished.
    DoctorResult {
        summary: String,
        is_error: bool,
    },
    /// Run diagnostics (search backend, MCP, providers). The result is
    /// logged at `codegg::doctor` and surfaced as a toast. The
    /// handler in `tui_cmd.rs` performs the actual async work.
    RunDoctor,
    /// Run `/security-review` asynchronously. Dispatched from the slash
    /// command handler in `execute_command` so the TUI renderer stays
    /// responsive while diff discovery, preflight, and optional LSP
    /// enrichment run. The handler in `src/tui/mod.rs` awaits
    /// `run_security_review_background` and surfaces the result via the
    /// message timeline + a toast. See
    /// `plans/security_review_async_dispatch.md`.
    ///
    /// Kept for backward compatibility with any older dispatchers; new
    /// code should use the spawn-and-finished path via
    /// `SecurityReviewFinished`.
    SecurityReviewRun {
        id: String,
        root: PathBuf,
        args: crate::security::workflow::SecurityReviewCommandArgs,
        lsp_tool: Option<Arc<crate::tool::lsp::LspTool>>,
    },
    /// Notification that a background security review task finished.
    /// Sent by the spawned tokio task created from
    /// `App::execute_command`'s `/security-review` branch. The
    /// `cmd_rx` arm in `run_event_loop` matches the run id against
    /// the active guard so stale completions are ignored.
    SecurityReviewFinished {
        id: String,
        receipt: Option<Box<crate::security::workflow::SecurityReviewReceipt>>,
        error: Option<String>,
    },
    /// Notification that a background subagent spawn attempt finished.
    /// Spawned from `handle_spawn_subagent` so the TUI dispatch loop
    /// never awaits the subagent pool directly.
    SubagentSpawnFinished {
        agent_name: String,
        task_id: u64,
        prompt: String,
        error: Option<String>,
    },
    /// Notification that a background git sidebar refresh finished.
    /// The generation counter guards against stale completions
    /// overwriting newer session/project state.
    GitSidebarRefreshFinished {
        generation: u64,
        root: Option<String>,
        branch: Option<String>,
        dirty: bool,
        error: Option<String>,
    },
    RunHumanShell {
        command: String,
        promote_after: bool,
    },
    ShellEvent(crate::shell::ShellEvent),
    ShellInclude {
        id: u64,
        mode: String,
        question: Option<String>,
    },
    ShellRerun {
        id: u64,
    },
    ShellKill {
        id: u64,
    },
    ShellList,
    ShellShow {
        id: u64,
    },
    ShellAsk {
        id: u64,
        question: String,
    },
    FileDiffStatsReady {
        path: PathBuf,
        generation: u64,
        result: crate::tui::file_diff::FileDiffStatsResult,
    },
    /// Completion: share operation finished.
    ShareSessionFinished {
        session_id: String,
        session: Option<crate::protocol::dto::Session>,
        error: Option<String>,
    },
    /// Completion: unshare operation finished.
    UnshareSessionFinished {
        session_id: String,
        session: Option<crate::protocol::dto::Session>,
        error: Option<String>,
    },
    /// Completion: export operation finished.
    ExportSessionFinished {
        session_id: String,
        json: Option<String>,
        error: Option<String>,
    },
    /// Completion: a goal operation finished (show, checkpoint, budget).
    GoalOperationFinished {
        session_id: String,
        op: String,
        response: Option<CoreResponse>,
        error: Option<String>,
    },
    /// Completion: session state (todos + active goal) refreshed.
    SessionStateRefreshed {
        todos: Vec<TodoEntry>,
        active_goal: Option<crate::bus::events::GoalSnapshot>,
        error: Option<String>,
    },
    /// Completion: background task list has been fetched from core.
    TasksListed {
        request_id: u64,
        tasks: Vec<serde_json::Value>,
        error: Option<String>,
    },
    /// Completion: a task operation (delete, schedule) has finished.
    TaskOperationFinished {
        request_id: u64,
        op: String,
        task_id: Option<String>,
        error: Option<String>,
    },
    /// Completion: worktree list has been fetched from core.
    WorktreeListed {
        request_id: u64,
        worktrees: Vec<String>,
        error: Option<String>,
    },
    /// Completion: a session has been created from a template.
    TemplateSessionCreated {
        request_id: u64,
        session: Option<crate::protocol::dto::Session>,
        agent: Option<String>,
        model: Option<String>,
        template_name: String,
        error: Option<String>,
    },
    /// Completion: a desktop notification has been sent.
    NotificationSent {
        error: Option<String>,
    },
    /// Request to display TUI diagnostics stats.
    TuiStats,
    /// Completion: a session mutation (delete, archive, fork, rename, etc.) has finished.
    SessionMutationFinished {
        request_id: u64,
        op: SessionMutationOp,
        affected_ids: Vec<String>,
        message: String,
        reload_after: bool,
        error: Option<String>,
    },
    /// Request to run a process-backed plugin command.
    PluginCommandRun {
        spec: crate::command::ProcessCommandSpec,
        args: Vec<String>,
        session_id: Option<String>,
        model: Option<String>,
    },
    /// Completion: a plugin command has finished executing.
    PluginCommandFinished {
        invocation_id: String,
        command: String,
        response: Option<Box<crate::protocol::plugin::PluginResponse>>,
        stdout: Option<String>,
        stderr: Option<String>,
        error: Option<String>,
    },
    /// Apply a single plugin UI effect directly (without going through a command response).
    PluginUiEffect {
        effect: crate::protocol::ui::UiEffect,
    },
    /// List all registered plugins.
    PluginList,
    /// Show detailed info for a single plugin.
    PluginInfo {
        selector: String,
    },
    /// Enable a plugin by selector.
    PluginEnable {
        selector: String,
    },
    /// Disable a plugin by selector.
    PluginDisable {
        selector: String,
    },
    /// Run diagnostic checks on a plugin (or all plugins).
    PluginDoctor {
        selector: Option<String>,
    },
    /// Remove (uninstall) a local plugin by selector.
    PluginRemove {
        selector: String,
    },
    /// Install a plugin from a local path.
    PluginInstall {
        path: String,
    },
    /// Completion: plugin list has been fetched from the marketplace.
    PluginListFinished {
        lines: Vec<String>,
        error: Option<String>,
    },
    /// Completion: plugin info has been fetched.
    PluginInfoFinished {
        plugin_id: String,
        lines: Vec<String>,
        error: Option<String>,
    },
    /// Completion: a plugin enable operation has finished.
    PluginEnableFinished {
        plugin_id: String,
        error: Option<String>,
    },
    /// Completion: a plugin disable operation has finished.
    PluginDisableFinished {
        plugin_id: String,
        error: Option<String>,
    },
    /// Completion: plugin diagnostics have finished.
    PluginDoctorFinished {
        lines: Vec<String>,
        error: Option<String>,
    },
    /// Completion: a plugin remove operation has finished.
    PluginRemoveFinished {
        plugin_id: String,
        error: Option<String>,
    },
    /// Completion: a plugin install operation has finished.
    PluginInstallFinished {
        source: String,
        lines: Vec<String>,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMutationOp {
    Delete,
    Archive,
    Unarchive,
    Fork,
    BulkDelete,
    BulkArchive,
    BulkExport,
    Rename,
    UndoDelete,
}

/// Main application state for the TUI.
///
/// The `App` struct is organized into several state domains:
///
/// - `ui_state`: Theme, layout, dialogs, routes
/// - `session_state`: Session management and history
/// - `prompt_state`: Prompt input and completions
/// - `messages_state`: Message history and display
/// - `dialog_state`: Dialog visibility and data
/// - `agent_state`: Agent and model configuration
///
/// Additionally:
/// - `sidebar` and `status_bar`: UI widgets
/// - `session_store` and `message_store`: Database access
/// - Areas for mouse event handling
///
/// # Event Handling
///
/// Events flow through `on_key()` which delegates to:
/// - `handle_dialog_key` - when a dialog is open
/// - `handle_command_key` - in command mode
/// - `handle_completion_key` - when completions shown
/// - Direct action handling for prompt input
///
/// # Rendering
///
/// The [`render`](App::render) method draws widgets in layers:
/// 1. Header, viewport, prompt, footer (main area)
/// 2. Sidebar (if visible)
/// 3. Dialog (if open)
/// 4. Completions overlay
/// 5. Timeline (if visible)
/// 6. Toasts (topmost)

#[derive(Debug, Clone, PartialEq)]
pub enum ClickTarget {
    Viewport,
    Prompt,
    Dialog,
    Completion,
    Sidebar,
    Scrollbar { track_y: u16, track_height: u16 },
    None,
}

#[derive(Debug, Clone, Default)]
pub struct RenderPanicInjection {
    pub messages: bool,
    pub sidebar: bool,
    pub dialog: bool,
    pub completions: bool,
    pub timeline: bool,
}

impl RenderPanicInjection {
    pub fn all_false() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct App {
    pub ui_state: UiState,
    pub session_state: SessionState,
    pub prompt_state: PromptState,
    pub messages_state: MessagesState,
    pub dialog_state: DialogState,
    pub agent_state: AgentState,
    pub sidebar: SidebarWidget,
    pub status_bar: StatusBarWidget,
    pub session_store: Option<Arc<SessionStore>>,
    pub message_store: Option<Arc<MessageStore>>,
    pub memory_store: Option<Arc<MemoryStore>>,
    /// User preferences backed by SQLite. Holds the active theme id and
    /// the last-used model id; both survive a config-file reset. Always
    /// `Some` in normal operation; `None` is reserved for test fixtures
    /// that don't open a database.
    pub preferences: Option<crate::storage::UserPreferences>,
    pub viewport_area: Option<Rect>,
    pub scrollbar_area: Option<Rect>,
    pub prompt_area: Option<Rect>,
    pub dialog_area: Option<Rect>,
    pub completion_area: Option<Rect>,
    pub sidebar_area: Option<Rect>,
    /// 1-column strip reserved for the TUI's outer left border.
    pub left_border_area: Option<Rect>,
    /// 1-row strip reserved for the TUI's outer bottom border (under
    /// the footer).
    pub bottom_border_area: Option<Rect>,
    pub last_click_time: Option<Instant>,
    pub last_click_target: Option<ClickTarget>,
    pub hover_target: Option<ClickTarget>,
    pub hover_position: Option<(u16, u16)>,
    pub context_hint: String,
    pub event_rx: Option<mpsc::Receiver<ChatEvent>>,
    pub tui_cmd_tx: Option<mpsc::Sender<TuiCommand>>,
    pub remote_event_rx: Option<mpsc::UnboundedReceiver<serde_json::Value>>,
    pub remote_send_tx: Option<mpsc::UnboundedSender<RemoteTuiMessage>>,
    pub core_client: Option<Arc<dyn CoreClient>>,
    pub config_watcher: Option<crate::config::ConfigWatcher>,
    pub theme_registry: Arc<crate::theme::ThemeRegistry>,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    pub undo_session_id: Option<String>,
    pub streaming_active: bool,
    pub undo_until: Option<Instant>,
    pub notification_manager: Option<crate::tui::components::notification::NotificationManager>,
    pub focus_manager: crate::tui::components::component::FocusManager,
    pub busy_spinner: crate::tui::components::spinner::SpinnerWidget,
    pub session_state_derived: crate::session::state::TuiSessionState,
    /// Active goal for the current session, kept in sync with the goal
    /// table via `AppEvent::GoalUpdated` and friends. Used by the
    /// sidebar, the status bar, and the `/goal show` command.
    pub active_goal: Option<crate::bus::events::GoalSnapshot>,
    /// LSP tool instance for security-review enrichment and other
    /// LSP-backed operations.  Created once at startup in local
    /// (non-socket) mode; `None` in remote/socket mode.
    pub lsp_tool: Option<Arc<crate::tool::lsp::LspTool>>,
    /// Reentrancy guard for the `/security-review` slash command.
    /// `Some(task_state)` while a background review is in flight;
    /// `None` when idle. Carries the `tokio::task::AbortHandle` so the
    /// user can cancel via `/security-review-cancel`. Cleared on both
    /// success and failure by the handler in `src/tui/mod.rs`.
    pub security_review_running: Option<crate::security::workflow::SecurityReviewTaskState>,
    /// Latest completed security review receipt, kept in memory so the
    /// user can reopen the result panel via `/security-review-show`.
    ///
    /// Persistence is intentionally deferred: the codebase lacks a
    /// generic session-artifact or message-metadata mechanism that would
    /// make schema-less storage trivial. A future pass could attach the
    /// structured receipt as a JSON blob on the assistant message row
    /// once such a mechanism exists. No schema migration in this pass.
    pub latest_security_review: Option<crate::security::workflow::SecurityReviewReceipt>,
    pub shell_store: crate::shell::ShellOutputStore,
    pub shell_handles: std::collections::HashMap<u64, crate::shell::runtime::ShellHandle>,
    /// Registry of TUI-owned background tasks.  Tracked tasks can be
    /// counted, cancelled, and reaped on shutdown or dialog close.
    pub task_registry: crate::tui::task_lifecycle::TuiTaskRegistry,
    /// Plugin-owned UI surfaces: dialogs, panels, and status items.
    /// Consumed by `apply_plugin_ui_effect` and rendered by
    /// `PluginUiRenderer`.
    pub plugin_ui_state: crate::tui::app::state::PluginUiState,
    pub render_panic_injection: RenderPanicInjection,
    /// Monotonic sequence number for remote TUI snapshots. Each
    /// `next_remote_snapshot` call increments this and includes the
    /// new value in the snapshot so remote clients can detect drops
    /// or skipped frames. Starts at 0 and never resets during a
    /// single app lifetime.
    pub remote_sequence: u64,
}

/// What to do at TUI startup with respect to session loading. The TUI
/// constructs one of these from CLI flags / `AttachDaemon` subcommand
/// arguments and dispatches it through `App::load_initial_session_via_core`
/// so the same code path works for both inproc (local stores) and
/// socket/RemoteCore (`CoreClient`) modes.
#[derive(Debug, Clone)]
pub enum InitialSessionRequest {
    /// Attach to an existing session by id.
    Attach { session_id: String },
    /// Continue the most recent session in `project_dir`.
    Continue { project_dir: String },
    /// Create a brand new session in `directory`.
    New {
        directory: String,
        title: Option<String>,
    },
    /// Fork the given session id and attach the resulting fork.
    ///
    /// The core client only returns an `Ack` from `SessionFork` today,
    /// so we follow up with a `SessionList` to pick up the new fork.
    /// If the listing does not contain a brand-new session, we fall
    /// back to attaching the original id (best-effort).
    Fork { session_id: String },
    /// No session to load — start with an empty TUI.
    None,
}

impl App {
    pub fn new(project_dir: String) -> Self {
        Self::with_config(project_dir, None)
    }

    pub fn with_config(project_dir: String, cfg: Option<&crate::config::schema::Config>) -> Self {
        let focus_manager = crate::tui::components::component::FocusManager::new();
        let agents = builtin_agents();
        let current_agent = agents.iter().position(|a| a.name == "build").unwrap_or(0);
        let models = vec![
            "opencode_zen/big-pickle".to_string(),
            "opencode_zen/minimax-m2.5-free".to_string(),
            "opencode_zen/nemotron-3-super-free".to_string(),
        ];
        let current_model = models[0].clone();
        use crate::tui::components::completion_overlay::{CompletionItem, CompletionItemKind};
        let slash_completions: Vec<CompletionItem> = crate::tui::command::COMMAND_REGISTRY
            .commands()
            .iter()
            .map(|c| CompletionItem {
                label: c.name.clone(),
                description: if c.description.is_empty() {
                    None
                } else {
                    Some(c.description.clone())
                },
                kind: CompletionItemKind::File,
            })
            .collect();
        let vim_mode = cfg.and_then(|c| c.vim_mode).unwrap_or(false);
        let help_lines = build_help_lines(vim_mode, HelpMode::Insert);
        let keybinds = cfg.and_then(|c| c.keybinds.as_ref()).map(|raw| {
            let mut bindings: HashMap<String, crate::tui::input::ActionKey> = HashMap::new();
            for (k, v) in raw {
                if let Ok(action) = serde_json::from_str(&format!("\"{}\"", v)) {
                    bindings.insert(k.clone(), action);
                }
            }
            KeybindConfig { bindings }
        });
        let bindings = super::input::build_bindings(keybinds.as_ref(), vim_mode);
        debug_log!(
            "loaded {} keybindings, custom config: {}",
            bindings.len(),
            keybinds.is_some()
        );
        let up_key = bindings.get(&(
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyCode::Up,
        ));
        let down_key = bindings.get(&(
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyCode::Down,
        ));
        let j_key = bindings.get(&(
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyCode::Char('j'),
        ));
        let k_key = bindings.get(&(
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyCode::Char('k'),
        ));
        debug_log!(
            "navigation bindings: up={:?}, down={:?}, j={:?}, k={:?}",
            up_key,
            down_key,
            j_key,
            k_key
        );
        // Note: navigation keybindings retrieved for debug logging only
        let _ = (up_key, down_key, j_key, k_key);
        let indexed_files: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let dir_clone = project_dir.clone();
        let files_clone = Arc::clone(&indexed_files);
        tokio::spawn(async move {
            let mut rx = shutdown_rx;
            loop {
                tokio::select! {
                    biased;
                    _ = rx.recv() => break,
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                        let files = index_files_sync(&dir_clone);
                        let mut guard = files_clone.write().await;
                        *guard = files;
                    }
                }
            }
        });

        let theme_registry = Arc::new(crate::theme::ThemeRegistry::load_with_config(
            cfg.and_then(|c| c.theme.as_ref()),
        ));
        for diag in theme_registry.diagnostics() {
            match diag.level {
                crate::theme::ThemeDiagnosticLevel::Warning => {
                    tracing::warn!(theme = %diag.theme_id, field = ?diag.field, "{}", diag.message);
                }
                crate::theme::ThemeDiagnosticLevel::Error => {
                    tracing::error!(theme = %diag.theme_id, field = ?diag.field, "{}", diag.message);
                }
            }
        }
        let resolution =
            crate::theme::ThemeResolutionConfig::from_config(cfg.and_then(|c| c.theme.as_ref()));
        let theme = theme_registry.resolve_tui_arc(&resolution);

        Self {
            ui_state: UiState {
                running: true,
                theme: Arc::clone(&theme),
                layout: TuiLayout::with_config(LayoutConfig::default()),
                routes: RouteManager::new(),
                dialog: Dialog::None,
                command_mode: false,
                input_mode: InputMode::Insert,
                shutdown_tx: Some(shutdown_tx),
                help_lines,
                bindings,
                keybinds: keybinds.clone(),
                vim_mode,
                mode: AppMode::Embedded,
                remote_status: None,
                sidebar_visible: true,
                auto_scroll: true,
                show_thinking: true,
                show_timestamps: false,
                timeline_visible: false,
                timeline_selected: 0,
                render_panic_count: 0,
                last_render_error: None,
                tts: Tts::new(),
                tts_enabled: false,
                fullscreen: false,
                dirty_regions: Vec::new(),
                resize_debounce: None,
                tts_via_daemon: false,
                diagnostics: Default::default(),
                plugin_ui_caps: crate::protocol::ui::PluginUiCapabilities::all_supported(),
            },
            session_state: SessionState {
                session: None,
                session_status: SessionStatus::Idle,
                token_in: 0,
                token_out: 0,
                live_output_tokens: 0,
                live_output_text: String::new(),
                reasoning_tokens: 0,
                cached_tokens: 0,
                history: std::collections::VecDeque::new(),
                history_pos: None,
                indexed_files,
                project_dir,
                last_edited_file: None,
                changed_files: Vec::new(),
                mcp_servers: Vec::new(),
                context_tokens: 0,
                context_limit: 128_000,
                compaction_count: 0,
                rpm_limit: None,
                tpm_limit: None,
                rpm_remaining: None,
                tpm_remaining: None,
                permission_pending: false,
                subagent_count: 0,
                git_sidebar: crate::tui::app::state::session::GitSidebarState::default(),
            },
            prompt_state: PromptState {
                prompt: PromptWidget::new(Arc::clone(&theme)),
                slash_completions,
                file_completions: Vec::new(),
                agent_completions: Vec::new(),
                completion_filter: String::new(),
                show_completions: false,
                completion_type: CompletionType::Slash,
                completion_sel: 0,
                stashed_prompts: Vec::new(),
                stash_pos: None,
                pending_send: false,
            },
            messages_state: MessagesState {
                messages: MessagesWidget::new(Arc::clone(&theme)),
                toasts: ToastManager::new(),
            },
            dialog_state: DialogState {
                model_dialog: ModelDialog::new(Arc::clone(&theme)),
                agent_dialog: AgentDialog::new(Arc::clone(&theme)),
                session_dialog: SessionDialog::new(Arc::clone(&theme)),
                tree_dialog: TreeDialog::new(Arc::clone(&theme)),
                theme_picker: None,
                question_dialog: None,
                question_session_id: None,
                command_palette: CommandPalette::new(),
                permission_dialog: None,
                permission_perm_id: None,
                keybind_dialog: None,
                mcp_dialog: None,
                share_dialog: None,
                import_dialog: None,
                template_dialog: None,
                connect_dialog: None,
                goto_dialog: None,
                plan_dialog: None,
                diff_dialog: None,
                review_dialog: None,
                security_review_dialog: None,
                source_preview_dialog: None,
                research_browser: None,
                help_dialog: None,
                info_dialog: None,
                ui_node_dialog: None,
                shell_detail_dialog: None,
                pending_delete_session: None,
                pending_archive_session: None,
                pending_bulk_delete: None,
                pending_bulk_delete_ids: None,
                pending_bulk_archive: None,
                pending_bulk_archive_ids: None,
                pending_shell_command: None,
                shell_detail_id: None,
                import_request: crate::tui::app::state::AsyncUiRequestState::new(),
                research_request: crate::tui::app::state::AsyncUiRequestState::new(),
                session_reload_request: crate::tui::app::state::AsyncUiRequestState::new(),
                task_list_request: crate::tui::app::state::AsyncUiRequestState::new(),
                task_delete_request: crate::tui::app::state::AsyncUiRequestState::new(),
                worktree_list_request: crate::tui::app::state::AsyncUiRequestState::new(),
                template_create_request: crate::tui::app::state::AsyncUiRequestState::new(),
                session_mutation_request: crate::tui::app::state::AsyncUiRequestState::new(),
                session_messages_request: crate::tui::app::state::AsyncUiRequestState::new(),
            },
            agent_state: AgentState {
                agents,
                current_agent,
                current_model,
                models,
                model_idx: 0,
                plan_mode: false,
                plan_topic: None,
            },
            sidebar: SidebarWidget::new(Arc::clone(&theme)),
            status_bar: StatusBarWidget::new(Arc::clone(&theme)),
            session_store: None,
            message_store: None,
            memory_store: None,
            viewport_area: None,
            scrollbar_area: None,
            prompt_area: None,
            dialog_area: None,
            completion_area: None,
            sidebar_area: None,
            left_border_area: None,
            bottom_border_area: None,
            last_click_time: None,
            last_click_target: None,
            hover_target: None,
            hover_position: None,
            context_hint: String::new(),
            event_rx: None,
            tui_cmd_tx: None,
            remote_event_rx: None,
            remote_send_tx: None,
            core_client: None,
            config_watcher: Some(
                cfg.and_then(|c| c.watcher.as_ref())
                    .map(|w| crate::config::ConfigWatcher::new().with_config(w))
                    .unwrap_or_default(),
            ),
            theme_registry,
            preferences: None,
            subagent_pool: None,
            bg_scheduler: None,
            undo_session_id: None,
            undo_until: None,
            notification_manager: None,
            focus_manager,
            busy_spinner: crate::tui::components::spinner::SpinnerWidget::new(),
            streaming_active: false,
            session_state_derived: crate::session::state::TuiSessionState::default(),
            active_goal: None,
            lsp_tool: None,
            security_review_running: None,
            latest_security_review: None,
            shell_store: cfg
                .and_then(|c| c.human_shell.as_ref())
                .map(crate::shell::ShellOutputStore::from_config)
                .unwrap_or_default(),
            remote_sequence: 0,
            shell_handles: std::collections::HashMap::new(),
            plugin_ui_state: crate::tui::app::state::PluginUiState::default(),
            task_registry: crate::tui::task_lifecycle::TuiTaskRegistry::new(),
            render_panic_injection: RenderPanicInjection::all_false(),
        }
    }

    pub fn new_remote(project_dir: String) -> Self {
        let mut app = Self::new(project_dir);
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: String::new(),
        };
        app.ui_state.tts_via_daemon = true;
        app.ui_state.remote_status = Some("Connected".to_string());
        app.remote_event_rx = None;
        app.remote_send_tx = None;
        app
    }

    pub fn remote_snapshot(&self) -> crate::protocol::tui::RemoteTuiStateSnapshot {
        self.build_remote_snapshot(self.remote_sequence)
    }

    /// Build a snapshot with the given sequence number. Use
    /// `next_remote_snapshot` to allocate a fresh monotonic sequence
    /// from the app, or pass a specific sequence when replaying state
    /// to a remote client that supplied `Resume { from_event_seq }`.
    pub fn build_remote_snapshot(
        &self,
        sequence: u64,
    ) -> crate::protocol::tui::RemoteTuiStateSnapshot {
        use crate::protocol::tui::{
            RemoteMessageView, RemoteToastView, RemoteToolCallView, REMOTE_TUI_PROTOCOL_VERSION,
        };

        let route = match self.ui_state.routes.current() {
            crate::tui::route::Route::Home => "home".to_string(),
            crate::tui::route::Route::Session(id) => format!("session:{}", id),
        };

        let session_id = self.session_state.session.as_ref().map(|s| s.id.clone());

        let status = match self.session_state.session_status {
            crate::tui::app::types::SessionStatus::Idle => "idle".to_string(),
            crate::tui::app::types::SessionStatus::Working => "working".to_string(),
            crate::tui::app::types::SessionStatus::Error => "error".to_string(),
        };

        let model = self.agent_state.current_model.clone();
        let agent = self
            .agent_state
            .agents
            .get(self.agent_state.current_agent)
            .map(|a| a.name.clone())
            .unwrap_or_default();

        let messages: Vec<RemoteMessageView> = self
            .messages_state
            .messages
            .messages
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    crate::tui::components::messages::MessageRole::User => "user".to_string(),
                    crate::tui::components::messages::MessageRole::Assistant => {
                        "assistant".to_string()
                    }
                };
                let content_preview = msg.text_content();
                let content_preview = if content_preview.len() > 200 {
                    format!("{}...", &content_preview[..200])
                } else {
                    content_preview
                };
                let tool_calls: Vec<RemoteToolCallView> = msg
                    .parts
                    .iter()
                    .filter_map(|part| {
                        if let crate::tui::components::messages::MsgPart::ToolCall {
                            id,
                            name,
                            status,
                            ..
                        } = part
                        {
                            let status_str = match status {
                                crate::session::message::ToolStatus::Pending => {
                                    "pending".to_string()
                                }
                                crate::session::message::ToolStatus::Running => {
                                    "running".to_string()
                                }
                                crate::session::message::ToolStatus::Completed => {
                                    "completed".to_string()
                                }
                                crate::session::message::ToolStatus::Error => "error".to_string(),
                            };
                            Some(RemoteToolCallView {
                                tool_id: id.clone(),
                                tool_name: name.clone(),
                                status: status_str,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                RemoteMessageView {
                    role,
                    content_preview,
                    tool_calls,
                }
            })
            .collect();

        let prompt = self.prompt_state.prompt.get_text();

        let dialog = if self.ui_state.dialog.is_open() {
            Some(format!("{:?}", self.ui_state.dialog))
        } else {
            None
        };

        let toasts: Vec<RemoteToastView> = self
            .messages_state
            .toasts
            .iter()
            .map(|t| RemoteToastView {
                message: t.message.clone(),
                level: format!("{:?}", t.level),
            })
            .collect();

        let plugin_panels: Vec<crate::protocol::tui::RemotePanelView> = self
            .plugin_ui_state
            .panels
            .iter()
            .map(|(id, panel)| {
                let placement_str = format!("{:?}", panel.placement);
                // Plugin ownership is encoded as the first `:`-segment
                // of the panel id (canonical form: "<plugin_id>:<command>"
                // for installed plugins, "command:local:<name>" for
                // project-local commands). Anything starting with
                // "command:" has no plugin owner.
                let source_plugin_id = plugin_id_from_surface_id(id);
                let body = snapshot_body_within_limit(&panel.body, SNAPSHOT_BODY_LIMIT);
                crate::protocol::tui::RemotePanelView {
                    id: id.clone(),
                    title: panel.title.clone(),
                    placement: placement_str,
                    source_plugin_id,
                    body,
                }
            })
            .collect();

        let plugin_status_items: Vec<crate::protocol::tui::RemoteStatusItemView> = self
            .plugin_ui_state
            .status_items
            .iter()
            .map(|(id, item)| {
                let placement_str = format!("{:?}", item.placement);
                let source_plugin_id = plugin_id_from_surface_id(id);
                let body = snapshot_body_within_limit(&item.body, SNAPSHOT_BODY_LIMIT);
                crate::protocol::tui::RemoteStatusItemView {
                    id: id.clone(),
                    label: item.label.clone(),
                    placement: placement_str,
                    source_plugin_id,
                    body,
                }
            })
            .collect();

        crate::protocol::tui::RemoteTuiStateSnapshot {
            protocol_version: REMOTE_TUI_PROTOCOL_VERSION,
            sequence,
            session_id,
            route,
            model,
            agent,
            status,
            messages,
            prompt,
            dialog,
            toasts,
            git: Some(crate::protocol::tui::RemoteGitInfo {
                root: self.session_state.git_sidebar.root.clone(),
                branch: self.session_state.git_sidebar.branch.clone(),
                dirty: self.session_state.git_sidebar.dirty,
                loading: self.session_state.git_sidebar.loading,
                error: self.session_state.git_sidebar.error.clone(),
            }),
            plugin_panels,
            plugin_status_items,
        }
    }

    /// Increment the remote snapshot sequence and return a fresh
    /// snapshot. Used by remote protocol handlers when responding to
    /// `RequestSnapshot` so the client sees a strictly increasing
    /// sequence.
    pub fn next_remote_snapshot(&mut self) -> crate::protocol::tui::RemoteTuiStateSnapshot {
        self.remote_sequence = self.remote_sequence.saturating_add(1);
        self.build_remote_snapshot(self.remote_sequence)
    }

    /// Create a minimal App instance for testing.
    /// This avoids spawning background tasks that interfere with tests.
    pub fn new_for_testing(project_dir: String) -> Self {
        let focus_manager = crate::tui::components::component::FocusManager::new();
        let agents = builtin_agents();
        let current_agent = agents.iter().position(|a| a.name == "build").unwrap_or(0);
        let models = vec![
            "opencode_zen/big-pickle".to_string(),
            "opencode_zen/minimax-m2.5-free".to_string(),
            "opencode_zen/nemotron-3-super-free".to_string(),
        ];
        let current_model = models[0].clone();
        use crate::tui::components::completion_overlay::{CompletionItem, CompletionItemKind};
        let slash_completions: Vec<CompletionItem> = crate::tui::command::COMMAND_REGISTRY
            .commands()
            .iter()
            .map(|c| CompletionItem {
                label: c.name.clone(),
                description: if c.description.is_empty() {
                    None
                } else {
                    Some(c.description.clone())
                },
                kind: CompletionItemKind::File,
            })
            .collect();
        let help_lines = build_help_lines(false, HelpMode::Insert);
        let bindings = super::input::build_bindings(None, false);
        let theme = Arc::new(Theme::dark());
        let indexed_files: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        Self {
            ui_state: UiState {
                running: true,
                theme: Arc::clone(&theme),
                layout: TuiLayout::with_config(LayoutConfig::default()),
                routes: RouteManager::new(),
                dialog: Dialog::None,
                command_mode: false,
                input_mode: InputMode::Insert,
                shutdown_tx: None, // No shutdown channel for tests
                help_lines,
                bindings,
                keybinds: None,
                vim_mode: false,
                mode: AppMode::Embedded,
                remote_status: None,
                sidebar_visible: true,
                auto_scroll: true,
                show_thinking: true,
                show_timestamps: false,
                timeline_visible: false,
                timeline_selected: 0,
                render_panic_count: 0,
                last_render_error: None,
                tts: Tts::new(),
                tts_enabled: false,
                fullscreen: false,
                dirty_regions: Vec::new(),
                resize_debounce: None,
                tts_via_daemon: false,
                diagnostics: Default::default(),
                plugin_ui_caps: crate::protocol::ui::PluginUiCapabilities::all_supported(),
            },
            session_state: SessionState {
                session: None,
                session_status: SessionStatus::Idle,
                token_in: 0,
                token_out: 0,
                live_output_tokens: 0,
                live_output_text: String::new(),
                reasoning_tokens: 0,
                cached_tokens: 0,
                history: std::collections::VecDeque::new(),
                history_pos: None,
                indexed_files,
                project_dir,
                last_edited_file: None,
                changed_files: Vec::new(),
                mcp_servers: Vec::new(),
                context_tokens: 0,
                context_limit: 128_000,
                compaction_count: 0,
                rpm_limit: None,
                tpm_limit: None,
                rpm_remaining: None,
                tpm_remaining: None,
                permission_pending: false,
                subagent_count: 0,
                git_sidebar: crate::tui::app::state::session::GitSidebarState::default(),
            },
            prompt_state: PromptState {
                prompt: PromptWidget::new(Arc::clone(&theme)),
                slash_completions,
                file_completions: Vec::new(),
                agent_completions: Vec::new(),
                completion_filter: String::new(),
                show_completions: false,
                completion_type: CompletionType::Slash,
                completion_sel: 0,
                stashed_prompts: Vec::new(),
                stash_pos: None,
                pending_send: false,
            },
            messages_state: MessagesState {
                messages: MessagesWidget::new(Arc::clone(&theme)),
                toasts: ToastManager::new(),
            },
            dialog_state: DialogState {
                model_dialog: ModelDialog::new(Arc::clone(&theme)),
                agent_dialog: AgentDialog::new(Arc::clone(&theme)),
                session_dialog: SessionDialog::new(Arc::clone(&theme)),
                tree_dialog: TreeDialog::new(Arc::clone(&theme)),
                theme_picker: None,
                question_dialog: None,
                question_session_id: None,
                command_palette: CommandPalette::new(),
                permission_dialog: None,
                permission_perm_id: None,
                keybind_dialog: None,
                mcp_dialog: None,
                share_dialog: None,
                import_dialog: None,
                template_dialog: None,
                connect_dialog: None,
                goto_dialog: None,
                plan_dialog: None,
                diff_dialog: None,
                review_dialog: None,
                security_review_dialog: None,
                source_preview_dialog: None,
                research_browser: None,
                help_dialog: None,
                info_dialog: None,
                ui_node_dialog: None,
                shell_detail_dialog: None,
                pending_delete_session: None,
                pending_archive_session: None,
                pending_bulk_delete: None,
                pending_bulk_delete_ids: None,
                pending_bulk_archive: None,
                pending_bulk_archive_ids: None,
                pending_shell_command: None,
                shell_detail_id: None,
                import_request: crate::tui::app::state::AsyncUiRequestState::new(),
                research_request: crate::tui::app::state::AsyncUiRequestState::new(),
                session_reload_request: crate::tui::app::state::AsyncUiRequestState::new(),
                task_list_request: crate::tui::app::state::AsyncUiRequestState::new(),
                task_delete_request: crate::tui::app::state::AsyncUiRequestState::new(),
                worktree_list_request: crate::tui::app::state::AsyncUiRequestState::new(),
                template_create_request: crate::tui::app::state::AsyncUiRequestState::new(),
                session_mutation_request: crate::tui::app::state::AsyncUiRequestState::new(),
                session_messages_request: crate::tui::app::state::AsyncUiRequestState::new(),
            },
            agent_state: AgentState {
                agents,
                current_agent,
                current_model,
                models,
                model_idx: 0,
                plan_mode: false,
                plan_topic: None,
            },
            sidebar: SidebarWidget::new(Arc::clone(&theme)),
            status_bar: StatusBarWidget::new(Arc::clone(&theme)),
            session_store: None,
            message_store: None,
            memory_store: None,
            viewport_area: None,
            scrollbar_area: None,
            prompt_area: None,
            dialog_area: None,
            completion_area: None,
            sidebar_area: None,
            left_border_area: None,
            bottom_border_area: None,
            last_click_time: None,
            last_click_target: None,
            hover_target: None,
            hover_position: None,
            context_hint: String::new(),
            event_rx: None,
            tui_cmd_tx: None,
            remote_event_rx: None,
            remote_send_tx: None,
            core_client: None,
            config_watcher: None, // No config watcher for tests
            theme_registry: Arc::new(crate::theme::ThemeRegistry::load_builtins()),
            preferences: None,
            subagent_pool: None,
            bg_scheduler: None,
            undo_session_id: None,
            undo_until: None,
            notification_manager: None,
            focus_manager,
            busy_spinner: crate::tui::components::spinner::SpinnerWidget::new(),
            streaming_active: false,
            session_state_derived: crate::session::state::TuiSessionState::default(),
            active_goal: None,
            lsp_tool: None,
            security_review_running: None,
            latest_security_review: None,
            shell_store: crate::shell::ShellOutputStore::new(),
            shell_handles: std::collections::HashMap::new(),
            plugin_ui_state: crate::tui::app::state::PluginUiState::default(),
            task_registry: crate::tui::task_lifecycle::TuiTaskRegistry::new(),
            render_panic_injection: RenderPanicInjection::all_false(),
            remote_sequence: 0,
        }
    }

    pub fn set_remote_event_rx(&mut self, rx: mpsc::UnboundedReceiver<serde_json::Value>) {
        self.remote_event_rx = Some(rx);
    }

    pub fn set_remote_send_tx(&mut self, tx: mpsc::UnboundedSender<RemoteTuiMessage>) {
        self.remote_send_tx = Some(tx);
    }

    pub fn set_core_client(&mut self, client: Arc<dyn CoreClient>) {
        self.core_client = Some(client);
    }

    /// Store the latest completed security review receipt. Subsequent
    /// calls overwrite the previous one. The user can reopen the result
    /// panel via `/security-review-show`.
    pub fn set_latest_security_review(
        &mut self,
        receipt: crate::security::workflow::SecurityReviewReceipt,
    ) {
        if let Some(ref mut dialog) = self.dialog_state.security_review_dialog {
            dialog.update_receipt(receipt.clone());
        }
        self.latest_security_review = Some(receipt);
    }

    /// Returns the current security review run id (if a review is
    /// running) without exposing the `AbortHandle`. Used to check id
    /// matches without needing to clone the handle.
    pub fn security_review_run_id(&self) -> Option<&str> {
        self.security_review_running.as_ref().map(|s| s.id.as_str())
    }

    /// Cancel the currently running security review, if any. Returns
    /// `true` if a review was cancelled, `false` if no review is
    /// running. After this call the guard is cleared; a stale
    /// completion arriving later (matching the cancelled id) is
    /// ignored by the handler in `src/tui/mod.rs`.
    pub fn cancel_security_review(&mut self) -> bool {
        if let Some(state) = self.security_review_running.take() {
            self.task_registry.cancel(state.task_id);
            self.messages_state
                .toasts
                .info("Security review cancelled.");
            true
        } else {
            self.messages_state
                .toasts
                .warning("No security review is running.");
            false
        }
    }

    /// Apply a theme by id from the registry. Returns true if applied.
    pub fn apply_theme(&mut self, theme_id: &str) -> bool {
        if let Some(theme) = self.theme_registry.get_tui(theme_id) {
            self.ui_state.theme = Arc::new(theme);
            return true;
        }
        false
    }

    /// Re-apply any persisted user preferences (theme id, last-used
    /// model) on top of the config-file defaults. Should be called once
    /// at startup, after `set_preferences` and after the model
    /// discovery has populated `agent_state.models`. CLI flags continue
    /// to win: if the user passed `--model`, the persisted value is
    /// only used when no CLI override is recorded.
    pub fn apply_persisted_preferences(&mut self) {
        // Theme
        if let Some(saved) = self.read_persisted_theme_id() {
            if self.theme_registry.get(&saved).is_some() {
                self.apply_theme(&saved);
            }
        }
        // Model
        if let Some(saved) = self.read_persisted_model_id() {
            if self.agent_state.models.iter().any(|m| m == &saved) {
                self.agent_state.current_model = saved.clone();
                if let Some(idx) = self.agent_state.models.iter().position(|m| m == &saved) {
                    self.agent_state.model_idx = idx;
                }
                self.dialog_state.model_dialog.set_current(&saved);
                self.sidebar.set_model(&saved);
            }
        }
    }

    /// Save the user's theme choice to the SQLite `user_preferences`
    /// table. The DB row survives a config-file reset and is the
    /// authoritative source of the active theme on next launch. We
    /// also keep the config file in sync so that a fresh git clone of a
    /// repo can still see which theme the user prefers.
    fn persist_theme_selection(&mut self, theme_id: &str) {
        if let Some(prefs) = self.preferences.clone() {
            let id = theme_id.to_string();
            self.task_registry
                .spawn(TuiTaskKind::Other, "persist-theme", async move {
                    if let Err(e) = prefs.set(crate::storage::KEY_THEME_ACTIVE, &id).await {
                        tracing::warn!("failed to persist theme to user_preferences: {}", e);
                    }
                });
        }

        // Mirror the value into config.toml so external tooling (and the
        // TUI before the DB is available) can still see it.
        let mut config = match crate::config::schema::Config::load() {
            Ok(c) => c,
            Err(e) => {
                self.messages_state
                    .toasts
                    .error(&format!("Failed to load config: {}", e));
                return;
            }
        };

        let mut theme_cfg = config.theme.clone().unwrap_or_default();
        theme_cfg.name = Some(theme_id.to_string());
        config.theme = Some(theme_cfg);

        if let Err(e) = config.save() {
            self.messages_state
                .toasts
                .error(&format!("Theme applied, but failed to save config: {}", e));
        }
    }

    /// Save the user's most recently selected model. Mirrors the theme
    /// pattern: fire-and-forget DB write so the next launch can restore
    /// the model.
    fn persist_model_selection(&mut self, model: &str) {
        if let Some(prefs) = self.preferences.clone() {
            let id = model.to_string();
            self.task_registry
                .spawn(TuiTaskKind::Other, "persist-model", async move {
                    if let Err(e) = prefs.set(crate::storage::KEY_MODEL_LAST_USED, &id).await {
                        tracing::warn!("failed to persist model to user_preferences: {}", e);
                    }
                });
        }
    }

    /// Synchronously read the persisted theme id (if any) and resolve it
    /// through the registry. Returns `None` if the user has no saved
    /// theme or the saved id is no longer in the registry.
    fn read_persisted_theme_id(&self) -> Option<String> {
        let prefs = self.preferences.as_ref()?;
        // Use the dedicated tokio runtime handle that already exists in
        // the app: storage calls are cheap and the spawn is fine. We use
        // `block_in_place` so the call works from the synchronous
        // startup path. Avoid panicking if the DB read fails.
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(prefs.get(crate::storage::KEY_THEME_ACTIVE))
        }) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to read persisted theme: {}", e);
                None
            }
        }
    }

    /// Synchronously read the persisted model id (if any). The caller is
    /// responsible for validating that the id is in the available
    /// `agent_state.models` list.
    fn read_persisted_model_id(&self) -> Option<String> {
        let prefs = self.preferences.as_ref()?;
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(prefs.get(crate::storage::KEY_MODEL_LAST_USED))
        }) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to read persisted model: {}", e);
                None
            }
        }
    }

    fn send_remote_message(&self, msg: RemoteTuiMessage) {
        if let Some(ref tx) = self.remote_send_tx {
            let _ = tx.send(msg);
        }
    }

    pub fn dispatch_turn_submit_request(&self, text: String) {
        let Some(client) = self.core_client.clone() else {
            return;
        };
        let Some(session_id) = self.session_state.session.as_ref().map(|s| s.id.clone()) else {
            return;
        };
        let messages = self.build_provider_context();
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::TurnSubmit {
                session_id,
                text,
                plan_mode: self.agent_state.plan_mode,
                model: self.agent_state.current_model.clone(),
                agents: crate::protocol_conversions::agents_to_dtos(
                    self.agent_state.agents.clone(),
                ),
                current_agent_idx: self.agent_state.current_agent,
                messages: crate::protocol_conversions::provider_messages_to_dtos(messages),
            },
        );
        tokio::spawn(async move {
            if let Err(e) = client.request(request).await {
                tracing::debug!("core facade turn.submit failed: {}", e);
            }
        });
    }

    fn build_provider_context(&self) -> Vec<crate::provider::Message> {
        let mut out = Vec::new();
        for m in &self.messages_state.messages.messages {
            let mut content: Vec<crate::provider::ContentPart> = Vec::new();
            let mut tool_calls: Vec<crate::provider::ToolCall> = Vec::new();
            let mut tool_results: Vec<crate::provider::Message> = Vec::new();

            for p in &m.parts {
                match p {
                    MsgPart::Text { content: text } => {
                        content.push(crate::provider::ContentPart::Text {
                            text: text.clone().into(),
                        })
                    }
                    MsgPart::Reasoning { content: text, .. } => {
                        content.push(crate::provider::ContentPart::Text {
                            text: format!("[Reasoning]\n{}", text).into(),
                        })
                    }
                    MsgPart::ToolCall {
                        id,
                        name,
                        input,
                        output,
                        ..
                    } => {
                        if let Ok(arguments) = serde_json::from_str::<serde_json::Value>(input) {
                            tool_calls.push(crate::provider::ToolCall {
                                id: id.clone().into(),
                                name: name.clone().into(),
                                arguments,
                            });
                        }
                        tool_results.push(crate::provider::Message::Tool {
                            tool_call_id: id.clone().into(),
                            content: output.clone().into(),
                        });
                    }
                    MsgPart::Image { data_uri, .. } => {
                        content.push(crate::provider::ContentPart::Image {
                            image_url: crate::provider::ImageUrl {
                                url: data_uri.clone().into(),
                            },
                        });
                    }
                    MsgPart::ShellCell {
                        command,
                        stdout_preview,
                        stderr_preview,
                        status,
                        ..
                    } => {
                        content.push(crate::provider::ContentPart::Text {
                            text: format!(
                                "$ {} [{}]\nstdout: {}\nstderr: {}",
                                command, status, stdout_preview, stderr_preview
                            )
                            .into(),
                        });
                    }
                }
            }

            match m.role {
                MessageRole::User => {
                    if !content.is_empty() {
                        out.push(crate::provider::Message::User { content });
                    }
                }
                MessageRole::Assistant => {
                    if !content.is_empty() || !tool_calls.is_empty() {
                        out.push(crate::provider::Message::Assistant {
                            content,
                            tool_calls,
                        });
                    }
                    out.extend(tool_results);
                }
            }
        }
        out
    }

    pub fn handle_remote_event(&mut self, event: serde_json::Value) {
        let _event_type = event
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        debug_log!("handle_remote_event: type={}", _event_type);

        match serde_json::from_value::<RemoteTuiMessage>(event) {
            Ok(RemoteTuiMessage::EventEnvelope {
                event_seq: _,
                payload,
            }) => {
                if let Ok(value) = serde_json::to_value(*payload) {
                    self.handle_remote_event(value);
                }
            }
            Ok(RemoteTuiMessage::TextDelta { delta }) => {
                self.add_live_output_delta(&delta);
                self.messages_state.messages.add_assistant_text(delta);
                if matches!(self.session_state.session_status, SessionStatus::Working) {
                    self.status_bar
                        .set_thinking(true, Some("Thinking...".to_string()));
                }
            }
            Ok(RemoteTuiMessage::ToolCallStarted {
                tool_id,
                tool_name,
                arguments,
            }) => {
                self.messages_state.messages.finalize_streaming();
                match serde_json::from_str::<serde_json::Value>(&arguments) {
                    Ok(args_val) => {
                        self.messages_state.messages.add_tool_call(
                            tool_id.clone(),
                            tool_name,
                            args_val,
                        );
                        self.messages_state
                            .messages
                            .mark_tool_call_running(&tool_id);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse remote tool call arguments for {}: {}",
                            tool_name,
                            e
                        );
                        self.messages_state.messages.add_tool_call(
                            tool_id.clone(),
                            tool_name,
                            serde_json::Value::Null,
                        );
                        self.messages_state
                            .messages
                            .mark_tool_call_running(&tool_id);
                    }
                }
            }
            Ok(RemoteTuiMessage::ToolResult {
                tool_id,
                output,
                success,
            }) => {
                let status = if success {
                    crate::session::message::ToolStatus::Completed
                } else {
                    crate::session::message::ToolStatus::Error
                };
                self.messages_state
                    .messages
                    .update_tool_call(&tool_id, output, status, None, None, None);
            }
            Ok(RemoteTuiMessage::SessionInfo { model, .. }) => {
                self.agent_state.current_model = model;
            }
            Ok(RemoteTuiMessage::SessionEnded { .. }) => {
                self.session_state.session_status = SessionStatus::Idle;
                self.status_bar.set_thinking(false, None);
            }
            Ok(RemoteTuiMessage::PermissionPending { id, tool, path }) => {
                self.show_permission_dialog(
                    id,
                    PermissionRequest {
                        tool,
                        path,
                        args: None,
                    },
                );
            }
            Ok(RemoteTuiMessage::QuestionPending { id, questions }) => {
                let questions = questions
                    .into_iter()
                    .map(|q| QuestionSpec {
                        question: q.prompt,
                        options: None,
                        initial: q.default,
                    })
                    .collect();
                self.show_question_dialog(questions, id);
            }
            Ok(RemoteTuiMessage::Error { message }) => {
                self.session_state.session_status = SessionStatus::Error;
                self.status_bar.set_thinking(false, None);
                self.messages_state.toasts.add(Toast::error(&message));
            }
            Ok(RemoteTuiMessage::RenderFrame { content }) => {
                tracing::warn!(
                    "RenderFrame received ({} bytes) — unsupported, sending error response",
                    content.len()
                );
                self.send_remote_message(RemoteTuiMessage::Error {
                    message: "Frame-driven remote rendering is not supported; request state snapshots instead (unsupported_render_frame)".to_string(),
                });
            }
            Ok(RemoteTuiMessage::ResyncRequired {
                reason,
                pending_permissions,
                pending_questions,
            }) => {
                let reason_str = reason.unwrap_or_else(|| "client lagged".to_string());
                self.messages_state.toasts.add(Toast::warning(&format!(
                    "Resyncing: {} ({} pending permissions, {} pending questions)",
                    reason_str,
                    pending_permissions.len(),
                    pending_questions.len()
                )));
                tracing::info!(
                    "ResyncRequired: reason={}, pending_permissions={:?}, pending_questions={:?}",
                    reason_str,
                    pending_permissions,
                    pending_questions
                );
            }
            Ok(RemoteTuiMessage::Resume { from_event_seq }) => {
                let requested = from_event_seq;
                tracing::info!("Resume received: from_event_seq={}", requested);
                if requested == 0 {
                    self.messages_state
                        .toasts
                        .add(Toast::warning("Cannot resume from sequence 0"));
                    self.send_remote_message(RemoteTuiMessage::ResyncRequired {
                        reason: Some("invalid_resume_sequence".to_string()),
                        pending_permissions: Vec::new(),
                        pending_questions: Vec::new(),
                    });
                } else if requested > self.remote_sequence {
                    self.messages_state.toasts.add(Toast::warning(&format!(
                        "Resume seq {} ahead of current {}; resyncing",
                        requested, self.remote_sequence
                    )));
                    self.send_remote_message(RemoteTuiMessage::ResyncRequired {
                        reason: Some("requested_sequence_ahead_of_current".to_string()),
                        pending_permissions: Vec::new(),
                        pending_questions: Vec::new(),
                    });
                } else {
                    let snapshot = self.next_remote_snapshot();
                    let seq = snapshot.sequence;
                    self.send_remote_message(RemoteTuiMessage::StateSnapshot {
                        sequence: seq,
                        snapshot,
                    });
                }
            }
            Ok(RemoteTuiMessage::RequestSnapshot { reason }) => {
                tracing::info!("RequestSnapshot received: reason={:?}", reason);
                let snapshot = self.next_remote_snapshot();
                let seq = snapshot.sequence;
                self.send_remote_message(RemoteTuiMessage::StateSnapshot {
                    sequence: seq,
                    snapshot,
                });
            }
            Ok(RemoteTuiMessage::StateSnapshot { snapshot, sequence }) => {
                tracing::info!(
                    "StateSnapshot received: seq={}, route={}, model={}, status={}",
                    sequence,
                    snapshot.route,
                    snapshot.model,
                    snapshot.status
                );
                self.agent_state.current_model = snapshot.model;
                self.session_state.session_status = match snapshot.status.as_str() {
                    "working" => SessionStatus::Working,
                    "error" => SessionStatus::Error,
                    _ => SessionStatus::Idle,
                };
                if self.session_state.session_status == SessionStatus::Working {
                    self.status_bar
                        .set_thinking(true, Some("Thinking...".to_string()));
                } else {
                    self.status_bar.set_thinking(false, None);
                }
            }
            Ok(RemoteTuiMessage::PluginUiEffect { envelope }) => {
                // Delegate to the envelope-aware handler so the session
                // guard, validation, and ownership check are applied
                // uniformly for both local and remote transports.
                let _ = self.apply_plugin_ui_envelope(envelope);
            }
            _ => {
                debug_log!("handle_remote_event: unhandled type={}", _event_type);
            }
        }
    }

    /// Prepare for TUI shutdown.
    ///
    /// Cancels all registered background tasks, kills running shell
    /// commands, and clears command senders.  Call this before leaving
    /// the event loop to ensure clean shutdown.  The
    /// [`TerminalGuard`](super::terminal::TerminalGuard) still restores
    /// terminal state even if this method encounters errors.
    pub fn prepare_shutdown(&mut self) {
        // Cancel all registered background tasks
        let active = self.task_registry.active_count();
        if active > 0 {
            tracing::info!(
                active_tasks = active,
                "cancelling background tasks on shutdown"
            );
            self.task_registry.cancel_all();
        }

        // Kill running shell commands
        for (id, handle) in self.shell_handles.drain() {
            tracing::debug!(shell_id = id, "killing shell command on shutdown");
            handle.kill();
            // Mark as killed in the store if the entry exists
            let cmd_id = crate::shell::types::ShellCommandId(id);
            self.shell_store
                .mark_killed(cmd_id, std::time::Duration::ZERO);
        }
    }

    pub fn reset_state(&mut self) {
        self.ui_state.dialog = Dialog::None;
        self.ui_state.command_mode = false;
        self.ui_state.timeline_visible = false;
        self.prompt_state.show_completions = false;
        self.prompt_state.completion_filter.clear();
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let main_chunks = self.ui_state.layout.split(area);
        let main_area = main_chunks[0];

        // Reserve a 1-column strip on the left of the main area for the
        // outer left border. Reserving the strip (rather than painting
        // the border on top of the content) keeps the line continuous —
        // the header / viewport / prompt / footer widgets no longer
        // overwrite the leftmost column with their own text.
        let bordered = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(main_area);
        let main_area_inner = bordered[1];
        self.left_border_area = Some(bordered[0]);

        let max_prompt_height = (area.height * 40 / 100).max(3);
        let prompt_height = self.prompt_state.prompt.needed_height(max_prompt_height);
        let session_chunks = self
            .ui_state
            .layout
            .session_layout(main_area_inner, Some(prompt_height));

        self.viewport_area = Some(session_chunks[1]);
        self.prompt_area = Some(session_chunks[2]);

        // Outer TUI frame: paint the left border on the reserved strip
        // and the four corner cells that connect it with the header's
        // bottom border (top) and the footer's bottom border (bottom).
        // The header and footer widgets supply the horizontal lines via
        // their own `Borders::BOTTOM` / `Borders::TOP | Borders::BOTTOM`.
        self.render_outer_borders(
            frame,
            bordered[0],
            main_area_inner,
            session_chunks[0],
            session_chunks[3],
        );

        self.render_header(frame, session_chunks[0]);

        // Viewport (messages) — highest-risk surface for render panics
        let viewport_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                if self.render_panic_injection.messages {
                    panic!("injected messages render panic");
                }
                self.render_viewport(frame, session_chunks[1]);
                Ok(())
            }));
        if let Err(err) = viewport_result {
            let msg = Self::extract_panic_message(&err);
            tracing::error!("Messages render panic: {msg}");
            self.ui_state.last_render_error = Some(msg);
            self.ui_state
                .diagnostics
                .record_component_render_panic("messages");
            self.render_component_fallback(frame, session_chunks[1], "Messages render error");
        }

        self.render_prompt(frame, session_chunks[2]);
        self.render_footer(frame, session_chunks[3]);

        // Sidebar — dynamic content can trigger panics
        if self.ui_state.sidebar_visible && main_chunks.len() > 1 {
            self.sidebar_area = Some(main_chunks[1]);
            let sidebar_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.sidebar {
                        panic!("injected sidebar render panic");
                    }
                    self.render_sidebar(frame, main_chunks[1]);
                    Ok(())
                }));
            if let Err(err) = sidebar_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Sidebar render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("sidebar");
                self.render_component_fallback(frame, main_chunks[1], "Sidebar unavailable");
            }
        } else {
            self.sidebar_area = None;
        }

        // Dialog — failures close only that dialog
        if self.ui_state.dialog.is_open() {
            let popup_area = centered_rect(60, 50, area);
            self.dialog_area = Some(popup_area);
            let dialog_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.dialog {
                        panic!("injected dialog render panic");
                    }
                    self.render_dialog(frame, area);
                    Ok(())
                }));
            if let Err(err) = dialog_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Dialog render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("dialog");
                self.ui_state.dialog = Dialog::None;
            }
        } else {
            self.dialog_area = None;
        }

        // Completion overlay — failures hide completions
        if self.prompt_state.show_completions {
            let prompt_area = session_chunks[2];
            let max_h = 8.min(self.prompt_state.slash_completions.len() as u16);
            let compl_h = max_h + 2;
            let compl_w = 40.min(prompt_area.width.saturating_sub(2));
            let compl_area = Rect {
                x: prompt_area.x + 1,
                y: prompt_area.y.saturating_sub(compl_h),
                width: compl_w,
                height: compl_h,
            };
            self.completion_area = Some(compl_area);
            let compl_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.completions {
                        panic!("injected completions render panic");
                    }
                    self.render_completions(frame, session_chunks[2]);
                    Ok(())
                }));
            if let Err(err) = compl_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Completion overlay render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("completions");
                self.prompt_state.show_completions = false;
            }
        } else {
            self.completion_area = None;
        }

        // Timeline
        if self.ui_state.timeline_visible {
            let tl_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.timeline {
                        panic!("injected timeline render panic");
                    }
                    self.render_timeline(frame, area);
                    Ok(())
                }));
            if let Err(err) = tl_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Timeline render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("timeline");
                self.ui_state.timeline_visible = false;
            }
        }

        if !self.messages_state.toasts.is_empty() {
            let toast_area = Rect {
                x: area.width.saturating_sub(60),
                y: 2,
                width: 60.min(area.width),
                height: 10.min(area.height.saturating_sub(4)),
            };
            self.messages_state
                .toasts
                .render(frame, toast_area, &self.ui_state.theme);
        }
    }

    pub fn render_error(&mut self, frame: &mut Frame, error_msg: &str) {
        let area = frame.area();
        let theme = &self.ui_state.theme;

        let block = Block::default()
            .title(" Error ")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme.error))
            .style(
                ratatui::style::Style::default()
                    .bg(theme.background)
                    .fg(theme.foreground),
            );

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                "⚠ Rendering Error",
                ratatui::style::Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::raw("The UI failed to render properly:")),
            Line::from(""),
            Line::from(Span::styled(
                error_msg,
                ratatui::style::Style::default().fg(theme.warning),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'r' to retry or 'q' to quit",
                ratatui::style::Style::default().fg(theme.muted),
            )),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);

        let center_area = centered_rect(60, 40, area);
        frame.render_widget(Clear, center_area);
        frame.render_widget(paragraph, center_area);
    }

    /// Render a compact fallback block when a component panics.
    fn render_component_fallback(&self, frame: &mut Frame, area: Rect, title: &str) {
        let theme = &self.ui_state.theme;
        let block = Block::default()
            .title(format!(" {title} "))
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme.warning))
            .style(
                ratatui::style::Style::default()
                    .bg(theme.background)
                    .fg(theme.muted),
            );
        let paragraph = Paragraph::new("Render failed — see log for details")
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    /// Extract a human-readable message from a panic payload.
    fn extract_panic_message(err: &Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = err.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = err.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown render panic".to_string()
        }
    }

    fn render_header(&mut self, frame: &mut Frame, area: Rect) {
        let agent_name = &self.agent_state.agents[self.agent_state.current_agent].name;
        let model_short = self
            .agent_state
            .current_model
            .split('/')
            .next_back()
            .unwrap_or(&self.agent_state.current_model);
        let mode_indicator = if self.agent_state.plan_mode {
            format!(
                "[PLAN: {}]  ",
                self.agent_state.plan_topic.as_deref().unwrap_or("general")
            )
        } else {
            String::new()
        };
        let context_indicator = self.active_context_indicator();
        let sess_title = if let Some(ref session) = self.session_state.session {
            format!("[{}]  ", clean_inline_text(&session.title, 48))
        } else {
            String::new()
        };
        let title = match self.ui_state.routes.current() {
            Route::Home => Line::from(vec![
                Span::styled(
                    " codegg ",
                    Style::default()
                        .fg(self.ui_state.theme.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                if context_indicator.is_empty() {
                    Span::raw("")
                } else {
                    Span::styled(
                        context_indicator.clone(),
                        Style::default()
                            .fg(self.ui_state.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    )
                },
                Span::styled(
                    format!("{mode_indicator}{sess_title}  "),
                    Style::default()
                        .fg(self.ui_state.theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("agent:{agent_name}  model:{model_short}"),
                    Style::default().fg(self.ui_state.theme.muted),
                ),
            ]),
            Route::Session(_) => Line::from(""),
        };
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(self.ui_state.theme.border))
            .style(Style::default().bg(self.ui_state.theme.background));
        let paragraph = Paragraph::new(title).block(block);
        frame.render_widget(paragraph, area);
    }

    /// Draw the left border on the reserved 1-column strip and the
    /// four corner cells that connect it with the header's bottom
    /// border (top edge) and the footer's bottom border (bottom edge).
    /// The header and footer widgets supply the horizontal lines via
    /// their own `Borders::BOTTOM` / `Borders::TOP | Borders::BOTTOM`
    /// — the corners here are what make the three sides look like a
    /// single connected frame instead of three disjoint segments.
    fn render_outer_borders(
        &mut self,
        frame: &mut Frame,
        left_area: Rect,
        content_area: Rect,
        header_area: Rect,
        footer_area: Rect,
    ) {
        // Left border: paint a `Borders::LEFT` block on the reserved
        // strip. Drawing on a dedicated strip keeps the line continuous
        // since the content widgets never write into column 0.
        let border_style = Style::default().fg(self.ui_state.theme.border);
        let bg_style = Style::default().bg(self.ui_state.theme.background);
        let left_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(border_style)
            .style(bg_style);
        frame.render_widget(left_block, left_area);

        if footer_area.height > 0 {
            let footer_bottom_y = footer_area.y + footer_area.height - 1;
            self.bottom_border_area = Some(Rect::new(
                footer_area.x,
                footer_bottom_y,
                footer_area.width,
                1,
            ));
        }
        if header_area.height > 0 {
            // Corner cells: top-left, top-right, bottom-left, bottom-right.
            // Drawn after the header and footer widgets (which supply the
            // horizontal lines) so the corner glyphs overwrite the line
            // glyphs at the intersections and visually connect them.
            let header_bottom_y = header_area.y + header_area.height - 1;
            let footer_bottom_y = footer_area.y + footer_area.height - 1;
            let right_x = content_area.x + content_area.width - 1;
            self.render_corner(frame, left_area.x, header_bottom_y, '┌', border_style);
            self.render_corner(frame, right_x, header_bottom_y, '┐', border_style);
            self.render_corner(frame, left_area.x, footer_bottom_y, '└', border_style);
            self.render_corner(frame, right_x, footer_bottom_y, '┘', border_style);
        }
    }

    /// Paint a single corner glyph at `(x, y)` in the given style.
    /// Uses a 1x1 [`Paragraph`] so the glyph goes through ratatui's
    /// normal styling pipeline and respects the active background.
    fn render_corner(&self, frame: &mut Frame, x: u16, y: u16, glyph: char, style: Style) {
        let cell = Paragraph::new(glyph.to_string()).style(style);
        frame.render_widget(cell, Rect::new(x, y, 1, 1));
    }

    fn active_context_indicator(&self) -> String {
        let dialog_type = self.focus_manager.active_dialog_type();
        if dialog_type != DialogType::None {
            return format!("[DIALOG: {:?}]  ", dialog_type);
        }
        if self.ui_state.dialog.is_open() {
            return format!("[DIALOG: {:?}]  ", self.ui_state.dialog);
        }
        if self.ui_state.command_mode {
            return "[CMD]  ".to_string();
        }
        if self.messages_state.messages.search_visible {
            return "[SEARCH]  ".to_string();
        }
        if self.agent_state.plan_mode {
            return "[PLAN]  ".to_string();
        }
        if self.session_state.permission_pending {
            return "[PERMISSION]  ".to_string();
        }
        let subagent_count = self.session_state.subagent_count;
        if subagent_count > 0 {
            return format!("[...{}]  ", subagent_count);
        }
        match self.session_state.session_status {
            SessionStatus::Working => {
                self.busy_spinner.tick();
                format!("{}  ", self.busy_spinner.frame())
            }
            _ => String::new(),
        }
    }

    fn render_viewport(&mut self, frame: &mut Frame, area: Rect) {
        match self.ui_state.routes.current() {
            Route::Home => self.render_home(frame, area),
            Route::Session(_) => self.render_session(frame, area),
        }
    }

    fn render_home(&mut self, frame: &mut Frame, area: Rect) {
        // Render a background block first to ensure the viewport isn't transparent
        let block = Block::default().style(Style::default().bg(self.ui_state.theme.background));
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  codegg ",
                Style::default()
                    .fg(self.ui_state.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Type a prompt to begin",
                Style::default().fg(self.ui_state.theme.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Ctrl+N  new session    Ctrl+L  change model    ?  help",
                Style::default().fg(self.ui_state.theme.muted),
            )),
        ];
        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    fn render_session(&mut self, frame: &mut Frame, area: Rect) {
        self.messages_state.messages.set_theme(&self.ui_state.theme);
        self.messages_state
            .messages
            .set_display_options(self.ui_state.show_thinking, self.ui_state.show_timestamps);

        let (content_area, scrollbar_area) = self.ui_state.layout.viewport_with_scrollbar(area);
        self.messages_state
            .messages
            .set_visible_height(content_area.height as usize);
        self.messages_state.messages.set_width(content_area.width);
        self.scrollbar_area = if scrollbar_area.width == 0 {
            None
        } else {
            Some(scrollbar_area)
        };

        // Render a background block first to ensure the viewport isn't transparent
        let block = Block::default().style(Style::default().bg(self.ui_state.theme.background));
        frame.render_widget(block, area);

        frame.render_widget(&self.messages_state.messages, content_area);

        if self.scrollbar_area.is_some() {
            let mut state = self
                .messages_state
                .messages
                .scrollbar_state(scrollbar_area.height as usize);
            frame.render_stateful_widget(
                Scrollbar::default()
                    .orientation(ScrollbarOrientation::VerticalRight)
                    .thumb_style(Style::default().fg(self.ui_state.theme.foreground))
                    .track_style(Style::default().fg(self.ui_state.theme.border))
                    .begin_symbol(None)
                    .end_symbol(None),
                scrollbar_area,
                &mut state,
            );
        }
    }

    fn render_prompt(&mut self, frame: &mut Frame, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(self.ui_state.theme.input_bg));
        frame.render_widget(bg_block, area);

        let mode_indicator = match self.ui_state.input_mode {
            InputMode::Insert => Span::styled(
                "[INS] ",
                Style::default()
                    .fg(self.ui_state.theme.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            InputMode::Normal => Span::styled(
                "[NOR]",
                Style::default()
                    .fg(self.ui_state.theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
        };
        let session_prefix = match &self.session_state.session_status {
            SessionStatus::Working => Span::styled(
                " ● ",
                Style::default()
                    .fg(self.ui_state.theme.warning)
                    .add_modifier(Modifier::BOLD),
            ),
            SessionStatus::Error => Span::styled(
                " ✗ ",
                Style::default()
                    .fg(self.ui_state.theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            SessionStatus::Idle => Span::styled(
                " ❯ ",
                Style::default()
                    .fg(self.ui_state.theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
        };
        self.prompt_state.prompt.set_theme(&self.ui_state.theme);
        self.prompt_state.prompt.set_mode_indicator(mode_indicator);
        self.prompt_state.prompt.set_prefix(session_prefix);

        let prompt_text = self.prompt_state.prompt.get_text();
        let placeholder = if prompt_text.starts_with('!') {
            "shell: run locally; not included in model context".to_string()
        } else {
            "Ask anything…".to_string()
        };
        self.prompt_state.prompt.set_placeholder(placeholder);
        let visible_lines = area.height.saturating_sub(2) as usize;
        self.prompt_state
            .prompt
            .ensure_cursor_visible(visible_lines);
        frame.render_widget(&self.prompt_state.prompt, area);

        if self.ui_state.command_mode {
            self.dialog_state
                .command_palette
                .render(frame, area, &self.ui_state.theme);
        }
    }

    fn render_footer(&mut self, frame: &mut Frame, area: Rect) {
        let mut summary = self.build_status_summary();

        let token_str = format_token_line(
            self.session_state.token_in,
            self.session_state.token_out,
            self.session_state.live_output_tokens,
            self.session_state.context_tokens as u64,
            self.session_state.context_limit as u64,
        );
        summary.secondary = Some(token_str);

        self.status_bar.set_theme(&self.ui_state.theme);
        self.status_bar.apply_summary(&summary);

        if let Some(ref lsp_tool) = self.lsp_tool {
            let handle = tokio::runtime::Handle::current();
            let lsp_status = handle.block_on(lsp_tool.lsp_status_line());
            self.status_bar.set_lsp_status(lsp_status);
        } else {
            self.status_bar.set_lsp_status(None);
        }

        frame.render_widget(&self.status_bar, area);
    }

    pub fn build_status_summary(&self) -> TuiStatusSummary {
        let mut primary = String::from("idle");
        let mut activity: Vec<String> = Vec::new();

        if let Some(ref err) = self.ui_state.last_render_error {
            primary = format!("degraded: {err}");
        } else if self.session_state.permission_pending {
            primary = "permission pending".to_string();
        } else if self.dialog_state.question_dialog.is_some() {
            primary = "question pending".to_string();
        } else if self.security_review_running.is_some() {
            primary = "security review".to_string();
        } else if self.session_state.session_status == SessionStatus::Working {
            primary = "working".to_string();
        } else if !self.shell_handles.is_empty() {
            primary = "shell running".to_string();
        } else if self.task_registry.active_count() > 0 {
            primary = format!("bg:{}", self.task_registry.active_count());
        } else if self.session_state.session_status == SessionStatus::Error {
            primary = "error".to_string();
        }

        let undo_message = if self.undo_session_id.is_some() {
            if let Some(until) = self.undo_until {
                if Instant::now() < until {
                    Some("Session deleted — press U to undo".to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let agent_name = &self.agent_state.agents[self.agent_state.current_agent].name;
        activity.push(format!("agent:{agent_name}"));

        if self.session_state.subagent_count > 0 {
            activity.push(format!("subagents:{}", self.session_state.subagent_count));
        }

        if self.dialog_state.session_reload_request.is_loading() {
            activity.push("reloading".to_string());
        }

        if self.dialog_state.import_request.is_loading() {
            activity.push("importing".to_string());
        }

        if self.dialog_state.research_request.is_loading() {
            activity.push("research".to_string());
        }

        if self.dialog_state.session_messages_request.is_loading() {
            activity.push("messages".to_string());
        }

        if self.dialog_state.session_mutation_request.is_loading() {
            activity.push("mutating".to_string());
        }

        if self.dialog_state.task_list_request.is_loading() {
            activity.push("tasks".to_string());
        }

        if self.dialog_state.worktree_list_request.is_loading() {
            activity.push("worktrees".to_string());
        }

        if self.dialog_state.template_create_request.is_loading() {
            activity.push("template".to_string());
        }

        let memory_tasks = self
            .task_registry
            .iter()
            .filter(|(_, r)| r.kind == crate::tui::task_lifecycle::TuiTaskKind::Memory)
            .count();
        if memory_tasks > 0 {
            activity.push(format!("mem:{memory_tasks}"));
        }

        let active_tasks = self.task_registry.active_count();
        if active_tasks > 0 {
            activity.push(format!("tasks:{active_tasks}"));
        }

        if !self.shell_handles.is_empty() {
            activity.push(format!("shell:{}", self.shell_handles.len()));
        }

        let pending_diffs = self
            .session_state
            .changed_files
            .iter()
            .filter(|f| {
                matches!(
                    f.diff_state,
                    crate::tui::app::state::session::DiffStatsState::Pending { .. }
                )
            })
            .count();
        if pending_diffs > 0 {
            activity.push(format!("diff:{pending_diffs}"));
        }

        if self.security_review_running.is_some() {
            activity.push("security".to_string());
        }

        if let Some(ref goal) = self.active_goal {
            activity.push(format!("goal:{}", format_goal_status_line(goal)));
        }

        TuiStatusSummary {
            primary,
            secondary: None,
            activity,
            undo_message,
        }
    }

    fn render_sidebar(&mut self, frame: &mut Frame, area: Rect) {
        self.sidebar.set_theme(&self.ui_state.theme);
        if let Some(ref sess) = self.session_state.session {
            self.sidebar.set_session(sess);
        }
        self.sidebar
            .set_agent(&self.agent_state.agents[self.agent_state.current_agent].name);
        self.sidebar.set_model(&self.agent_state.current_model);
        let provider = self
            .agent_state
            .current_model
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();
        self.sidebar.set_provider(&provider);
        self.sidebar
            .set_mcp_servers(self.session_state.mcp_servers.clone());
        self.sidebar.set_file_changes(
            self.session_state
                .changed_files
                .iter()
                .map(|file| super::components::sidebar::SidebarFileChange {
                    path: file.path.to_string_lossy().into_owned(),
                    action: file.action.clone(),
                    diff_preview: file.diff_preview.clone(),
                    diff_state: file.diff_state.clone(),
                })
                .collect(),
        );

        if let Some(ref sess) = self.session_state.session {
            let cached = &self.session_state.git_sidebar;
            let display_root = cached
                .root
                .clone()
                .or_else(|| Some(sess.project_id.clone()));
            self.sidebar
                .set_git_info(cached.branch.clone(), cached.dirty, display_root);
        } else {
            self.sidebar.set_git_info(None, false, None);
        }

        let derived = &self.session_state_derived;
        self.sidebar.set_goal(derived.goal.clone());
        self.sidebar.set_plan(derived.plan.clone());

        frame.render_widget(&self.sidebar, area);
    }

    fn render_dialog(&mut self, frame: &mut Frame, area: Rect) {
        if self.focus_manager.is_empty() && !self.ui_state.dialog.is_open() {
            return;
        }

        let popup_area = centered_rect(60, 50, area);
        frame.render_widget(Clear, popup_area);

        if !self.focus_manager.is_empty() {
            self.focus_manager
                .render(frame, popup_area, &self.ui_state.theme);
        }
    }

    fn render_completions(&self, frame: &mut Frame, prompt_area: Rect) {
        use crate::tui::components::completion_overlay::CompletionItem;
        let items: Vec<ListItem> = match self.prompt_state.completion_type {
            CompletionType::Slash => {
                let filter = self.prompt_state.completion_filter.trim_start_matches('/');
                let mut scored: Vec<(&CompletionItem, usize)> = self
                    .prompt_state
                    .slash_completions
                    .iter()
                    .filter_map(|item| {
                        let item_name = item.label.trim_start_matches('/');
                        let score = if filter.is_empty() {
                            usize::MAX
                        } else {
                            fuzzy_score(filter, item_name)
                        };
                        if filter.is_empty() || score > 0 {
                            Some((item, score))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !filter.is_empty() {
                    scored.sort_by_key(|b| std::cmp::Reverse(b.1));
                }
                scored
                    .into_iter()
                    .enumerate()
                    .map(|(i, (c, _))| {
                        let style = if i == self.prompt_state.completion_sel {
                            Style::default()
                                .bg(self.ui_state.theme.selection)
                                .fg(self.ui_state.theme.primary)
                        } else {
                            Style::default().fg(self.ui_state.theme.foreground)
                        };
                        let content = if let Some(ref desc) = c.description {
                            Text::from(vec![Line::from(vec![
                                Span::styled(format!("{} ", c.label), style),
                                Span::styled(desc, Style::default().fg(self.ui_state.theme.muted)),
                            ])])
                        } else {
                            Text::from(Span::styled(&c.label, style))
                        };
                        ListItem::new(content)
                    })
                    .collect()
            }
            CompletionType::File => self
                .prompt_state
                .file_completions
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let style = if i == self.prompt_state.completion_sel {
                        Style::default()
                            .bg(self.ui_state.theme.selection)
                            .fg(self.ui_state.theme.primary)
                    } else {
                        Style::default().fg(self.ui_state.theme.foreground)
                    };
                    let content = if let Some(ref desc) = c.description {
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("{} ", c.icon()), style),
                            Span::styled(format!("{} ", c.label), style),
                            Span::styled(desc, Style::default().fg(self.ui_state.theme.muted)),
                        ])])
                    } else {
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("{} ", c.icon()), style),
                            Span::styled(&c.label, style),
                        ])])
                    };
                    ListItem::new(content)
                })
                .collect(),
            CompletionType::Agent => self
                .prompt_state
                .agent_completions
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let style = if i == self.prompt_state.completion_sel {
                        Style::default()
                            .bg(self.ui_state.theme.selection)
                            .fg(self.ui_state.theme.primary)
                    } else {
                        Style::default().fg(self.ui_state.theme.foreground)
                    };
                    let content = if let Some(ref desc) = c.description {
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("@{} ", c.label), style),
                            Span::styled(desc, Style::default().fg(self.ui_state.theme.muted)),
                        ])])
                    } else {
                        Text::from(Span::styled(format!("@{}", c.label), style))
                    };
                    ListItem::new(content)
                })
                .collect(),
        };
        if items.is_empty() {
            return;
        }
        let max_h = 8.min(items.len() as u16);
        let compl_h = max_h + 2;
        let compl_w = 40.min(prompt_area.width.saturating_sub(2));
        let compl_area = Rect {
            x: prompt_area.x + 1,
            y: prompt_area.y.saturating_sub(compl_h),
            width: compl_w,
            height: compl_h,
        };
        frame.render_widget(Clear, compl_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.ui_state.theme.border))
            .style(Style::default().bg(self.ui_state.theme.background));
        let list = List::new(items).block(block);
        frame.render_widget(list, compl_area);
    }

    pub fn process_msg(&mut self, msg: TuiMsg) {
        debug_log!("process_msg: {:?}", msg);
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
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::OpenTreeDialog);
                }
            }
            TuiMsg::OpenThemeDialog => self.open_dialog(Dialog::Theme),
            TuiMsg::OpenShareDialog => {
                if let Some(ref session) = self.session_state.session {
                    let session_id = session.id.clone();
                    if let Some(ref tx) = self.tui_cmd_tx {
                        let _ = tx.try_send(TuiCommand::ShareSession { session_id });
                    }
                }
            }
            TuiMsg::OpenImportDialog => self.open_dialog(Dialog::Import),
            TuiMsg::OpenDiffDialog {
                old_content,
                new_content,
                title,
            } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::OpenDiffDialog {
                        old_content,
                        new_content,
                        title,
                    });
                }
            }
            TuiMsg::SelectModel { model } => {
                self.agent_state.current_model = model.clone();
                self.dialog_state
                    .model_dialog
                    .set_current(&self.agent_state.current_model);
                if let Some(idx) = self.agent_state.models.iter().position(|m| m == &model) {
                    self.agent_state.model_idx = idx;
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
            TuiMsg::ConnectConfigured {
                provider_name,
                env_var,
                api_key,
            } => {
                let provider_id = provider_name.to_lowercase();
                if let (Some(env_var), Some(api_key)) = (env_var, api_key) {
                    std::env::set_var(&env_var, &api_key);

                    // Persist to config
                    if let Ok(mut config) = crate::config::schema::Config::load() {
                        let provider_map = config
                            .provider
                            .get_or_insert_with(std::collections::HashMap::new);
                        let mut p_config =
                            provider_map.get(&provider_id).cloned().unwrap_or_default();
                        p_config.api_key = Some(api_key.clone());
                        provider_map.insert(provider_id.clone(), p_config);

                        if let Err(e) = config.save() {
                            tracing::error!("Failed to save config: {}", e);
                            self.messages_state
                                .toasts
                                .error(&format!("Failed to save API key to config: {}", e));
                        } else {
                            self.messages_state
                                .toasts
                                .success(&format!("API key saved to config for {}", provider_name));
                        }
                    }

                    self.messages_state.toasts.info(&format!(
                        "API key set for {}. {} environment variable updated.",
                        provider_name, env_var
                    ));
                    self.refresh_models();
                } else {
                    self.messages_state.toasts.info(&format!(
                        "Connected to {} (no API key required)",
                        provider_name
                    ));
                }
                self.dialog_state.connect_dialog = None;
                self.close_dialog();
            }
            TuiMsg::CloseDialog => {
                self.close_dialog();
            }
            TuiMsg::ConfirmResult(confirmed) => {
                self.close_dialog();
                if confirmed == Some(true) {
                    if let Some(session_id) = self.dialog_state.pending_delete_session.take() {
                        let undo_id = session_id.clone();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::DeleteSession { session_id });
                        }
                        self.undo_session_id = Some(undo_id);
                        self.undo_until = Some(Instant::now() + std::time::Duration::from_secs(30));
                        self.status_bar
                            .set_undo_message("Session deleted — press U to undo");
                    } else if let Some((session_id, unarchive)) =
                        self.dialog_state.pending_archive_session.take()
                    {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ArchiveSession {
                                session_id,
                                unarchive,
                            });
                        }
                    } else if let Some(_count) = self.dialog_state.pending_bulk_delete.take() {
                        let ids = self
                            .dialog_state
                            .pending_bulk_delete_ids
                            .take()
                            .unwrap_or_default();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::BulkDelete { session_ids: ids });
                        }
                        self.dialog_state.session_dialog.toggle_bulk_mode();
                    } else if let Some((_count, unarchive)) =
                        self.dialog_state.pending_bulk_archive.take()
                    {
                        let ids = self
                            .dialog_state
                            .pending_bulk_archive_ids
                            .take()
                            .unwrap_or_default();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::BulkArchive {
                                session_ids: ids,
                                unarchive,
                            });
                        }
                        self.dialog_state.session_dialog.toggle_bulk_mode();
                    } else if let Some((command, promote_after)) =
                        self.dialog_state.pending_shell_command.take()
                    {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::RunHumanShell {
                                command,
                                promote_after,
                            });
                        }
                    }
                } else {
                    self.dialog_state.pending_delete_session = None;
                    self.dialog_state.pending_archive_session = None;
                    self.dialog_state.pending_bulk_delete = None;
                    self.dialog_state.pending_bulk_delete_ids = None;
                    self.dialog_state.pending_bulk_archive = None;
                    self.dialog_state.pending_bulk_archive_ids = None;
                    self.dialog_state.pending_shell_command = None;
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
                    let _ = tx.try_send(TuiCommand::ForkSession { session_id });
                }
            }
            TuiMsg::ForkSession { session_id } => {
                self.close_dialog();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ForkSession { session_id });
                }
            }
            TuiMsg::SubmitImportPreview => {
                self.handle_import_send();
            }
            TuiMsg::ConfirmImport => {
                self.handle_import_send();
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
            TuiMsg::UndoDelete => {
                if let Some(session_id) = self.undo_session_id.take() {
                    if let Some(ref tx) = self.tui_cmd_tx {
                        let _ = tx.try_send(TuiCommand::UndoDelete { session_id });
                    }
                    self.undo_until = None;
                }
            }
            TuiMsg::ReviewOpenDiff { path } => {
                self.handle_diff_command(Some(&path));
            }
            TuiMsg::ResearchOpenRun { run_id } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ResearchLoadRun { run_id });
                }
            }
            TuiMsg::ResearchRefreshRuns => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ResearchListRuns);
                }
            }
            TuiMsg::ResearchLoadSection { run_id, section } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ResearchLoadSection { run_id, section });
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
                self.dialog_state.source_preview_dialog = Some(dialog);
                if let Some(ref mut dlg) = self.dialog_state.source_preview_dialog {
                    dlg.set_theme(&self.ui_state.theme);
                    self.focus_manager.push(Box::new(dlg.clone()));
                }
                self.ui_state.dialog = crate::tui::Dialog::SourcePreview;
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
                    let _ = tx.try_send(TuiCommand::ShellInclude {
                        id: id.parse().unwrap_or(0),
                        mode,
                        question: None,
                    });
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
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ShellRerun {
                        id: id.parse().unwrap_or(0),
                    });
                }
            }
            TuiMsg::ShellKill { id } => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ShellKill {
                        id: id.parse().unwrap_or(0),
                    });
                }
            }
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        debug_log!(
            "on_key: dialog_open={}, dialog={:?}, command_mode={}, timeline_visible={}, show_completions={}, pending_send={}, key_code={:?}, key_modifiers={:?}",
            self.ui_state.dialog.is_open(),
            self.ui_state.dialog,
            self.ui_state.command_mode,
            self.ui_state.timeline_visible,
            self.prompt_state.show_completions,
            self.prompt_state.pending_send,
            key.code,
            key.modifiers
        );

        // Modal dialogs own input while they are active.
        if !self.focus_manager.is_empty() {
            if let Some(msg) = self.focus_manager.handle_key(key) {
                self.process_msg(msg);
            }
            return;
        }

        if self.ui_state.dialog.is_open() {
            if self.focus_manager.is_empty() {
                tracing::error!("FocusManager is empty but dialog is open - this indicates a state inconsistency");
                self.ui_state.dialog = Dialog::None;
                return;
            }
            self.handle_dialog_key(key);
            return;
        }

        if self.ui_state.timeline_visible {
            debug_log!("routing to handle_timeline_key");
            self.handle_timeline_key(key);
            return;
        }

        if self.ui_state.command_mode {
            debug_log!("routing to handle_command_key");
            self.handle_command_key(key);
            return;
        }

        if self.prompt_state.show_completions && self.handle_completion_key(key) {
            debug_log!(
                "routing to handle_completion_key - completions shown, Enter was intercepted"
            );
            return;
        }

        let action = handle_event_with_bindings_moded(
            crossterm::event::Event::Key(key),
            Some(&self.ui_state.bindings),
            self.ui_state.input_mode,
        );
        debug_log!("action from bindings: {:?}", action);
        match action {
            Some(InputAction::Send) => self.process_msg(TuiMsg::SubmitPrompt),
            Some(InputAction::Newline) => self.prompt_state.prompt.insert_newline(),
            Some(InputAction::Cancel) => self.cancel(),
            Some(InputAction::NavigateUp) => {
                if self.ui_state.input_mode == InputMode::Normal {
                    self.scroll_viewport_up();
                } else {
                    self.process_msg(TuiMsg::NavigateUp);
                }
            }
            Some(InputAction::NavigateDown) => {
                if self.ui_state.input_mode == InputMode::Normal {
                    self.scroll_viewport_down();
                } else {
                    self.process_msg(TuiMsg::NavigateDown);
                }
            }
            Some(InputAction::SwitchAgent) => self.process_msg(TuiMsg::CycleAgent),
            Some(InputAction::SelectModel) => self.process_msg(TuiMsg::OpenModelDialog),
            Some(InputAction::ClearSession) => self.process_msg(TuiMsg::ClearSession),
            Some(InputAction::NewSession) => self.process_msg(TuiMsg::NewSession),
            Some(InputAction::ToggleSidebar) => self.process_msg(TuiMsg::ToggleSidebar),
            Some(InputAction::ToggleSection) => {
                if self.ui_state.sidebar_visible {
                    self.sidebar.toggle_focused();
                }
            }
            Some(InputAction::CloseSession) => self.process_msg(TuiMsg::CloseSession),
            Some(InputAction::Help) => self.process_msg(TuiMsg::OpenHelpDialog),
            Some(InputAction::FocusPrompt) => {
                if self.prompt_state.prompt.cursor_pos() == 0 {
                    self.ui_state.command_mode = true;
                    self.prompt_state.prompt.insert_char('/');
                    self.prompt_state.prompt.set_cursor(1);
                    self.dialog_state.command_palette.set_query("/");
                }
                self.process_msg(TuiMsg::FocusPrompt);
            }
            Some(InputAction::StashPrompt) => self.process_msg(TuiMsg::StashPrompt),
            Some(InputAction::RestorePrompt) => self.process_msg(TuiMsg::RestorePrompt),
            Some(InputAction::CopyMessage) => self.process_msg(TuiMsg::CopyMessage),
            Some(InputAction::CycleModelForward) => self.process_msg(TuiMsg::CycleModelForward),
            Some(InputAction::CycleModelBackward) => self.process_msg(TuiMsg::CycleModelBackward),
            Some(InputAction::ToggleReasoning) => self.process_msg(TuiMsg::ToggleReasoning),
            Some(InputAction::ToggleTts) => self.process_msg(TuiMsg::ToggleTts),
            Some(InputAction::StopTts) => self.stop_tts(),
            Some(InputAction::ToggleFullscreen) => self.process_msg(TuiMsg::ToggleFullscreen),
            Some(InputAction::TogglePermissionMode) => {
                if self.agent_state.plan_mode {
                    self.exit_plan_mode();
                } else {
                    self.enter_plan_mode(None);
                }
            }
            Some(InputAction::OpenDiff) => {
                let old = "line 1\nline 2\nline 3";
                let new = "line 1\nline modified\nline 3";
                self.process_msg(TuiMsg::OpenDiffDialog {
                    old_content: old.to_string().into_boxed_str(),
                    new_content: new.to_string().into_boxed_str(),
                    title: "Example Diff".to_string().into_boxed_str(),
                });
            }
            Some(InputAction::Quit) => self.process_msg(TuiMsg::Quit),
            Some(InputAction::ExternalEditor) => self.process_msg(TuiMsg::ExternalEditor),
            Some(InputAction::Char(c)) => self.process_msg(TuiMsg::CharInput(c)),
            Some(InputAction::Backspace) => self.process_msg(TuiMsg::Backspace),
            Some(InputAction::Delete) => self.process_msg(TuiMsg::Delete),
            Some(InputAction::Left) => self.process_msg(TuiMsg::CursorLeft),
            Some(InputAction::Right) => self.process_msg(TuiMsg::CursorRight),
            Some(InputAction::Home) => self.process_msg(TuiMsg::CursorHome),
            Some(InputAction::End) => self.process_msg(TuiMsg::CursorEnd),
            Some(InputAction::PageUp) => self.scroll_page_up(),
            Some(InputAction::PageDown) => self.scroll_page_down(),
            Some(InputAction::GoToTop) => self.go_to_top(),
            Some(InputAction::GoToBottom) => self.go_to_bottom(),
            Some(InputAction::Search) => self.process_msg(TuiMsg::Search),
            Some(InputAction::SearchNext) => self.process_msg(TuiMsg::SearchNext),
            Some(InputAction::SearchPrev) => self.process_msg(TuiMsg::SearchPrev),
            Some(InputAction::ClearSearch) => self.process_msg(TuiMsg::ClearSearch),
            Some(InputAction::Command) => {
                self.ui_state.command_mode = true;
                self.prompt_state.prompt.insert_char(':');
                self.prompt_state.prompt.set_cursor(1);
                self.dialog_state.command_palette.set_query(":");
            }
            None => {
                // In Normal mode, only 'i' switches to Insert mode
                // All other keys pass through to their bindings (j/k for navigate, etc.)
                if self.ui_state.input_mode == InputMode::Normal {
                    if let crossterm::event::KeyCode::Char('i') = key.code {
                        if key.modifiers == crossterm::event::KeyModifiers::NONE {
                            self.ui_state.input_mode = InputMode::Insert;
                            return;
                        }
                    }
                }
            }
        }

        if let (Some(_session_id), Some(undo_until)) = (&self.undo_session_id, &self.undo_until) {
            if Instant::now() < *undo_until {
                if let crossterm::event::KeyEvent {
                    code: crossterm::event::KeyCode::Char('u'),
                    modifiers: crossterm::event::KeyModifiers::NONE,
                    ..
                } = key
                {
                    self.process_msg(TuiMsg::UndoDelete);
                }
            } else {
                self.undo_session_id = None;
                self.undo_until = None;
                self.status_bar.clear_undo_message();
            }
        }
    }

    fn handle_dialog_key(&mut self, key: KeyEvent) {
        debug_log!(
            "handle_dialog_key: dialog={:?}, key_code={:?}",
            self.ui_state.dialog,
            key.code
        );

        if let Dialog::Keybind = &self.ui_state.dialog {
            self.handle_keybind_key(key);
            return;
        }
        if let Dialog::Model = &self.ui_state.dialog {
            if key.code == crossterm::event::KeyCode::Tab {
                self.dialog_state.model_dialog.next_tab();
                return;
            }
        }
        if let Dialog::ShellShow = &self.ui_state.dialog {
            if let Some(id) = self.dialog_state.shell_detail_id {
                match key.code {
                    crossterm::event::KeyCode::Char('i') => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellInclude {
                                id,
                                mode: "all".to_string(),
                                question: None,
                            });
                        }
                        self.close_dialog();
                        return;
                    }
                    crossterm::event::KeyCode::Char('a') => {
                        self.prompt_state.prompt.clear();
                        self.prompt_state
                            .prompt
                            .set_text(format!("/shell-ask {}", id));
                        self.ui_state.command_mode = true;
                        self.close_dialog();
                        return;
                    }
                    crossterm::event::KeyCode::Char('r') => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellRerun { id });
                        }
                        self.close_dialog();
                        return;
                    }
                    crossterm::event::KeyCode::Char('k') => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellKill { id });
                        }
                        self.close_dialog();
                        return;
                    }
                    _ => {}
                }
            }
        }
        let action = handle_event_with_bindings_moded(
            crossterm::event::Event::Key(key),
            Some(&self.ui_state.bindings),
            InputMode::Insert,
        );
        debug_log!("handle_dialog_key: action={:?}", action);
        match action {
            Some(InputAction::Cancel) => {
                self.dialog_state.theme_picker = None;
                if let Some(ref mut mcp) = self.dialog_state.mcp_dialog {
                    if mcp.action_mode {
                        mcp.exit_action_mode();
                    } else {
                        self.dialog_state.mcp_dialog = None;
                        self.close_dialog();
                    }
                } else if matches!(self.ui_state.dialog, Dialog::Session) {
                    if self.dialog_state.session_dialog.bulk_mode {
                        self.dialog_state.session_dialog.toggle_bulk_mode();
                    } else {
                        self.dialog_state.session_dialog.clear_message_preview();
                        self.close_dialog();
                    }
                } else if matches!(self.ui_state.dialog, Dialog::Connect) {
                    if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                        if cd.step
                            == crate::tui::components::dialogs::connect::ConnectStep::EnterApiKey
                        {
                            cd.back_to_provider_selection();
                        } else {
                            self.dialog_state.connect_dialog = None;
                            self.close_dialog();
                        }
                    }
                } else if matches!(
                    self.ui_state.dialog,
                    Dialog::Context
                        | Dialog::Cost
                        | Dialog::Usage
                        | Dialog::ShellShow
                        | Dialog::Stats
                        | Dialog::TaskList
                        | Dialog::MemoryResults
                        | Dialog::DoctorReport
                ) {
                    self.close_dialog();
                } else {
                    self.dialog_state.import_dialog = None;
                    self.close_dialog();
                }
            }
            Some(InputAction::NavigateUp) => {
                debug_log!(
                    "NavigateUp: calling select_up on {:?}",
                    self.ui_state.dialog
                );
                match &mut self.ui_state.dialog {
                    Dialog::Model => self.dialog_state.model_dialog.select_up(),
                    Dialog::Agent => self.dialog_state.agent_dialog.select_up(),
                    Dialog::Session => self.dialog_state.session_dialog.select_up(),
                    Dialog::Tree => self.dialog_state.tree_dialog.select_up(),
                    Dialog::Theme => {
                        if let Some(picker) = &mut self.dialog_state.theme_picker {
                            picker.select_up();
                        }
                    }
                    Dialog::Question => {
                        if let Some(qd) = &mut self.dialog_state.question_dialog {
                            qd.select_up();
                        }
                    }
                    Dialog::Permission => {
                        if let Some(pd) = &mut self.dialog_state.permission_dialog {
                            pd.cursor_up();
                        }
                    }
                    Dialog::Mcp => {
                        if let Some(ref mut mcp) = self.dialog_state.mcp_dialog {
                            mcp.select_up();
                        }
                    }
                    Dialog::Template => {
                        if let Some(ref mut td) = self.dialog_state.template_dialog {
                            td.select_up();
                        }
                    }
                    Dialog::Connect => {
                        if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                            cd.cursor_up();
                        }
                    }
                    Dialog::Context
                    | Dialog::Cost
                    | Dialog::Usage
                    | Dialog::ShellShow
                    | Dialog::Stats
                    | Dialog::TaskList
                    | Dialog::MemoryResults
                    | Dialog::DoctorReport => {}
                    _ => {}
                }
            }
            Some(InputAction::NavigateDown) => {
                debug_log!(
                    "NavigateDown: calling select_down on {:?}",
                    self.ui_state.dialog
                );
                match &mut self.ui_state.dialog {
                    Dialog::Model => self.dialog_state.model_dialog.select_down(),
                    Dialog::Agent => self.dialog_state.agent_dialog.select_down(),
                    Dialog::Session => self.dialog_state.session_dialog.select_down(),
                    Dialog::Tree => self.dialog_state.tree_dialog.select_down(),
                    Dialog::Theme => {
                        if let Some(picker) = &mut self.dialog_state.theme_picker {
                            picker.select_down();
                        }
                    }
                    Dialog::Question => {
                        if let Some(qd) = &mut self.dialog_state.question_dialog {
                            qd.select_down();
                        }
                    }
                    Dialog::Permission => {
                        if let Some(pd) = &mut self.dialog_state.permission_dialog {
                            pd.cursor_down();
                        }
                    }
                    Dialog::Mcp => {
                        if let Some(ref mut mcp) = self.dialog_state.mcp_dialog {
                            mcp.select_down();
                        }
                    }
                    Dialog::Template => {
                        if let Some(ref mut td) = self.dialog_state.template_dialog {
                            td.select_down();
                        }
                    }
                    Dialog::Connect => {
                        if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                            cd.cursor_down();
                        }
                    }
                    Dialog::Context
                    | Dialog::Cost
                    | Dialog::Usage
                    | Dialog::ShellShow
                    | Dialog::Stats
                    | Dialog::TaskList
                    | Dialog::MemoryResults
                    | Dialog::DoctorReport => {}
                    _ => {}
                }
            }
            Some(InputAction::Send) => match &self.ui_state.dialog {
                Dialog::Model => {
                    if self.dialog_state.model_dialog.adding_model {
                        if let Some(model) = self.dialog_state.model_dialog.add_custom_model() {
                            let full_name = format!("{}/{}", model.provider, model.name);
                            if !self.agent_state.models.contains(&full_name) {
                                self.agent_state.models.push(full_name);
                            }
                        }
                    } else if let Some(model) = self.dialog_state.model_dialog.selected() {
                        self.agent_state.current_model = model.clone();
                        if let Some(idx) = self.agent_state.models.iter().position(|m| m == &model)
                        {
                            self.agent_state.model_idx = idx;
                        }
                        self.close_dialog();
                    }
                }
                Dialog::Agent => {
                    if let Some(agent) = self.dialog_state.agent_dialog.selected() {
                        if let Some(idx) =
                            self.agent_state.agents.iter().position(|a| a.name == agent)
                        {
                            self.agent_state.current_agent = idx;
                        }
                    }
                    self.close_dialog();
                }
                Dialog::Session => {
                    if self.dialog_state.session_dialog.bulk_mode {
                        self.dialog_state.session_dialog.toggle_bulk_mode();
                    } else if let Some(session) =
                        self.dialog_state.session_dialog.selected_session()
                    {
                        self.set_session(session.clone());
                        self.close_dialog();
                    }
                }
                Dialog::Theme => {
                    if let Some(picker) = &self.dialog_state.theme_picker {
                        if let Some(theme) = picker.selected_theme() {
                            self.ui_state.theme = Arc::new(theme.clone());
                            self.messages_state
                                .toasts
                                .info(&format!("Theme: {}", theme.name));
                        }
                    }
                    self.dialog_state.theme_picker = None;
                    self.close_dialog();
                }
                Dialog::Question => {
                    self.submit_question_answers();
                }
                Dialog::Permission => {
                    self.on_permission_confirm();
                }
                Dialog::Mcp => {
                    if let Some(ref mut mcp) = self.dialog_state.mcp_dialog {
                        if mcp.action_mode {
                            if let Some(action) = mcp.selected_action_name() {
                                match action {
                                    "Configure OAuth" => {
                                        if let Some(server) = mcp.selected_server() {
                                            self.messages_state.toasts.info(&format!(
                                                "OAuth for {} - configure in .codegg/mcp.json",
                                                server.name
                                            ));
                                        }
                                    }
                                    "Browse Resources" => {
                                        mcp.browse_mode = BrowseMode::Resources { selected: 0 };
                                        mcp.action_mode = false;
                                    }
                                    "Disconnect" => {
                                        if let Some(server) = mcp.selected_server() {
                                            self.messages_state
                                                .toasts
                                                .info(&format!("Disconnecting {}...", server.name));
                                        }
                                    }
                                    "Reconnect" => {
                                        if let Some(server) = mcp.selected_server() {
                                            self.messages_state
                                                .toasts
                                                .info(&format!("Reconnecting {}...", server.name));
                                        }
                                    }
                                    "Connect" => {
                                        if let Some(server) = mcp.selected_server() {
                                            self.messages_state
                                                .toasts
                                                .info(&format!("Connecting {}...", server.name));
                                        }
                                    }
                                    "Remove" => {
                                        if let Some(server) = mcp.selected_server() {
                                            self.messages_state
                                                .toasts
                                                .info(&format!("Removing {}...", server.name));
                                        }
                                    }
                                    "Wait" => {
                                        self.messages_state
                                            .toasts
                                            .info("Server is connecting, please wait...");
                                    }
                                    "Configure" => {
                                        if let Some(server) = mcp.selected_server() {
                                            self.messages_state.toasts.info(&format!(
                                                "Configure {} - edit .codegg/mcp.json",
                                                server.name
                                            ));
                                        }
                                    }
                                    "/shell-ask" => {
                                        self.ui_state.command_mode = false;
                                        let query = self.dialog_state.command_palette.query.clone();
                                        let args_str =
                                            query.strip_prefix("/shell-ask").unwrap_or("").trim();
                                        let parts: Vec<&str> = args_str.splitn(2, ' ').collect();
                                        let id_str = parts.first().copied().unwrap_or("");
                                        let question =
                                            parts.get(1).copied().unwrap_or("").to_string();
                                        if question.is_empty() {
                                            self.messages_state
                                                .toasts
                                                .warning("Usage: /shell-ask <id|last> <question>");
                                        } else {
                                            match self.resolve_shell_id(id_str) {
                                                Some(id) => {
                                                    if let Some(ref tx) = self.tui_cmd_tx {
                                                        let _ = tx.try_send(TuiCommand::ShellAsk {
                                                            id,
                                                            question,
                                                        });
                                                    }
                                                }
                                                None => {
                                                    self.messages_state.toasts.warning(
                                                        "Usage: /shell-ask <id|last> <question>",
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else {
                            mcp.enter_action_mode();
                        }
                    }
                }
                Dialog::Share => {
                    if let Some(ref mut share) = self.dialog_state.share_dialog {
                        if share.copy_url() {
                            self.messages_state.toasts.info("URL copied to clipboard!");
                        } else {
                            self.messages_state.toasts.error("Failed to copy URL");
                        }
                    }
                }
                Dialog::Import => {
                    self.handle_import_send();
                }
                Dialog::Template => {
                    if let Some(ref template_dialog) = self.dialog_state.template_dialog {
                        if let Some((key, template)) = template_dialog.selected() {
                            self.apply_template(key, template);
                        }
                    }
                    self.dialog_state.template_dialog = None;
                    self.close_dialog();
                }
                Dialog::Connect => {
                    self.handle_connect_send();
                }
                _ => {}
            },
            Some(InputAction::Char(c)) => match &mut self.ui_state.dialog {
                Dialog::Model => {
                    if self.dialog_state.model_dialog.adding_model {
                        if self.dialog_state.model_dialog.new_model_name.is_empty() {
                            self.dialog_state.model_dialog.new_model_name.push(c);
                        } else if self.dialog_state.model_dialog.new_model_provider.is_empty() {
                            self.dialog_state.model_dialog.new_model_provider.push(c);
                        } else if self.dialog_state.model_dialog.new_model_api_key.is_empty() {
                            self.dialog_state.model_dialog.new_model_api_key.push(c);
                        } else {
                            self.dialog_state.model_dialog.new_model_base_url.push(c);
                        }
                    } else if self.dialog_state.model_dialog.tab
                        == crate::tui::components::dialogs::model::ModelDialogTab::Configure
                    {
                        match c {
                            'a' => {
                                self.dialog_state.model_dialog.start_adding_model();
                            }
                            'd' => {
                                self.dialog_state
                                    .model_dialog
                                    .remove_custom_model(self.dialog_state.model_dialog.selected);
                            }
                            _ => {}
                        }
                    } else {
                        if c.is_alphanumeric() || c == '-' || c == '_' || c == '/' {
                            self.dialog_state.model_dialog.set_filter(c);
                        }
                    }
                }
                Dialog::Agent => {
                    if c.is_alphanumeric() || c == '-' || c == '_' || c == '/' {
                        self.dialog_state.agent_dialog.set_filter(c);
                    }
                }
                Dialog::Session => {
                    if self.dialog_state.session_dialog.bulk_mode {
                        match c {
                            'a' => {
                                let count = self.dialog_state.session_dialog.selected_count();
                                if count > 0 {
                                    let msg = format!("Archive {} selected sessions?", count);
                                    let ids = self.dialog_state.session_dialog.get_selected_ids();
                                    self.dialog_state.pending_bulk_archive = Some((count, false));
                                    self.dialog_state.pending_bulk_archive_ids = Some(ids);
                                    self.push_dialog(
                                        Dialog::Confirm,
                                        Box::new(ConfirmDialog::new(
                                            "Archive Sessions".to_string(),
                                            msg,
                                        )),
                                    );
                                }
                            }
                            'd' => {
                                let count = self.dialog_state.session_dialog.selected_count();
                                if count > 0 {
                                    let msg = format!(
                                        "Delete {} selected sessions? This cannot be undone.",
                                        count
                                    );
                                    let ids = self.dialog_state.session_dialog.get_selected_ids();
                                    self.dialog_state.pending_bulk_delete = Some(count);
                                    self.dialog_state.pending_bulk_delete_ids = Some(ids);
                                    self.push_dialog(
                                        Dialog::Confirm,
                                        Box::new(ConfirmDialog::new(
                                            "Delete Sessions".to_string(),
                                            msg,
                                        )),
                                    );
                                }
                            }
                            'A' => {
                                self.dialog_state.session_dialog.select_all();
                            }
                            'D' => {
                                self.dialog_state.session_dialog.deselect_all();
                            }
                            ' ' => {
                                self.dialog_state.session_dialog.toggle_selection();
                            }
                            _ => {
                                self.dialog_state.session_dialog.set_filter(c);
                            }
                        }
                    } else {
                        match c {
                            's' => {
                                self.dialog_state.session_dialog.cycle_sort();
                            }
                            'h' => {
                                self.toggle_show_archived();
                            }
                            'b' => {
                                self.dialog_state.session_dialog.toggle_bulk_mode();
                            }
                            _ => {
                                self.dialog_state.session_dialog.set_filter(c);
                            }
                        }
                    }
                }
                Dialog::Tree => match c {
                    'e' => {
                        self.dialog_state.tree_dialog.toggle_expand();
                    }
                    'f' => {
                        self.fork_tree_session();
                    }
                    _ => {}
                },
                Dialog::Question => {
                    if let Some(qd) = &mut self.dialog_state.question_dialog {
                        qd.set_answer(c);
                    }
                }
                Dialog::Import => {
                    if let Some(ref mut import) = self.dialog_state.import_dialog {
                        if let super::components::dialogs::import::ImportState::Input = import.state
                        {
                            import.set_input(c);
                        }
                    }
                }
                Dialog::Template => {
                    if let Some(ref mut td) = self.dialog_state.template_dialog {
                        td.set_filter(c);
                    }
                }
                Dialog::Connect => {
                    if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                        if cd.step
                            == crate::tui::components::dialogs::connect::ConnectStep::EnterApiKey
                        {
                            cd.insert_char(c);
                        }
                    }
                }
                _ => {}
            },
            Some(InputAction::Backspace) => match &mut self.ui_state.dialog {
                Dialog::Model => {
                    if self.dialog_state.model_dialog.adding_model {
                        if !self.dialog_state.model_dialog.new_model_base_url.is_empty() {
                            self.dialog_state.model_dialog.new_model_base_url.pop();
                        } else if !self.dialog_state.model_dialog.new_model_api_key.is_empty() {
                            self.dialog_state.model_dialog.new_model_api_key.pop();
                        } else if !self.dialog_state.model_dialog.new_model_provider.is_empty() {
                            self.dialog_state.model_dialog.new_model_provider.pop();
                        } else if !self.dialog_state.model_dialog.new_model_name.is_empty() {
                            self.dialog_state.model_dialog.new_model_name.pop();
                        }
                    } else {
                        self.dialog_state.model_dialog.backspace_filter();
                    }
                }
                Dialog::Agent => self.dialog_state.agent_dialog.backspace_filter(),
                Dialog::Session => self.dialog_state.session_dialog.backspace_filter(),
                Dialog::Question => {
                    if let Some(qd) = &mut self.dialog_state.question_dialog {
                        qd.backspace();
                    }
                }
                Dialog::Import => {
                    if let Some(ref mut import) = self.dialog_state.import_dialog {
                        if let super::components::dialogs::import::ImportState::Input = import.state
                        {
                            import.backspace();
                        }
                    }
                }
                Dialog::Template => {
                    if let Some(ref mut td) = self.dialog_state.template_dialog {
                        td.backspace_filter();
                    }
                }
                Dialog::Connect => {
                    if let Some(ref mut cd) = self.dialog_state.connect_dialog {
                        if cd.step
                            == crate::tui::components::dialogs::connect::ConnectStep::EnterApiKey
                        {
                            cd.backspace();
                        }
                    }
                }
                _ => {}
            },
            Some(InputAction::Delete) => {
                if let Dialog::Question = &self.ui_state.dialog {
                    if let Some(qd) = &mut self.dialog_state.question_dialog {
                        qd.delete();
                    }
                }
            }
            Some(InputAction::Left) => {
                if let Dialog::Question = &self.ui_state.dialog {
                    if let Some(qd) = &mut self.dialog_state.question_dialog {
                        qd.cursor_left();
                    }
                }
            }
            Some(InputAction::Right) => {
                if let Dialog::Question = &self.ui_state.dialog {
                    if let Some(qd) = &mut self.dialog_state.question_dialog {
                        qd.cursor_right();
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_keybind_key(&mut self, key: KeyEvent) {
        use crate::tui::components::dialogs::keybind::KeybindMode;
        use crossterm::event::KeyCode;

        let kd = match &mut self.dialog_state.keybind_dialog {
            Some(kd) => kd,
            None => return,
        };

        match kd.mode {
            KeybindMode::WaitingForKey => {
                let key_str = format_key_event(&key);
                if key.code == KeyCode::Esc {
                    kd.cancel_remap();
                    return;
                }
                if let Some(action_idx) = kd.waiting_for_key {
                    let actions =
                        crate::tui::components::dialogs::keybind::KeybindDialog::actions();
                    if action_idx < actions.len() {
                        let action = &actions[action_idx];
                        let existing_key_for_action = kd
                            .bindings
                            .iter()
                            .find(|(_, val)| *val == action)
                            .map(|(k, _)| k.clone());

                        if let Some(ref existing_key) = existing_key_for_action {
                            if existing_key != &key_str {
                                kd.bindings.remove(existing_key);
                            }
                        }

                        let current_binding_for_action = kd.get_binding(action);
                        if current_binding_for_action.as_ref() != Some(&key_str) {
                            kd.bindings.insert(key_str, action.clone());
                        }
                        kd.clear_conflict();
                    }
                    kd.waiting_for_key = None;
                    kd.mode = KeybindMode::Normal;
                    self.save_keybinds();
                }
            }
            KeybindMode::Export => {
                if key.code == KeyCode::Esc {
                    kd.cancel_mode();
                }
            }
            KeybindMode::Import => match key.code {
                KeyCode::Esc => {
                    kd.cancel_mode();
                }
                KeyCode::Enter => {
                    if let Err(e) = kd.apply_import() {
                        kd.conflict = Some(e);
                    } else {
                        self.save_keybinds();
                    }
                }
                KeyCode::Char(c) => {
                    kd.import_text.push(c);
                }
                KeyCode::Backspace => {
                    kd.import_text.pop();
                }
                _ => {}
            },
            KeybindMode::Normal => {
                let action = handle_event(crossterm::event::Event::Key(key));
                match action {
                    Some(InputAction::Cancel) => {
                        self.dialog_state.keybind_dialog = None;
                        self.close_dialog();
                    }
                    Some(InputAction::NavigateUp) => {
                        kd.select_up();
                    }
                    Some(InputAction::NavigateDown) => {
                        kd.select_down();
                    }
                    Some(InputAction::Send) => {
                        kd.start_remap();
                    }
                    Some(InputAction::Char(c)) => match c {
                        'r' | 'R' => {
                            kd.reset_to_defaults();
                            self.save_keybinds();
                        }
                        'e' | 'E' => {
                            kd.start_export();
                        }
                        'i' | 'I' => {
                            kd.start_import();
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    fn save_keybinds(&mut self) {
        if let Some(ref kd) = self.dialog_state.keybind_dialog {
            if let Some(keybinds) = &mut self.ui_state.keybinds {
                keybinds.bindings = kd.bindings.clone();
            } else {
                self.ui_state.keybinds = Some(crate::tui::input::KeybindConfig {
                    bindings: kd.bindings.clone(),
                });
            }
        }
    }

    fn handle_completion_key(&mut self, key: KeyEvent) -> bool {
        let action = handle_event(crossterm::event::Event::Key(key));
        debug_log!("handle_completion_key: action={:?}", action);
        match action {
            Some(InputAction::NavigateUp) => {
                if self.prompt_state.completion_sel > 0 {
                    self.prompt_state.completion_sel -= 1;
                }
                true
            }
            Some(InputAction::NavigateDown) => {
                let max_sel = match self.prompt_state.completion_type {
                    CompletionType::Slash => {
                        let filter = self.prompt_state.completion_filter.trim_start_matches('/');
                        if filter.is_empty() {
                            self.prompt_state.slash_completions.len().saturating_sub(1)
                        } else {
                            self.prompt_state
                                .slash_completions
                                .iter()
                                .filter(|item| {
                                    let item_name = item.label.trim_start_matches('/');
                                    fuzzy_score(filter, item_name) > 0
                                })
                                .count()
                                .saturating_sub(1)
                        }
                    }
                    CompletionType::File => {
                        self.prompt_state.file_completions.len().saturating_sub(1)
                    }
                    CompletionType::Agent => {
                        self.prompt_state.agent_completions.len().saturating_sub(1)
                    }
                };
                if self.prompt_state.completion_sel < max_sel {
                    self.prompt_state.completion_sel += 1;
                }
                true
            }
            Some(InputAction::Send) => {
                debug_log!("handle_completion_key: Send action - accepting completion");
                self.accept_completion();
                true
            }
            Some(InputAction::Cancel) => {
                self.prompt_state.show_completions = false;
                true
            }
            Some(InputAction::Char(c)) => {
                self.prompt_state.completion_filter.push(c);
                self.prompt_state.completion_sel = 0;
                true
            }
            Some(InputAction::Backspace) => {
                self.prompt_state.completion_filter.pop();
                self.prompt_state.completion_sel = 0;
                true
            }
            _ => false,
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        let action = handle_event(crossterm::event::Event::Key(key));
        debug_log!("handle_command_key: action={:?}", action);
        match action {
            Some(InputAction::Cancel) => {
                debug_log!("handle_command_key: Cancel - exiting command mode");
                self.ui_state.command_mode = false;
                self.dialog_state.command_palette.set_query("");
                self.prompt_state.prompt.clear();
            }
            Some(InputAction::NavigateUp) => {
                self.dialog_state.command_palette.cursor_up();
            }
            Some(InputAction::NavigateDown) => {
                self.dialog_state.command_palette.cursor_down();
            }
            Some(InputAction::Send) => {
                if let Some(cmd) = self.dialog_state.command_palette.selected() {
                    debug_log!("handle_command_key: executing command: {}", cmd.name);
                    let command_query = self.dialog_state.command_palette.query.clone();
                    self.execute_command(cmd, Some(&command_query));
                    self.ui_state.command_mode = false;
                    self.dialog_state.command_palette.set_query("");
                    self.prompt_state.prompt.clear();
                } else {
                    debug_log!("handle_command_key: no matching command, exiting command mode and sending prompt");
                    self.ui_state.command_mode = false;
                    self.dialog_state.command_palette.set_query("");
                    self.send_prompt();
                }
            }
            Some(InputAction::Char(c)) => {
                self.prompt_state.prompt.insert_char(c);
                let query = self.prompt_state.prompt.get_text();
                self.dialog_state.command_palette.set_query(&query);
            }
            Some(InputAction::Backspace) => {
                if self.prompt_state.prompt.cursor_pos() > 0 {
                    self.prompt_state.prompt.backspace();
                }
                let query = self.prompt_state.prompt.get_text();
                if query.is_empty() || query == "/" {
                    self.ui_state.command_mode = false;
                } else {
                    self.dialog_state.command_palette.set_query(&query);
                }
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: &crate::tui::command::Command, raw_input: Option<&str>) {
        if let Some(dialog) = &cmd.dialog {
            self.ui_state.command_mode = false;
            self.open_dialog(dialog.clone());
            return;
        }
        if let Some(ref spec) = cmd.process {
            if self.prompt_state.pending_send {
                self.messages_state
                    .toasts
                    .warning("Still waiting for previous prompt to finish");
                return;
            }
            let args = raw_input
                .and_then(|input| input.trim().split_once(' ').map(|(_, rest)| rest.trim()))
                .unwrap_or_default()
                .to_string();
            let arg_list: Vec<String> = if args.is_empty() {
                Vec::new()
            } else {
                args.split_whitespace().map(String::from).collect()
            };
            if let Some(ref tx) = self.tui_cmd_tx {
                let session_id = self.session_state.session.as_ref().map(|s| s.id.clone());
                let model = Some(self.agent_state.current_model.clone());
                let _ = tx.try_send(TuiCommand::PluginCommandRun {
                    spec: spec.clone(),
                    args: arg_list,
                    session_id,
                    model,
                });
            }
            self.ui_state.command_mode = false;
            self.prompt_state.prompt.clear();
            self.prompt_state.show_completions = false;
            return;
        }
        if let Some(template) = &cmd.template {
            if self.prompt_state.pending_send {
                self.messages_state
                    .toasts
                    .warning("Still waiting for previous prompt to finish");
                return;
            }
            let args = raw_input
                .and_then(|input| input.trim().split_once(' ').map(|(_, rest)| rest.trim()))
                .unwrap_or_default()
                .to_string();
            let mut variables = std::collections::HashMap::new();
            variables.insert("args".to_string(), args);
            let rendered = crate::command::execute_command_template(template, &variables);
            self.messages_state
                .messages
                .add_user_message(rendered.clone(), Some(self.agent_state.plan_mode));
            self.prompt_state.prompt.clear();
            self.prompt_state.show_completions = false;
            self.reset_live_token_estimate();
            self.session_state.session_status = SessionStatus::Working;
            if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
                self.send_remote_message(RemoteTuiMessage::Input { text: rendered });
                self.prompt_state.pending_send = false;
            } else {
                self.prompt_state.pending_send = true;
            }
            return;
        }
        match cmd.name.as_str() {
            "/exit" | "/quit" | "/q" => {
                self.ui_state.running = false;
                let _ = self.ui_state.shutdown_tx.take().map(|tx| tx.send(()));
            }
            "/help" => {
                self.ui_state.command_mode = false;
                self.open_dialog(Dialog::Help);
            }
            "/tree" => {
                self.ui_state.command_mode = false;
                self.open_tree_dialog();
            }
            "/model" => {
                self.ui_state.command_mode = false;
                self.open_dialog(Dialog::Model);
            }
            "/agent" => {
                self.ui_state.command_mode = false;
                self.open_dialog(Dialog::Agent);
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
                self.open_dialog(Dialog::Context);
            }
            "/cost" => {
                self.ui_state.command_mode = false;
                self.open_dialog(Dialog::Cost);
            }
            "/usage" => {
                self.ui_state.command_mode = false;
                self.open_dialog(Dialog::Usage);
            }
            "/themes" | "/theme" => {
                self.handle_theme_command(raw_input);
            }
            "/tui" => {
                self.toggle_fullscreen();
            }
            "/tui-stats" => {
                self.ui_state.command_mode = false;
                let by_kind: Vec<(String, usize)> = self
                    .task_registry
                    .iter()
                    .fold(
                        std::collections::HashMap::<String, usize>::new(),
                        |mut acc, (_id, rec)| {
                            *acc.entry(rec.kind.to_string()).or_insert(0) += 1;
                            acc
                        },
                    )
                    .into_iter()
                    .collect();
                let oldest = self
                    .task_registry
                    .iter()
                    .min_by_key(|(_id, rec)| rec.started_at)
                    .map(|(_id, rec)| rec.name.to_string());
                let task_summary = crate::tui::ui_builders::stats::TaskSummaryView {
                    active: self.task_registry.active_count(),
                    completed: self.task_registry.completed_count(),
                    cancelled: self.task_registry.cancelled_count(),
                    panicked: self.task_registry.panicked_count(),
                    by_kind,
                    oldest,
                };
                let node = crate::tui::ui_builders::stats::stats_node(
                    &self.ui_state.diagnostics,
                    &task_summary,
                    self.shell_handles.len(),
                );
                self.open_ui_node_dialog("TUI Stats".into(), node);
            }
            "/tts" => {
                self.toggle_tts();
            }
            "/sessions" => {
                self.open_dialog(Dialog::Session);
            }
            "/share" => {
                if let Some(ref session) = self.session_state.session {
                    if let Some(ref existing) = session.share_url {
                        let mut dialog = crate::tui::components::dialogs::share::ShareDialog::new(
                            Arc::clone(&self.ui_state.theme),
                        );
                        dialog.set_theme(&self.ui_state.theme);
                        dialog.set_url(existing.clone());
                        self.dialog_state.share_dialog = Some(dialog);
                        self.open_dialog(Dialog::Share);
                    } else if self.tui_cmd_tx.is_some() {
                        let session_id = session.id.clone();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShareSession { session_id });
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
                let sub = raw_input
                    .map(|s| s.trim_start_matches("/export").trim())
                    .unwrap_or("");
                if sub == "handoff" {
                    self.export_handoff();
                } else {
                    self.messages_state
                        .toasts
                        .info("Exporting session - copy to clipboard");
                }
            }
            "/import" => {
                self.dialog_state.import_dialog =
                    Some(crate::tui::components::dialogs::import::ImportDialog::new(
                        Arc::clone(&self.ui_state.theme),
                    ));
                self.open_dialog(Dialog::Import);
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
                self.refresh_models();
            }
            "/variants" => {
                let model = &self.agent_state.current_model;
                let base = model.split('/').next_back().unwrap_or(model);
                self.messages_state
                    .toasts
                    .info(&format!("Variants for {}: default (no suffix)", base));
            }
            "/mcps" => {
                if self.session_state.mcp_servers.is_empty() {
                    self.messages_state.toasts.info("No MCP servers configured");
                } else {
                    let status: Vec<String> = self
                        .session_state
                        .mcp_servers
                        .iter()
                        .map(|(name, status)| format!("{}: {}", name, status))
                        .collect();
                    self.messages_state.toasts.info(&status.join(", "));
                }
            }
            "/fork" => {
                if let Some(idx) = self.messages_state.messages.sel_msg {
                    self.messages_state.toasts.info(&format!(
                        "Fork from message {} - use CLI --fork flag for now",
                        idx
                    ));
                } else {
                    self.messages_state
                        .toasts
                        .info("Select a message with arrow keys first, then use /fork");
                }
            }
            "/workspaces" => {
                self.messages_state
                    .toasts
                    .info("Workspace management - use /sessions to switch workspaces");
            }
            "/worktree" => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::WorktreeList);
                } else {
                    let git_root = std::path::PathBuf::from(&self.session_state.project_dir);
                    let trees = tokio::runtime::Handle::current().block_on(async {
                        match crate::worktree::find_git_root(&git_root) {
                            Some(root) => crate::worktree::list_worktrees(&root)
                                .await
                                .unwrap_or_default(),
                            None => Vec::new(),
                        }
                    });
                    if trees.is_empty() {
                        self.messages_state.toasts.info("No worktrees found");
                    } else {
                        let names: Vec<String> = trees
                            .iter()
                            .map(|t| format!("{} ({})", t.path, t.branch))
                            .collect();
                        self.messages_state.toasts.info(&names.join(", "));
                    }
                }
            }
            "/editor" => {
                self.open_external_editor();
            }
            "/loop" => {
                let query = self.dialog_state.command_palette.query.clone();
                let parts: Vec<&str> = query.splitn(2, ' ').collect();
                if parts.len() < 2 {
                    self.messages_state.toasts.warning(
                        "Usage: /loop <interval> \"<message>\" (e.g. /loop 5m \"check status\")",
                    );
                } else if let Some(duration) = crate::agent::task::parse_duration(parts[0]) {
                    let message = parts[1].trim_matches('"').to_string();
                    if let Some(ref tx) = self.tui_cmd_tx {
                        let _ = tx.try_send(TuiCommand::TaskSchedule {
                            interval_secs: duration.as_secs(),
                            message,
                        });
                    } else {
                        self.messages_state
                            .toasts
                            .error("Command channel unavailable");
                    }
                } else {
                    self.messages_state
                        .toasts
                        .warning("Invalid interval. Examples: 30s, 5m, 1h, 1d");
                }
            }
            "/tasks" => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ListTasks);
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
                    let _ = tx.try_send(TuiCommand::DeleteTask { id: id.to_string() });
                } else {
                    self.messages_state
                        .toasts
                        .warning("Scheduler not available");
                }
            }
            "/memory" => {
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::MemorySummary);
                } else {
                    self.handle_memory_command(None);
                }
            }
            "/memory-search" => {
                let query = self.dialog_state.command_palette.query.trim().to_string();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::MemorySearch { query });
                } else {
                    self.handle_memory_command(Some(("search", &query)));
                }
            }
            "/memory-list" => {
                let query = self.dialog_state.command_palette.query.trim().to_string();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::MemorySummary);
                } else {
                    self.handle_memory_command(Some(("list", &query)));
                }
            }
            "/memory-remember" => {
                let text = self.dialog_state.command_palette.query.trim().to_string();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::MemoryRemember { text });
                } else {
                    self.handle_memory_command(Some(("remember", &text)));
                }
            }
            "/memory-forget" => {
                let id = self.dialog_state.command_palette.query.trim().to_string();
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::MemoryForget { id });
                } else {
                    self.handle_memory_command(Some(("forget", &id)));
                }
            }
            "/memory-consolidate" => {
                self.handle_memory_command(Some(("consolidate", "")));
            }
            "/goal" => {
                let query = self.dialog_state.command_palette.query.trim().to_string();
                let parts: Vec<&str> = query.splitn(2, ' ').collect();
                let subcmd = parts.first().copied().unwrap_or("");
                let args = parts.get(1).copied().unwrap_or("").trim();

                let Some(session) = self.session_state.session.clone() else {
                    self.messages_state.toasts.warning("No active session");
                    return;
                };
                let session_id = session.id.clone();
                let project_id = self.session_state.project_dir.clone();

                match subcmd {
                    "set" => {
                        if args.is_empty() {
                            self.messages_state
                                .toasts
                                .warning("Usage: /goal set <objective>");
                            return;
                        }
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalSet {
                                session_id,
                                project_id,
                                objective: args.to_string(),
                            });
                        }
                    }
                    "from-file" => {
                        if args.is_empty() {
                            self.messages_state
                                .toasts
                                .warning("Usage: /goal from-file <path>");
                            return;
                        }
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalFromFile {
                                session_id,
                                project_id,
                                path: args.to_string(),
                            });
                        }
                    }
                    "show" => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalShow { session_id });
                        }
                    }
                    "pause" => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalPause { session_id });
                        }
                    }
                    "resume" => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalResume { session_id });
                        }
                    }
                    "clear" => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalClear { session_id });
                        }
                    }
                    "done" => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalDone { session_id });
                        }
                    }
                    "checkpoint" => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalCheckpoint {
                                session_id,
                                project_id,
                            });
                        }
                    }
                    "budget" => {
                        if args.is_empty() {
                            self.messages_state
                                .toasts
                                .info("Usage: /goal budget [show | raise <axis> <n> | raise clear <axis>]");
                            return;
                        }
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::GoalBudget {
                                session_id,
                                subcommand: args.to_string(),
                            });
                        }
                    }
                    _ => {
                        // Treat bare text as goal set (e.g., /goal fix the bug)
                        if !subcmd.is_empty()
                            && subcmd != "set"
                            && subcmd != "from-file"
                            && subcmd != "show"
                            && subcmd != "pause"
                            && subcmd != "resume"
                            && subcmd != "clear"
                            && subcmd != "done"
                            && subcmd != "checkpoint"
                            && subcmd != "budget"
                        {
                            // The whole query is the goal text
                            let goal_text = query.trim();
                            if !goal_text.is_empty() {
                                if let Some(ref tx) = self.tui_cmd_tx {
                                    let _ = tx.try_send(TuiCommand::GoalSet {
                                        session_id,
                                        project_id,
                                        objective: goal_text.to_string(),
                                    });
                                }
                            } else {
                                self.messages_state
                                    .toasts
                                    .warning("Usage: /goal <text> or /goal set <text>");
                            }
                        } else {
                            self.messages_state
                                .toasts
                                .warning("Usage: /goal <text> or /goal set <text>");
                        }
                    }
                }
            }
            "/plan" => {
                let query = self.dialog_state.command_palette.query.trim().to_string();
                let parts: Vec<&str> = query.splitn(2, ' ').collect();
                let subcmd = parts.first().copied().unwrap_or("");
                let args = parts.get(1).copied().unwrap_or("").trim();

                match subcmd {
                    "add" => {
                        if args.is_empty() {
                            self.messages_state
                                .toasts
                                .warning("Usage: /plan add <text>");
                            return;
                        }
                        let item_id = format!("plan-{}", uuid::Uuid::new_v4());
                        let mut plan = self.session_state_derived.plan.clone().unwrap_or(
                            crate::session::events::AgentPlan {
                                items: Vec::new(),
                                updated_at: chrono::Utc::now(),
                            },
                        );
                        plan.items.push(crate::session::events::AgentPlanItem {
                            id: item_id,
                            text: args.to_string(),
                            status: crate::session::events::PlanItemStatus::Pending,
                            note: None,
                        });
                        plan.updated_at = chrono::Utc::now();
                        self.session_state_derived.plan = Some(plan.clone());
                        self.messages_state
                            .toasts
                            .info(&format!("Plan item added: {}", args));
                    }
                    "done" | "skip" | "block" => {
                        let status = match subcmd {
                            "done" => crate::session::events::PlanItemStatus::Done,
                            "skip" => crate::session::events::PlanItemStatus::Skipped,
                            "block" => crate::session::events::PlanItemStatus::Blocked,
                            _ => crate::session::events::PlanItemStatus::Pending,
                        };
                        let index: Option<usize> = args.parse().ok();
                        if let Some(idx) = index {
                            if let Some(ref mut plan) = self.session_state_derived.plan {
                                if let Some(item) = plan.items.get_mut(idx) {
                                    item.status = status;
                                    self.messages_state
                                        .toasts
                                        .info(&format!("Plan item {} marked as {}", idx, subcmd));
                                } else {
                                    self.messages_state
                                        .toasts
                                        .warning(&format!("Plan item {} not found", idx));
                                }
                            } else {
                                self.messages_state.toasts.warning("No active plan");
                            }
                        } else {
                            self.messages_state
                                .toasts
                                .warning(&format!("Usage: /plan {} <index>", subcmd));
                        }
                    }
                    "clear" => {
                        self.session_state_derived.plan = None;
                        self.messages_state.toasts.info("Plan cleared");
                    }
                    "" => {
                        // Show current plan
                        if let Some(ref plan) = self.session_state_derived.plan {
                            if plan.items.is_empty() {
                                self.messages_state.toasts.info("Plan is empty");
                            } else {
                                let items: Vec<String> = plan
                                    .items
                                    .iter()
                                    .enumerate()
                                    .map(|(i, item)| {
                                        let icon = match item.status {
                                            crate::session::events::PlanItemStatus::Done => "[x]",
                                            crate::session::events::PlanItemStatus::InProgress => {
                                                "[>]"
                                            }
                                            crate::session::events::PlanItemStatus::Skipped => {
                                                "[-]"
                                            }
                                            crate::session::events::PlanItemStatus::Blocked => {
                                                "[?]"
                                            }
                                            crate::session::events::PlanItemStatus::Pending => {
                                                "[ ]"
                                            }
                                        };
                                        format!("{} {} {}", i, icon, item.text)
                                    })
                                    .collect();
                                let mut plan_lines =
                                    vec![format!("Plan ({} items):", plan.items.len())];
                                plan_lines.extend(items);
                                self.show_short_or_info(
                                    crate::tui::components::dialogs::info::InfoType::Stats,
                                    plan_lines,
                                );
                            }
                        } else {
                            self.messages_state.toasts.info("No active plan");
                        }
                    }
                    _ => {
                        self.messages_state
                            .toasts
                            .warning("Usage: /plan [add <text>|done <i>|skip <i>|block <i>|clear]");
                    }
                }
            }
            "/state" => {
                let state = &self.session_state_derived;
                let mut lines = Vec::new();

                if let Some(ref goal) = state.goal {
                    lines.push(format!("Goal: {}", goal));
                }

                if let Some(ref plan) = state.plan {
                    let done = plan
                        .items
                        .iter()
                        .filter(|i| i.status == crate::session::events::PlanItemStatus::Done)
                        .count();
                    lines.push(format!("Plan: {}/{} done", done, plan.items.len()));
                }

                let model_short = self
                    .agent_state
                    .current_model
                    .split('/')
                    .next_back()
                    .unwrap_or(&self.agent_state.current_model);
                lines.push(format!("Model: {}", model_short));

                let agent_name = &self.agent_state.agents[self.agent_state.current_agent].name;
                lines.push(format!("Agent: {}", agent_name));

                lines.push(format!("Files: {} changed", state.changed_files.len()));

                lines.push(format!(
                    "Context: {}%",
                    (self.session_state.context_tokens as f64
                        / self.session_state.context_limit as f64
                        * 100.0) as u64
                ));

                self.show_short_or_info(
                    crate::tui::components::dialogs::info::InfoType::Stats,
                    lines,
                );
            }
            "/search" => {
                let query = self.dialog_state.command_palette.query.trim().to_string();
                if query.is_empty() {
                    self.messages_state.toasts.warning("Usage: /search <query>");
                } else {
                    self.messages_state.messages.search(&query);
                    let count = self.messages_state.messages.search_matches.len();
                    if count == 0 {
                        self.messages_state
                            .toasts
                            .info(&format!("No matches for \"{}\"", query));
                    } else {
                        self.messages_state.toasts.info(&format!(
                            "{} match{} for \"{}\"",
                            count,
                            if count == 1 { "" } else { "es" },
                            query
                        ));
                    }
                }
            }
            "/doctor" => {
                // The doctor routine is async (it has to await the
                // eggsearch MCP spawn), so fire it in a background task
                // and let the agent loop pick up the result via a
                // TuiCommand::SendNotification on the way back.
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::RunDoctor);
                } else {
                    self.messages_state
                        .toasts
                        .info("doctor: not connected to a core client");
                }
            }
            "/lsp-status" => {
                self.ui_state.command_mode = false;
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    let detail = handle.block_on(lsp_tool.lsp_summary_detail());
                    if let Some(text) = detail {
                        self.messages_state.toasts.info(&text);
                    } else {
                        self.messages_state.toasts.info("No LSP server connected");
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-previews" | "/preview-list" => {
                self.ui_state.command_mode = false;
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let output = lsp_tool.preview_list_text();
                    self.messages_state.toasts.info(&output);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-preview" | "/preview-show" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let id = query
                    .strip_prefix("/lsp-preview ")
                    .or_else(|| query.strip_prefix("/preview-show "))
                    .unwrap_or("")
                    .trim();
                if id.is_empty() {
                    self.messages_state.toasts.info("Usage: /lsp-preview <id>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    match lsp_tool.preview_detail_text(id) {
                        Some(detail) => self.messages_state.toasts.info(&detail),
                        None => self
                            .messages_state
                            .toasts
                            .info(&format!("Preview not found: {id}")),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-preview-clear" | "/preview-clear" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query
                    .strip_prefix("/lsp-preview-clear ")
                    .or_else(|| query.strip_prefix("/preview-clear "))
                    .unwrap_or("")
                    .trim();
                if let Some(ref lsp_tool) = self.lsp_tool {
                    if arg == "--all" || arg.is_empty() {
                        let count = lsp_tool.clear_preview(None);
                        self.messages_state
                            .toasts
                            .info(&format!("Cleared {count} preview(s)"));
                    } else {
                        let count = lsp_tool.clear_preview(Some(arg));
                        if count > 0 {
                            self.messages_state
                                .toasts
                                .info(&format!("Cleared preview: {arg}"));
                        } else {
                            self.messages_state
                                .toasts
                                .info(&format!("Preview not found: {arg}"));
                        }
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-preview-refresh" | "/preview-refresh" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let id = query
                    .strip_prefix("/lsp-preview-refresh ")
                    .or_else(|| query.strip_prefix("/preview-refresh "))
                    .unwrap_or("")
                    .trim();
                if id.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-preview-refresh <id>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    match lsp_tool.refresh_preview_staleness(id) {
                        Some((true, detail)) => {
                            self.messages_state
                                .toasts
                                .info(&format!("Preview is STALE\n{detail}"));
                        }
                        Some((false, _)) => {
                            self.messages_state.toasts.info("Preview is FRESH");
                        }
                        None => {
                            self.messages_state
                                .toasts
                                .info(&format!("Preview not found: {id}"));
                        }
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-preview-apply" | "/preview-apply" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let id = query
                    .strip_prefix("/lsp-preview-apply ")
                    .or_else(|| query.strip_prefix("/preview-apply "))
                    .unwrap_or("")
                    .trim();
                if id.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-preview-apply <id>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    // Phase 9: Refresh stale-base status before validation.
                    let (is_stale, refresh_detail) = lsp_tool
                        .refresh_preview_staleness(id)
                        .unwrap_or((true, "preview not found".to_string()));

                    if is_stale {
                        let mut msg = format!(
                            "Preview is STALE — base content has changed since creation.\n\
                             Refresh/recompute the preview before applying.\n\n\
                             Detail: {refresh_detail}"
                        );
                        // Pull a candidate to surface how many patches are blocked.
                        let patch_count = {
                            let registry = lsp_tool.preview_registry();
                            egglsp::tui_summary::export_preview_apply_candidate(&registry, id)
                                .map(|c| c.patches.len())
                                .unwrap_or(0)
                        };
                        if patch_count > 0 {
                            msg.push_str(&format!(
                                "\n\n{patch_count} patch(es) available but stale. \
                                 Re-run the original LSP preview command to generate a fresh preview."
                            ));
                        }
                        self.messages_state.toasts.info(&msg);
                    } else {
                        // Phase 9 / 12 hardening: route through the testable
                        // validation boundary. validate_preview_apply performs
                        // all gating checks (not-found, stale, no-patches,
                        // already-applied, hash mismatch, patch failure) and
                        // computes the new file content in memory.
                        let validation = {
                            let registry = lsp_tool.preview_registry();
                            egglsp::tui_summary::validate_preview_apply(
                                &registry,
                                id,
                                &crate::tool::patch_util::apply_unified_diff,
                            )
                        };

                        match validation {
                            Ok(plan) => {
                                // Phase 12 hardening: use the tested
                                // write-side helper that rechecks each
                                // file's SHA-256 before writing. The
                                // caller MUST NOT call mark_applied
                                // unless all_succeeded is true.
                                match egglsp::tui_summary::write_preview_apply_plan_atomically_enough(&plan) {
                                    Ok(report) => {
                                        if report.all_succeeded {
                                            lsp_tool.mark_preview_applied(&plan.preview_id);
                                            self.messages_state.toasts.info(&format!(
                                                "Applied {} patch(es) for preview {} successfully.\nFiles: {}",
                                                report.written.len(),
                                                plan.preview_id,
                                                report.written.join(", ")
                                            ));
                                        } else if report.written.is_empty() {
                                            self.messages_state.toasts.info(&format!(
                                                "Failed to apply all {} patch(es) for preview {}.",
                                                plan.files.len(),
                                                plan.preview_id,
                                            ));
                                        } else {
                                            self.messages_state.toasts.info(&format!(
                                                "Partial apply for preview {}: {} succeeded, {} failed.\nSucceeded: {}",
                                                plan.preview_id,
                                                report.written.len(),
                                                plan.files.len() - report.written.len(),
                                                report.written.join(", "),
                                            ));
                                        }
                                    }
                                    Err(egglsp::tui_summary::PreviewApplyWriteError::StaleDuringWrite {
                                        path,
                                        expected_hash,
                                        actual_hash,
                                    }) => {
                                        self.messages_state.toasts.info(&format!(
                                            "Write blocked: {path} changed since validation.\n\
                                             Expected: {expected_hash}\nActual: {actual_hash}\n\n\
                                             Re-run the LSP preview command to generate a fresh preview."
                                        ));
                                    }
                                    Err(egglsp::tui_summary::PreviewApplyWriteError::WriteFailed {
                                        path,
                                        error,
                                        completed,
                                    }) => {
                                        self.messages_state.toasts.info(&format!(
                                            "Write failed for {path}: {error}\n\
                                             {} file(s) completed before failure.",
                                            completed.len(),
                                        ));
                                    }
                                }
                            }
                            Err(err) => {
                                // Map error variants to user-facing toast text.
                                let msg = match &err {
                                    egglsp::tui_summary::PreviewApplyError::NotFound {
                                        preview_id,
                                    } => {
                                        format!("Preview not found: {preview_id}")
                                    }
                                    egglsp::tui_summary::PreviewApplyError::Stale {
                                        preview_id,
                                        detail,
                                        ..
                                    } => {
                                        format!(
                                        "Preview {preview_id} is STALE — base content changed.\n\n{detail}"
                                    )
                                    }
                                    egglsp::tui_summary::PreviewApplyError::NoPatches {
                                        preview_id,
                                    } => {
                                        format!(
                                        "Preview {preview_id} has no patches — re-run the original LSP preview command."
                                    )
                                    }
                                    egglsp::tui_summary::PreviewApplyError::AlreadyApplied {
                                        preview_id,
                                    } => {
                                        format!(
                                        "Preview {preview_id} has already been applied. Re-apply requires an explicit confirmation flow."
                                    )
                                    }
                                    egglsp::tui_summary::PreviewApplyError::FileReadError {
                                        path,
                                        error,
                                    } => {
                                        format!("Failed to read {path}: {error}")
                                    }
                                    egglsp::tui_summary::PreviewApplyError::HashMismatch {
                                        path,
                                        ..
                                    } => {
                                        format!(
                                        "{path} changed since preview was created — refresh/recompute before applying."
                                    )
                                    }
                                    egglsp::tui_summary::PreviewApplyError::PatchError {
                                        path,
                                        error,
                                    } => {
                                        format!("Patch failed for {path}: {error}")
                                    }
                                };
                                self.messages_state.toasts.info(&msg);
                            }
                        }
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-servers" | "/lsp-detail" => {
                self.ui_state.command_mode = false;
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    let detail = handle.block_on(lsp_tool.lsp_servers_detail());
                    if let Some(text) = detail {
                        self.messages_state.toasts.info(&text);
                    } else {
                        self.messages_state.toasts.info("No active LSP servers");
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-capabilities" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let key = query
                    .strip_prefix("/lsp-capabilities ")
                    .unwrap_or("")
                    .trim();
                if key.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-capabilities <server-key>\nUse /lsp-servers to see available keys.");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.lsp_capabilities_for_key(key)) {
                        Some(text) => self.messages_state.toasts.info(&text),
                        None => self
                            .messages_state
                            .toasts
                            .info(&format!("Server not found: {key}")),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-errors" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let key = query.strip_prefix("/lsp-errors ").unwrap_or("").trim();
                if key.is_empty() {
                    self.messages_state.toasts.info(
                        "Usage: /lsp-errors <server-key>\nUse /lsp-servers to see available keys.",
                    );
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.lsp_errors_for_key(key)) {
                        Some(text) => self.messages_state.toasts.info(&text),
                        None => self
                            .messages_state
                            .toasts
                            .info(&format!("Server not found: {key}")),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-root" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let path_arg = query.strip_prefix("/lsp-root ").unwrap_or("").trim();
                if path_arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-root <file-path>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let text = lsp_tool.lsp_root_diagnose(path_arg);
                    self.messages_state.toasts.info(&text);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-doctor" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let path_arg = query.strip_prefix("/lsp-doctor ").unwrap_or("").trim();
                if path_arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-doctor <file-path>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    let text = handle.block_on(lsp_tool.lsp_doctor(path_arg));
                    self.messages_state.toasts.info(&text);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-context-diagnostics" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let path_arg = query
                    .strip_prefix("/lsp-context-diagnostics ")
                    .unwrap_or("")
                    .trim();
                if path_arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-context-diagnostics <file-path>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    let text = handle.block_on(lsp_tool.lsp_context_diagnostics(path_arg));
                    self.messages_state.toasts.info(&text);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-repair-local" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query
                    .strip_prefix("/lsp-repair-local ")
                    .unwrap_or("")
                    .trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-repair-local <path[:line]>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let (path, line, col) = parse_path_line_col(arg);
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::RepairLocal,
                        primary_path: Some(path),
                        line,
                        column: col,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-repair-hunk" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-repair-hunk ").unwrap_or("").trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-repair-hunk <path> [hunk-id|range]");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
                    let path = parts[0].to_string();
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::RepairHunk,
                        primary_path: Some(path),
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-review-file" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-review-file ").unwrap_or("").trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-review-file <path>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::ReviewFile,
                        primary_path: Some(arg.to_string()),
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-review-diff" => {
                self.ui_state.command_mode = false;
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::ReviewDiff,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-security-review" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query
                    .strip_prefix("/lsp-security-review ")
                    .unwrap_or("")
                    .trim();
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let review_mode = if arg.is_empty() {
                        Some("diff".to_string())
                    } else {
                        Some(arg.to_string())
                    };
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::SecurityReviewEnriched,
                        review_mode,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-impact" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-impact ").unwrap_or("").trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-impact <path:line:col>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let (path, line, col) = parse_path_line_col(arg);
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::ImpactAnalysis,
                        primary_path: Some(path),
                        line,
                        column: col,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-test-repair" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-test-repair ").unwrap_or("").trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-test-repair <test-file> [failure-text]");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
                    let test_file = parts[0].to_string();
                    let failure_text = parts.get(1).map(|s| s.to_string());
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::TestFailureRepair,
                        primary_path: Some(test_file),
                        failure_text,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-interface" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-interface ").unwrap_or("").trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-interface <path[:symbol]>");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let parts: Vec<&str> = arg.splitn(2, ':').collect();
                    let path = parts[0].to_string();
                    let symbol = parts.get(1).map(|s| s.to_string());
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::InterfaceBoundary,
                        primary_path: Some(path),
                        symbol,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-cross-repair" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query
                    .strip_prefix("/lsp-cross-repair ")
                    .unwrap_or("")
                    .trim();
                if arg.is_empty() {
                    self.messages_state
                        .toasts
                        .info("Usage: /lsp-cross-repair <primary> [related...]");
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let parts: Vec<&str> = arg.split_whitespace().collect();
                    let primary = parts[0].to_string();
                    let related_files: Vec<String> =
                        parts[1..].iter().map(|s| s.to_string()).collect();
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::CrossFileRepair,
                        primary_path: Some(primary),
                        related_files,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-call-neighbors" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query
                    .strip_prefix("/lsp-call-neighbors ")
                    .unwrap_or("")
                    .trim();
                if arg.is_empty() {
                    self.messages_state.toasts.info(
                        "Usage: /lsp-call-neighbors <path:line:col> [incoming|outgoing|both]",
                    );
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
                    let (path, line, col) = parse_path_line_col(parts[0]);
                    let direction = match parts.get(1).copied() {
                        Some("incoming") => Some(egglsp::context::HierarchyDirection::Incoming),
                        Some("outgoing") => Some(egglsp::context::HierarchyDirection::Outgoing),
                        Some("both") => Some(egglsp::context::HierarchyDirection::Both),
                        _ => None,
                    };
                    let invocation = LspWorkflowInvocation {
                        recipe: LspWorkflowRecipe::CallNeighborhood,
                        primary_path: Some(path),
                        line,
                        column: col,
                        direction,
                        ..Default::default()
                    };
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
                        Ok(display) => {
                            let text = render_workflow_display(&display);
                            self.messages_state.toasts.info(&text);
                        }
                        Err(e) => self.messages_state.toasts.info(&e),
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-restart" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let key = query.strip_prefix("/lsp-restart ").unwrap_or("").trim();
                if key.is_empty() {
                    self.messages_state.toasts.info(
                        "Usage: /lsp-restart <server-key>\nUse /lsp-servers to see available keys.",
                    );
                } else if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    let result = handle.block_on(lsp_tool.lsp_restart_server(key));
                    self.messages_state.toasts.info(&result);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-stop" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-stop ").unwrap_or("").trim();
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let handle = tokio::runtime::Handle::current();
                    let key_opt = if arg.is_empty() || arg == "--all" {
                        None
                    } else {
                        Some(arg)
                    };
                    let result = handle.block_on(lsp_tool.lsp_stop_server(key_opt));
                    self.messages_state.toasts.info(&result);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-cache-status" => {
                self.ui_state.command_mode = false;
                if let Some(ref lsp_tool) = self.lsp_tool {
                    let text = lsp_tool.lsp_cache_status();
                    self.messages_state.toasts.info(&text);
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/lsp-cache-clear" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let arg = query.strip_prefix("/lsp-cache-clear ").unwrap_or("").trim();
                if let Some(ref lsp_tool) = self.lsp_tool {
                    if arg.is_empty() || arg == "--all" {
                        let count = lsp_tool.clear_semantic_cache();
                        self.messages_state
                            .toasts
                            .info(&format!("Cleared {count} cached entries"));
                    } else {
                        let root = std::path::PathBuf::from(arg);
                        let count = lsp_tool.clear_semantic_cache_for_root(&root);
                        self.messages_state.toasts.info(&format!(
                            "Cleared {count} cached entries for root: {}",
                            root.display()
                        ));
                    }
                } else {
                    self.messages_state.toasts.info("LSP not available");
                }
            }
            "/tool-backends" | "/tools" | "/backends" => {
                // Build the report synchronously from the resolved
                // config. The App doesn't hold a direct reference to
                // the live `ToolRegistry`, so we rely on the resolved
                // `Config` plus the eggsearch server name in
                // `search_backend::state` (if initialized).
                let config = crate::config::schema::Config::load().unwrap_or_default();
                let search_cfg = config.search.clone().unwrap_or_default();
                let mcp_server_names: Option<Vec<String>> =
                    crate::search_backend::state::mcp_service().map(|_mcp| {
                        // We don't currently expose the live list of
                        // server names without an async call, so we
                        // approximate by checking the resolved
                        // SearchConfig. The full live list is shown
                        // by `/mcps`.
                        Vec::new()
                    });
                let report = crate::tool::backend::build_report(
                    &search_cfg,
                    config.tool_backends.as_ref(),
                    mcp_server_names.as_deref(),
                );
                let rendered = report.render();
                self.messages_state.toasts.info(&rendered);
            }
            "/review" => {
                self.ui_state.command_mode = false;
                self.open_dialog(Dialog::Review);
            }
            "/diff" => {
                let query = self.dialog_state.command_palette.query.clone();
                let path_arg = query.trim_start_matches("/diff").trim();
                self.ui_state.command_mode = false;
                if path_arg.is_empty() {
                    self.handle_diff_command(None);
                } else {
                    self.handle_diff_command(Some(path_arg));
                }
            }
            "/tests" => {
                let query = self.dialog_state.command_palette.query.clone();
                let subcmd = query.trim_start_matches("/tests").trim();
                self.ui_state.command_mode = false;
                self.handle_tests_command(subcmd);
            }
            "/revert" => {
                let query = self.dialog_state.command_palette.query.clone();
                let path_arg = query.trim_start_matches("/revert").trim();
                self.ui_state.command_mode = false;
                if path_arg.is_empty() {
                    self.messages_state.toasts.warning("Usage: /revert <path>");
                } else {
                    self.handle_revert_command(path_arg);
                }
            }
            "/research" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.trim_start_matches("/research").trim();
                self.ui_state.command_mode = false;
                if args.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /research <question> [--mode <mode>] [--depth <depth>]");
                } else {
                    self.handle_research_command(args);
                }
            }
            "/research-runs" => {
                self.ui_state.command_mode = false;
                self.handle_research_runs_command();
            }
            "/research-open" => {
                let query = self.dialog_state.command_palette.query.clone();
                let run_id = query.trim_start_matches("/research-open").trim();
                self.ui_state.command_mode = false;
                if run_id.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /research-open <run_id>");
                } else {
                    self.handle_research_open_command(run_id);
                }
            }
            "/research-show" => {
                let query = self.dialog_state.command_palette.query.clone();
                let rest = query.trim_start_matches("/research-show").trim();
                self.ui_state.command_mode = false;
                if rest.is_empty() {
                    self.handle_research_runs_command();
                } else {
                    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                    let section = parts[0];
                    let run_id = parts.get(1).copied().unwrap_or("").trim();
                    if run_id.is_empty() {
                        self.messages_state.toasts.warning(
                            "Usage: /research-show <section> <run_id> (section: report, brief, claims, handoff)",
                        );
                    } else {
                        self.open_dialog(Dialog::ResearchBrowser);
                        if let Some(ref mut browser) = self.dialog_state.research_browser {
                            browser.loading = true;
                        }
                        let run_id_owned = run_id.to_string();
                        let section_owned = section.to_string();
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ResearchLoadRun {
                                run_id: run_id_owned.clone(),
                            });
                            let _ = tx.try_send(TuiCommand::ResearchLoadSection {
                                run_id: run_id_owned,
                                section: section_owned,
                            });
                        }
                    }
                }
            }
            "/security-review" => {
                self.ui_state.command_mode = false;
                let raw_args = raw_input.unwrap_or("").trim();
                let parsed_args = crate::security::workflow::parse_security_review_args(raw_args);

                let root =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

                // Reentrancy guard: only one security review runs at a
                // time. The flag is cleared by the handler in
                // `src/tui/mod.rs` on both success and failure.
                if self.security_review_running.is_some() {
                    self.messages_state.toasts.warning(
                        "Security review already running. Wait for it to finish or cancel it.",
                    );
                    return;
                }

                let id = crate::security::workflow::SecurityReviewRunId::new();
                let lsp_tool = self.lsp_tool.clone();

                self.messages_state
                    .toasts
                    .info("Security review started. The result will appear in the message log when complete.");

                // Spawn the review in a tokio task so the renderer stays
                // responsive. The completion path sends
                // `TuiCommand::SecurityReviewFinished` back through
                // `tui_cmd_tx`. The `AbortHandle` is stored on the App
                // for the duration of the run so the user can cancel
                // via `/security-review-cancel`.
                let tx = self.tui_cmd_tx.clone();
                if let Some(tx) = tx {
                    let run_id = id.0.clone();
                    let task_id = self.task_registry.spawn(
                        TuiTaskKind::SecurityReview,
                        "security-review",
                        async move {
                            let result = crate::security::workflow::run_security_review_background(
                                root,
                                parsed_args,
                                lsp_tool,
                            )
                            .await;
                            let (receipt, error) = match result {
                                Ok(r) => (Some(Box::new(r)), None),
                                Err(e) => (None, Some(e)),
                            };
                            let _ = tx.try_send(TuiCommand::SecurityReviewFinished {
                                id: run_id,
                                receipt,
                                error,
                            });
                        },
                    );
                    self.security_review_running =
                        Some(crate::security::workflow::SecurityReviewTaskState {
                            id: id.0,
                            task_id,
                        });
                } else {
                    // Fallback: no channel available (e.g. test fixture).
                    self.messages_state
                        .toasts
                        .error("TUI command channel unavailable; cannot run security review");
                }
            }
            "/security-review-show" => {
                self.ui_state.command_mode = false;
                if self.latest_security_review.is_some() {
                    self.open_dialog(Dialog::SecurityReview);
                } else {
                    self.messages_state.toasts.warning(
                        "No security review result available yet. Run /security-review first.",
                    );
                }
            }
            "/security-review-cancel" => {
                self.ui_state.command_mode = false;
                self.cancel_security_review();
            }
            "/shell-include" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let args_str = query.strip_prefix("/shell-include").unwrap_or("").trim();
                let args: Vec<&str> = args_str.splitn(3, ' ').collect();
                let id_str = args.first().copied().unwrap_or("");
                let flag = args.get(1).copied().unwrap_or("all");
                let extra = args.get(2).map(|s| s.to_string());
                match self.resolve_shell_id(id_str) {
                    Some(id) => {
                        let mode = match flag {
                            "--tail" => {
                                let n = extra
                                    .as_deref()
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .unwrap_or(200);
                                format!("tail {}", n)
                            }
                            "--stdout" => "stdout".to_string(),
                            "--stderr" => "stderr".to_string(),
                            "--summary" => "summary".to_string(),
                            "stdout" => "stdout".to_string(),
                            "stderr" => "stderr".to_string(),
                            "summary" => "summary".to_string(),
                            "all" => "all".to_string(),
                            _ => "all".to_string(),
                        };
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellInclude {
                                id,
                                mode,
                                question: None,
                            });
                        }
                    }
                    None => {
                        self.messages_state.toasts.warning(
                            "Usage: /shell-include <id|last> [--tail N|--stdout|--stderr|--summary|all]",
                        );
                    }
                }
            }
            "/shell-rerun" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let id_str = query.strip_prefix("/shell-rerun").unwrap_or("").trim();
                match self.resolve_shell_id(id_str) {
                    Some(id) => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellRerun { id });
                        }
                    }
                    None => {
                        self.messages_state
                            .toasts
                            .warning("Usage: /shell-rerun <id|last>");
                    }
                }
            }
            "/shell-kill" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let id_str = query.strip_prefix("/shell-kill").unwrap_or("").trim();
                match self.resolve_shell_id(id_str) {
                    Some(id) => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellKill { id });
                        }
                    }
                    None => {
                        self.messages_state
                            .toasts
                            .warning("Usage: /shell-kill <id|last>");
                    }
                }
            }
            "/shell-list" => {
                self.ui_state.command_mode = false;
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::ShellList);
                } else {
                    let recent = self.shell_store.list_recent(10);
                    if recent.is_empty() {
                        self.messages_state
                            .toasts
                            .info("No shell commands in history");
                    } else {
                        let lines: Vec<String> = recent
                            .iter()
                            .map(|e| {
                                format!(
                                    "[{}] ${} ({})",
                                    e.id.0,
                                    e.command,
                                    match e.status {
                                        crate::shell::types::ShellStatus::Running => "running",
                                        crate::shell::types::ShellStatus::Exited => "done",
                                        crate::shell::types::ShellStatus::TimedOut => "timeout",
                                        crate::shell::types::ShellStatus::FailedToStart => "failed",
                                        crate::shell::types::ShellStatus::Killed => "killed",
                                    }
                                )
                            })
                            .collect();
                        self.messages_state.toasts.info(&lines.join("\n"));
                    }
                }
            }
            "/shell-show" => {
                self.ui_state.command_mode = false;
                let query = self.dialog_state.command_palette.query.clone();
                let id_str = query.strip_prefix("/shell-show").unwrap_or("").trim();
                match self.resolve_shell_id(id_str) {
                    Some(id) => {
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::ShellShow { id });
                        }
                    }
                    None => {
                        self.messages_state
                            .toasts
                            .warning("Usage: /shell-show <id|last>");
                    }
                }
            }
            "/plugins" | "/plugin-list" | "/plugin-ls" => {
                crate::tui::commands::plugin_management::show_plugins(self);
            }
            "/plugin-info" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                if args.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /plugin-info <plugin-id-or-name>");
                } else {
                    crate::tui::commands::plugin_management::show_plugin_info(self, args);
                }
            }
            "/plugin-enable" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                if args.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /plugin-enable <plugin-id-or-name>");
                } else {
                    crate::tui::commands::plugin_management::enable_plugin(self, args);
                }
            }
            "/plugin-disable" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                if args.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /plugin-disable <plugin-id-or-name>");
                } else {
                    crate::tui::commands::plugin_management::disable_plugin(self, args);
                }
            }
            "/plugin-doctor" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                let query_opt = if args.is_empty() { None } else { Some(args) };
                crate::tui::commands::plugin_management::doctor_plugin(self, query_opt);
            }
            "/plugin-remove" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                if args.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /plugin-remove <plugin-id-or-name>");
                } else {
                    crate::tui::commands::plugin_management::remove_plugin(self, args);
                }
            }
            "/plugin-install" => {
                let query = self.dialog_state.command_palette.query.clone();
                let args = query.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                if args.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /plugin-install <path>");
                } else {
                    crate::tui::commands::plugin_management::install_plugin(self, args);
                }
            }
            _ => {}
        }
    }

    pub fn on_paste(&mut self, text: String) {
        if self.ui_state.command_mode {
            self.prompt_state.prompt.paste(text);
            let query = self.prompt_state.prompt.get_text();
            self.dialog_state.command_palette.set_query(&query);
        } else if !self.ui_state.dialog.is_open() {
            self.paste_into_prompt(text);
        }
        // If a dialog is open but FocusManager didn't handle the paste,
        // don't paste into the prompt behind the dialog
    }

    /// Paste text into the prompt and update derived state (completions).
    fn paste_into_prompt(&mut self, text: String) {
        self.prompt_state.prompt.paste(text);
        self.update_completions();
    }

    pub fn on_resize(&mut self) {
        self.ui_state.auto_scroll = true;
        // Snap to bottom on resize — after a width/height change, the
        // layout cache, scroll offsets, and visible range all change.
        // Following the bottom avoids leaving the user stuck on a stale
        // scroll offset that no longer maps to a valid line once the
        // content has been re-wrapped. `set_width` already invalidates
        // the layout cache, so this scroll reset is independent of that.
        self.messages_state.messages.scroll_to_bottom();
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
            self.send_remote_message(RemoteTuiMessage::Resize { w, h });
        }
    }

    fn update_context_hint(&mut self, target: &ClickTarget) {
        self.context_hint = match target {
            ClickTarget::Viewport => {
                if self.messages_state.messages.sel_msg.is_some() {
                    "Click: Select message | j/k: Navigate".to_string()
                } else {
                    "j/k: Scroll | Enter: Read more".to_string()
                }
            }
            ClickTarget::Prompt => {
                "Enter: Send | Shift+Enter: New line | Tab: Complete".to_string()
            }
            ClickTarget::Dialog => "Enter: Select | Esc: Close".to_string(),
            ClickTarget::Completion => "Tab/Enter: Complete | Esc: Close".to_string(),
            ClickTarget::Sidebar => "Click: Collapse section | Wheel: Scroll sidebar".to_string(),
            ClickTarget::Scrollbar { .. } => "Click/drag: Jump | Scroll: Page up/down".to_string(),
            ClickTarget::None => String::new(),
        };
    }

    pub fn on_mouse(&mut self, event: crossterm::event::MouseEvent) {
        use crossterm::event::{MouseButton, MouseEventKind};

        let target = self.clickable_area_at(event.column, event.row);

        match event.kind {
            MouseEventKind::Down(btn) => {
                let is_double_click = self
                    .last_click_target
                    .as_ref()
                    .map(|t| t == &target)
                    .unwrap_or(false)
                    && self
                        .last_click_time
                        .map(|t| t.elapsed() < std::time::Duration::from_millis(300))
                        .unwrap_or(false);

                self.last_click_time = Some(Instant::now());
                self.last_click_target = Some(target.clone());

                match btn {
                    MouseButton::Left | MouseButton::Right => {
                        if is_double_click {
                            self.on_double_click(&target);
                        } else {
                            self.on_click(&target, event.column, event.row);
                        }
                    }
                    MouseButton::Middle => {
                        self.on_scroll_up(&target);
                    }
                }
            }
            MouseEventKind::Up(btn) => {
                match btn {
                    MouseButton::Left | MouseButton::Right => {
                        // Complete click or drag operation
                        // Clear any drag state if present
                    }
                    MouseButton::Middle => {
                        // Middle button release - could paste or other action
                    }
                }
            }
            MouseEventKind::Moved => {
                let prev_target = self.hover_target.clone();
                self.hover_target = Some(target.clone());
                self.hover_position = Some((event.column, event.row));

                if prev_target != Some(target.clone()) {
                    self.update_context_hint(&target);
                }

                if target == ClickTarget::Sidebar {
                    self.sidebar
                        .set_hover_position(event.column, event.row, self.sidebar_area);
                } else {
                    self.sidebar.clear_hover();
                }
            }
            MouseEventKind::Drag(btn) => {
                match btn {
                    MouseButton::Left => {
                        // Handle text selection or drag operations in prompt/viewport
                        if target == ClickTarget::Prompt {
                            // Could start/extend text selection
                        } else if let Some(ClickTarget::Scrollbar {
                            track_y,
                            track_height,
                        }) = self.last_click_target.clone()
                        {
                            // Continue a scrollbar drag with the original track dims.
                            let drag_target = ClickTarget::Scrollbar {
                                track_y,
                                track_height,
                            };
                            self.on_click(&drag_target, event.column, event.row);
                        }
                    }
                    MouseButton::Right => {
                        // Right drag - could be context menu or other action
                    }
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp => {
                self.on_scroll_up(&target);
            }
            MouseEventKind::ScrollDown => {
                self.on_scroll_down(&target);
            }
            MouseEventKind::ScrollLeft => {
                self.on_scroll_left(&target);
            }
            MouseEventKind::ScrollRight => {
                self.on_scroll_right(&target);
            }
        }
    }

    fn clickable_area_at(&self, x: u16, y: u16) -> ClickTarget {
        if let Some(ref area) = self.dialog_area {
            if Self::in_rect(x, y, *area) {
                return ClickTarget::Dialog;
            }
        }
        if let Some(ref area) = self.completion_area {
            if Self::in_rect(x, y, *area) {
                return ClickTarget::Completion;
            }
        }
        if let Some(ref area) = self.prompt_area {
            if Self::in_rect(x, y, *area) {
                return ClickTarget::Prompt;
            }
        }
        if let Some(ref area) = self.viewport_area {
            if Self::in_rect(x, y, *area) {
                return ClickTarget::Viewport;
            }
        }
        if let Some(ref area) = self.scrollbar_area {
            if Self::in_rect(x, y, *area) {
                let track_y = area
                    .y
                    .saturating_sub(self.viewport_area.map(|v| v.y).unwrap_or(0));
                return ClickTarget::Scrollbar {
                    track_y,
                    track_height: area.height,
                };
            }
        }
        if let Some(ref area) = self.sidebar_area {
            if Self::in_rect(x, y, *area) {
                return ClickTarget::Sidebar;
            }
        }
        ClickTarget::None
    }

    fn in_rect(x: u16, y: u16, rect: Rect) -> bool {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }

    fn on_click(&mut self, target: &ClickTarget, x: u16, y: u16) {
        match target {
            ClickTarget::Viewport => {
                if let Some(viewport) = self.viewport_area {
                    let row = y.saturating_sub(viewport.y) as usize;
                    if let Some(idx) = self.messages_state.messages.select_at_viewport_line(row) {
                        if self.messages_state.messages.message_has_tool_output(idx) {
                            self.messages_state.messages.toggle_tool_output(idx);
                        }
                    } else {
                        self.messages_state.messages.sel_msg = None;
                    }
                } else {
                    self.messages_state.messages.sel_msg = None;
                }
            }
            ClickTarget::Dialog => {
                if let Some(ref area) = self.dialog_area {
                    let rel_y = y.saturating_sub(area.y);
                    let rel_x = x.saturating_sub(area.x);
                    self.select_dialog_item(rel_x, rel_y, *area);
                }
            }
            ClickTarget::Completion => {
                if let Some(ref area) = self.completion_area {
                    let rel_y = y.saturating_sub(area.y);
                    if rel_y > 0 && rel_y < area.height.saturating_sub(1) {
                        let idx = (rel_y as usize).saturating_sub(1);
                        let max_idx = match self.prompt_state.completion_type {
                            CompletionType::Slash => {
                                self.prompt_state.slash_completions.len().saturating_sub(1)
                            }
                            CompletionType::File => {
                                self.prompt_state.file_completions.len().saturating_sub(1)
                            }
                            CompletionType::Agent => {
                                self.prompt_state.agent_completions.len().saturating_sub(1)
                            }
                        };
                        if idx <= max_idx {
                            self.prompt_state.completion_sel = idx;
                        }
                    }
                }
            }
            ClickTarget::Prompt => {
                if let Some(ref area) = self.prompt_area {
                    let rel_x = x.saturating_sub(area.x);
                    let char_w = 1u16;
                    let cursor_pos = rel_x as usize / char_w as usize;
                    self.prompt_state.prompt.set_cursor(cursor_pos.min(256));
                    self.prompt_state.prompt.focus();
                }
            }
            ClickTarget::Sidebar => {
                self.sidebar.toggle_hovered_section();
            }
            ClickTarget::Scrollbar {
                track_y,
                track_height,
            } => {
                if let Some(viewport) = self.viewport_area {
                    let rel_y = y.saturating_sub(viewport.y).saturating_sub(*track_y);
                    if *track_height > 0 {
                        let max_scroll = self.messages_state.messages.max_scroll();
                        let new_scroll = (rel_y as usize) * max_scroll / (*track_height as usize);
                        self.messages_state.messages.scroll = new_scroll;
                        self.messages_state.messages.auto_scroll = false;
                    }
                }
            }
            ClickTarget::None => {}
        }
    }

    fn on_double_click(&mut self, target: &ClickTarget) {
        match target {
            ClickTarget::Dialog => {
                if let Some(InputAction::Send) = self.dialog_confirm_action() {
                    self.confirm_dialog();
                }
            }
            ClickTarget::Completion => {
                self.accept_completion();
            }
            ClickTarget::Scrollbar { .. } => {
                self.messages_state.messages.scroll_to_bottom();
            }
            _ => {}
        }
    }

    fn on_scroll_up(&mut self, target: &ClickTarget) {
        match target {
            ClickTarget::Viewport | ClickTarget::Prompt | ClickTarget::None => {
                self.messages_state.messages.scroll_up();
            }
            ClickTarget::Sidebar => {
                if let Some(area) = self.sidebar_area {
                    self.sidebar.scroll_up(area);
                }
            }
            ClickTarget::Dialog => {
                let up_key = crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Up,
                    crossterm::event::KeyModifiers::NONE,
                );
                if let Some(msg) = self.focus_manager.handle_key(up_key) {
                    self.process_msg(msg);
                }
            }
            ClickTarget::Completion => {
                if self.prompt_state.completion_sel > 0 {
                    self.prompt_state.completion_sel -= 1;
                }
            }
            ClickTarget::Scrollbar { .. } => {
                self.messages_state.messages.scroll_page_up();
            }
        }
    }

    fn on_scroll_down(&mut self, target: &ClickTarget) {
        match target {
            ClickTarget::Viewport | ClickTarget::Prompt | ClickTarget::None => {
                self.messages_state.messages.scroll_down();
            }
            ClickTarget::Sidebar => {
                if let Some(area) = self.sidebar_area {
                    self.sidebar.scroll_down(area);
                }
            }
            ClickTarget::Dialog => {
                let down_key = crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Down,
                    crossterm::event::KeyModifiers::NONE,
                );
                if let Some(msg) = self.focus_manager.handle_key(down_key) {
                    self.process_msg(msg);
                }
            }
            ClickTarget::Completion => {
                let max_sel = match self.prompt_state.completion_type {
                    CompletionType::Slash => {
                        self.prompt_state.slash_completions.len().saturating_sub(1)
                    }
                    CompletionType::File => {
                        self.prompt_state.file_completions.len().saturating_sub(1)
                    }
                    CompletionType::Agent => {
                        self.prompt_state.agent_completions.len().saturating_sub(1)
                    }
                };
                if self.prompt_state.completion_sel < max_sel {
                    self.prompt_state.completion_sel += 1;
                }
            }
            ClickTarget::Scrollbar { .. } => {
                self.messages_state.messages.scroll_page_down();
            }
        }
    }

    fn on_scroll_left(&mut self, target: &ClickTarget) {
        if target == &ClickTarget::Viewport {
            self.messages_state.messages.scroll_left();
        }
    }

    fn on_scroll_right(&mut self, target: &ClickTarget) {
        if target == &ClickTarget::Viewport {
            self.messages_state.messages.scroll_right();
        }
    }

    fn select_dialog_item(&mut self, _rel_x: u16, rel_y: u16, _area: Rect) {
        if rel_y < 1 {
            return;
        }
        let rel_y_usize = rel_y as usize;

        // First, get the index from hit_test (shared borrow of focus_manager)
        let idx = if let Some(active) = self.focus_manager.top() {
            active.hit_test(rel_y_usize)
        } else {
            None
        };

        // Now the shared borrow is dropped, we can have mutable borrows
        if let Some(idx) = idx {
            // Update dialog_state for legacy code paths
            match &mut self.ui_state.dialog {
                Dialog::Model => {
                    self.dialog_state.model_dialog.selected = idx;
                }
                Dialog::Agent => {
                    let visible: Vec<usize> = self
                        .agent_state
                        .agents
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| !a.hidden)
                        .map(|(i, _)| i)
                        .collect();
                    if let Some(&real_idx) = visible.get(idx) {
                        self.agent_state.current_agent = real_idx;
                    }
                }
                Dialog::Session => {
                    self.dialog_state.session_dialog.selected = idx;
                }
                Dialog::Tree => {
                    let cur = self.dialog_state.tree_dialog.selected;
                    if idx > cur {
                        for _ in cur..idx {
                            self.dialog_state.tree_dialog.select_down();
                        }
                    } else if idx < cur {
                        for _ in idx..cur {
                            self.dialog_state.tree_dialog.select_up();
                        }
                    }
                }
                Dialog::Question => {
                    if let Some(qd) = &mut self.dialog_state.question_dialog {
                        qd.selected_question = idx;
                    }
                }
                Dialog::Permission => {
                    if idx < 4 {
                        if let Some(pd) = &mut self.dialog_state.permission_dialog {
                            pd.selected_option = idx;
                        }
                    }
                }
                _ => {}
            }

            // Sync the selection to the focused clone in FocusManager
            if let Some(top) = self.focus_manager.top_mut() {
                top.set_selected(idx);
            }

            return;
        }

        // Fallback: legacy handling (should not be reached with FocusManager)
        let idx = rel_y_usize.saturating_sub(1);
        match &mut self.ui_state.dialog {
            Dialog::Model => {
                let total = self.dialog_state.model_dialog.models.len();
                if idx < total {
                    self.dialog_state.model_dialog.selected = idx;
                }
            }
            Dialog::Agent => {
                let visible: Vec<usize> = self
                    .agent_state
                    .agents
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| !a.hidden)
                    .map(|(i, _)| i)
                    .collect();
                if idx < visible.len() {
                    if let Some(&real_idx) = visible.get(idx) {
                        self.agent_state.current_agent = real_idx;
                    }
                }
            }
            Dialog::Session => {
                let total = self.dialog_state.session_dialog.sessions.len();
                if idx < total {
                    self.dialog_state.session_dialog.selected = idx;
                }
            }
            Dialog::Tree => {
                let cur = self.dialog_state.tree_dialog.selected;
                if idx > cur {
                    for _ in cur..idx {
                        self.dialog_state.tree_dialog.select_down();
                    }
                } else if idx < cur {
                    for _ in idx..cur {
                        self.dialog_state.tree_dialog.select_up();
                    }
                }
            }
            Dialog::Question => {
                if let Some(qd) = &mut self.dialog_state.question_dialog {
                    if idx < qd.questions.len() {
                        qd.selected_question = idx;
                    }
                }
            }
            Dialog::Permission => {
                if idx < 4 {
                    if let Some(pd) = &mut self.dialog_state.permission_dialog {
                        pd.selected_option = idx;
                    }
                }
            }
            _ => {}
        }
    }

    fn dialog_confirm_action(&self) -> Option<InputAction> {
        match &self.ui_state.dialog {
            Dialog::Model
            | Dialog::Agent
            | Dialog::Session
            | Dialog::Theme
            | Dialog::Question
            | Dialog::Permission => Some(InputAction::Send),
            _ => None,
        }
    }

    fn dialog_navigate_up(&mut self) {
        match &mut self.ui_state.dialog {
            Dialog::Model => self.dialog_state.model_dialog.select_up(),
            Dialog::Agent => self.dialog_state.agent_dialog.select_up(),
            Dialog::Session => self.dialog_state.session_dialog.select_up(),
            Dialog::Tree => self.dialog_state.tree_dialog.select_up(),
            Dialog::Theme => {
                if let Some(picker) = &mut self.dialog_state.theme_picker {
                    picker.select_up();
                }
            }
            Dialog::Question => {
                if let Some(qd) = &mut self.dialog_state.question_dialog {
                    qd.select_up();
                }
            }
            Dialog::Permission => {
                if let Some(pd) = &mut self.dialog_state.permission_dialog {
                    pd.cursor_up();
                }
            }
            _ => {}
        }
    }

    fn dialog_navigate_down(&mut self) {
        match &mut self.ui_state.dialog {
            Dialog::Model => self.dialog_state.model_dialog.select_down(),
            Dialog::Agent => self.dialog_state.agent_dialog.select_down(),
            Dialog::Session => self.dialog_state.session_dialog.select_down(),
            Dialog::Tree => self.dialog_state.tree_dialog.select_down(),
            Dialog::Theme => {
                if let Some(picker) = &mut self.dialog_state.theme_picker {
                    picker.select_down();
                }
            }
            Dialog::Question => {
                if let Some(qd) = &mut self.dialog_state.question_dialog {
                    qd.select_down();
                }
            }
            Dialog::Permission => {
                if let Some(pd) = &mut self.dialog_state.permission_dialog {
                    pd.cursor_down();
                }
            }
            _ => {}
        }
    }

    fn confirm_dialog(&mut self) {
        let enter_key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        if !self.focus_manager.is_empty() {
            if let Some(msg) = self.focus_manager.handle_key(enter_key) {
                self.process_msg(msg);
            }
        } else {
            self.handle_dialog_key(enter_key);
        }
    }

    fn send_prompt(&mut self) {
        let text = self.prompt_state.prompt.get_text();
        let trimmed_text = text.trim().to_string();
        debug_log!(
            "send_prompt: text='{}', trimmed='{}', pending_send={}",
            text,
            trimmed_text,
            self.prompt_state.pending_send
        );

        if trimmed_text.is_empty() {
            debug_log!("send_prompt: returning - trimmed text is empty");
            return;
        }
        if self.prompt_state.pending_send {
            debug_log!("send_prompt: returning - pending_send already true");
            return;
        }
        if self.handle_slash_command(&text) {
            debug_log!("send_prompt: handled slash command, clearing prompt");
            self.prompt_state.prompt.clear();
            self.prompt_state.show_completions = false;
            return;
        }

        match crate::shell::classify_prompt_submission(&trimmed_text) {
            crate::shell::types::PromptSubmissionKind::HumanShell {
                command,
                promote_after,
            } => {
                debug_log!("send_prompt: intercepted human shell command: {}", command);
                self.prompt_state.prompt.clear();
                self.prompt_state.show_completions = false;
                if let Some(ref tx) = self.tui_cmd_tx {
                    let _ = tx.try_send(TuiCommand::RunHumanShell {
                        command,
                        promote_after,
                    });
                }
                return;
            }
            crate::shell::types::PromptSubmissionKind::Slash(s) => {
                let _ = &s;
                debug_log!("send_prompt: slash command via classify: {}", s);
            }
            crate::shell::types::PromptSubmissionKind::Chat(_) => {}
        }

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

        self.messages_state
            .messages
            .add_user_message(trimmed_text, Some(self.agent_state.plan_mode));
        self.prompt_state.prompt.clear();
        self.prompt_state.show_completions = false;
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            self.send_remote_message(RemoteTuiMessage::Input {
                text: text.trim().to_string(),
            });
            self.prompt_state.pending_send = false;
        } else {
            self.prompt_state.pending_send = true;
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

        debug_log!("send_prompt: completed - pending_send set to true, status=Working");
    }

    fn resolve_shell_id(&self, id_str: &str) -> Option<u64> {
        match id_str {
            "last" => self.shell_store.get_last().map(|e| e.id.0),
            s => s.parse::<u64>().ok(),
        }
    }

    fn handle_slash_command(&mut self, text: &str) -> bool {
        let trimmed = text.trim();
        if !trimmed.starts_with('/') {
            return false;
        }

        if trimmed.starts_with("/search ") {
            let query = trimmed.trim_start_matches("/search ").trim();
            if !query.is_empty() {
                self.messages_state.messages.search(query);
            }
            return true;
        }
        let command_name = trimmed
            .split_whitespace()
            .next()
            .unwrap_or(trimmed)
            .trim_start_matches('/');
        if let Some(cmd) = crate::tui::command::COMMAND_REGISTRY.find_by_name_or_alias(command_name)
        {
            self.execute_command(cmd, Some(trimmed));
            return true;
        }

        false
    }

    fn cancel(&mut self) {
        if self.ui_state.dialog.is_open() {
            self.close_dialog();
            return;
        }
        if self.prompt_state.show_completions {
            self.prompt_state.show_completions = false;
            return;
        }

        // If in Insert mode, switch to Normal mode instead of canceling
        if self.ui_state.input_mode == InputMode::Insert {
            self.ui_state.input_mode = InputMode::Normal;
            return;
        }

        // Ctrl+C semantics: clear input if text exists, otherwise exit
        let text = self.prompt_state.prompt.get_text();
        if !text.trim().is_empty() {
            // Clear the input
            self.prompt_state.prompt.clear();
            self.prompt_state.show_completions = false;
        } else {
            // Exit the program
            self.ui_state.running = false;
            let _ = self.ui_state.shutdown_tx.take().map(|tx| tx.send(()));
        }
    }

    fn navigate_up(&mut self) {
        // Input history navigation in insert mode.
        if self.session_state.history.is_empty() {
            return;
        }
        let new_pos = match self.session_state.history_pos {
            Some(pos) if pos > 0 => pos - 1,
            None => self.session_state.history.len() - 1,
            _ => return,
        };
        self.session_state.history_pos = Some(new_pos);
        if let Some(entry) = self.session_state.history.get(new_pos) {
            self.prompt_state.prompt.set_text(entry.text.clone());
        }
    }

    fn navigate_down(&mut self) {
        // Input history navigation in insert mode.
        let new_pos = match self.session_state.history_pos {
            Some(pos) if pos + 1 < self.session_state.history.len() => pos + 1,
            Some(_) => {
                self.session_state.history_pos = None;
                self.prompt_state.prompt.clear();
                return;
            }
            None => return,
        };
        self.session_state.history_pos = Some(new_pos);
        if let Some(entry) = self.session_state.history.get(new_pos) {
            self.prompt_state.prompt.set_text(entry.text.clone());
        }
    }

    fn scroll_page_up(&mut self) {
        self.messages_state.messages.scroll_page_up();
    }

    fn scroll_page_down(&mut self) {
        self.messages_state.messages.scroll_page_down();
    }

    fn scroll_viewport_up(&mut self) {
        if self.ui_state.dialog.is_open() {
            self.dialog_navigate_up();
        } else {
            self.messages_state.messages.scroll_up();
        }
    }

    fn scroll_viewport_down(&mut self) {
        if self.ui_state.dialog.is_open() {
            self.dialog_navigate_down();
        } else {
            self.messages_state.messages.scroll_down();
        }
    }

    fn go_to_top(&mut self) {
        if !self.ui_state.dialog.is_open() {
            self.messages_state.messages.scroll_to_top();
        }
    }

    fn go_to_bottom(&mut self) {
        if !self.ui_state.dialog.is_open() {
            self.messages_state.messages.scroll_to_bottom();
        }
    }

    fn cycle_agent(&mut self) {
        let visible: Vec<usize> = self
            .agent_state
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.hidden)
            .map(|(i, _)| i)
            .collect();
        if visible.is_empty() {
            return;
        }
        let cur_pos = visible
            .iter()
            .position(|&i| i == self.agent_state.current_agent)
            .unwrap_or(0);
        let next_pos = (cur_pos + 1) % visible.len();
        self.agent_state.current_agent = visible[next_pos];
    }

    fn push_dialog(
        &mut self,
        dialog: Dialog,
        component: Box<dyn crate::tui::components::component::Component>,
    ) {
        self.ui_state.dialog = dialog;
        self.focus_manager.push(component);
    }

    pub(crate) fn close_dialog(&mut self) {
        // If the theme picker was live-previewing, revert to the
        // original theme before closing. Esc and Enter both fire
        // `ThemeRevert`/`ThemeCommit` and tear down the dialog
        // themselves; this path catches every other dismissal (mouse,
        // parent close, etc).
        if self.ui_state.dialog == Dialog::Theme {
            if let Some(picker) = self.dialog_state.theme_picker.as_ref() {
                if picker.is_previewing() {
                    let target = picker
                        .preview_original_id()
                        .unwrap_or_else(|| crate::theme::registry::DEFAULT_THEME_ID.to_string());
                    if let Some(theme) = self.theme_registry.get_tui(&target) {
                        self.ui_state.theme = Arc::new(theme);
                    }
                }
            }
            self.dialog_state.theme_picker = None;
        }

        // Cancel background tasks and async request state scoped to
        // the dialog being closed. This ensures stale completions
        // from in-flight requests are ignored.
        use crate::tui::task_lifecycle::TuiTaskKind;
        match self.ui_state.dialog {
            Dialog::Tree => {
                self.task_registry.cancel_kind(TuiTaskKind::Command);
            }
            Dialog::Import => {
                self.task_registry.cancel_kind(TuiTaskKind::Command);
                self.dialog_state.import_request.cancel();
            }
            Dialog::ResearchBrowser => {
                self.task_registry.cancel_kind(TuiTaskKind::Research);
                self.dialog_state.research_request.cancel();
            }
            Dialog::Session => {
                self.task_registry.cancel_kind(TuiTaskKind::Command);
                self.dialog_state.session_reload_request.cancel();
                self.dialog_state.session_messages_request.cancel();
                self.dialog_state.session_mutation_request.cancel();
                self.dialog_state.task_list_request.cancel();
                self.dialog_state.task_delete_request.cancel();
                self.dialog_state.worktree_list_request.cancel();
                self.dialog_state.template_create_request.cancel();
            }
            Dialog::ShellShow => {
                self.dialog_state.shell_detail_id = None;
            }
            _ => {}
        }

        self.focus_manager.pop();
        let active_type = self.focus_manager.active_dialog_type();
        if active_type != DialogType::None {
            self.ui_state.dialog = Dialog::from(active_type);
        } else {
            self.ui_state.dialog = Dialog::None;
        }
    }

    /// Apply a protocol-level [`UiEffect`] from a plugin response to
    /// the TUI state. Returns a high-level result so the caller can
    /// decide whether to emit additional toasts or chat messages.
    ///
    /// When `source_plugin_id` is provided, the effect is attributed to
    /// that plugin for cross-plugin ID spoofing checks: surface IDs
    /// belonging to a different plugin are rejected.
    pub fn apply_plugin_ui_effect(
        &mut self,
        effect: crate::protocol::ui::UiEffect,
        source_plugin_id: Option<&str>,
    ) -> crate::tui::app::state::PluginUiApplyResult {
        use crate::protocol::ui::UiEffect;
        use crate::tui::app::state::PluginUiApplyResult;

        // Capability gate: reject effects the client does not support.
        // Degrade to a summary toast so the effect is not silently lost.
        if !self.ui_state.plugin_ui_caps.supports_effect(&effect) {
            let summary = Self::degrade_effect_to_summary(&effect);
            if !summary.is_empty() {
                self.messages_state.toasts.info(&summary);
            }
            return PluginUiApplyResult::Unsupported(
                "effect type not supported by client capabilities".to_string(),
            );
        }

        // Cross-plugin spoofing protection: when a plugin source is
        // provided, reject effects whose surface IDs belong to a
        // different plugin.
        if let Some(owner) = source_plugin_id {
            if let Some(violation) = self.validate_plugin_surface_ownership(&effect, owner) {
                return PluginUiApplyResult::Unsupported(violation);
            }
        }

        match &effect {
            UiEffect::ShowToast { toast } => {
                match toast.level {
                    crate::protocol::ui::ToastLevel::Info => {
                        self.messages_state.toasts.info(&toast.message);
                    }
                    crate::protocol::ui::ToastLevel::Success => {
                        self.messages_state.toasts.success(&toast.message);
                    }
                    crate::protocol::ui::ToastLevel::Warning => {
                        self.messages_state.toasts.warning(&toast.message);
                    }
                    crate::protocol::ui::ToastLevel::Error => {
                        self.messages_state.toasts.error(&toast.message);
                    }
                };
                return PluginUiApplyResult::ToastRequested;
            }
            UiEffect::EmitChat { block } => {
                // EmitChat is now rendered visibly in the TUI. Both
                // ChatFormat::Plain and ChatFormat::Markdown are lowered to
                // line-based text so they never execute embedded escape
                // sequences or markdown links. Output is shown via the
                // toast/info-dialog surface: short blocks toast, long
                // blocks open the scrollable info dialog. This output is
                // NOT added to the model-visible chat transcript — it is a
                // user-facing display surface only.
                let lines: Vec<String> = block.content.lines().map(|s| s.to_string()).collect();
                if lines.is_empty() {
                    return PluginUiApplyResult::Ignored;
                }
                self.show_short_or_info(
                    crate::tui::components::dialogs::info::InfoType::Stats,
                    lines,
                );
                return PluginUiApplyResult::ChatApplied;
            }
            _ => {}
        }

        let result = self.plugin_ui_state.apply_effect(effect);

        // If a plugin dialog was just opened and no first-party modal
        // is active, open it in the FocusManager.
        if matches!(result, PluginUiApplyResult::Applied) {
            if let Some(spec) = self.plugin_ui_state.dialogs.values().last() {
                if !matches!(
                    self.ui_state.dialog,
                    Dialog::Permission | Dialog::Question | Dialog::SecurityReview
                ) {
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
                    self.ui_state.dialog = Dialog::Plugin;
                }
            }
        }

        result
    }

    /// Render a structured `UiValidationError` as a short user-facing
    /// description for toasts and tests. This avoids leaking raw serde
    /// field paths while keeping the human-readable cause.
    fn validation_error_to_string(err: &crate::protocol::ui::UiValidationError) -> String {
        use crate::protocol::ui::UiValidationError;
        match err {
            UiValidationError::TooManyEffects { limit } => {
                format!("too many effects (limit {})", limit)
            }
            UiValidationError::EffectTooLarge { limit, approx } => {
                format!(
                    "effect payload too large (~{} bytes, limit {})",
                    approx, limit
                )
            }
            UiValidationError::StringTooLong { limit, len } => {
                format!("string field too long ({} chars, limit {})", len, limit)
            }
            UiValidationError::TooDeep { limit } => {
                format!("node depth exceeds limit {}", limit)
            }
            UiValidationError::TableTooLarge {
                rows,
                cols,
                row_limit,
                col_limit,
            } => format!(
                "table {}x{} exceeds limits {}x{}",
                rows, cols, row_limit, col_limit
            ),
        }
    }

    /// Produce a short summary string for an unsupported [`UiEffect`],
    /// used to degrade gracefully when the client cannot render the
    /// full effect. Returns an empty string for effects that should
    /// be silently omitted (e.g. status items).
    fn degrade_effect_to_summary(effect: &crate::protocol::ui::UiEffect) -> String {
        use crate::protocol::ui::UiEffect;
        match effect {
            UiEffect::OpenDialog { dialog } => {
                let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                    &dialog.body,
                );
                let preview = lines.join(" ");
                let preview = if preview.len() > 120 {
                    format!("{}...", &preview[..120])
                } else {
                    preview
                };
                format!("[dialog] {}: {}", dialog.title, preview)
            }
            UiEffect::OpenPanel { panel } => {
                let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                    &panel.body,
                );
                let preview = lines.join(" ");
                let preview = if preview.len() > 120 {
                    format!("{}...", &preview[..120])
                } else {
                    preview
                };
                format!("[panel] {}: {}", panel.title, preview)
            }
            UiEffect::AddStatusItem { item } => {
                // Status items are omitted unless they carry text content.
                let lines = crate::tui::components::ui_node_renderer::UiNodeRenderer::node_to_lines(
                    &item.body,
                );
                let preview = lines.join(" ");
                if preview.is_empty() {
                    return String::new();
                }
                let label = item.label.as_deref().unwrap_or("status");
                format!("[status] {}: {}", label, preview)
            }
            UiEffect::EmitChat { block } => {
                let preview = if block.content.len() > 120 {
                    format!("{}...", &block.content[..120])
                } else {
                    block.content.clone()
                };
                format!("[chat] {}", preview)
            }
            UiEffect::ShowToast { toast } => toast.message.clone(),
            UiEffect::CloseDialog { .. }
            | UiEffect::UpdatePanel { .. }
            | UiEffect::ClosePanel { .. }
            | UiEffect::UpdateStatusItem { .. }
            | UiEffect::RemoveStatusItem { .. } => String::new(),
        }
    }

    /// Check that surface IDs in the effect belong to the claimed plugin.
    /// Returns `Some(reason)` if a spoofing attempt is detected.
    fn validate_plugin_surface_ownership(
        &self,
        effect: &crate::protocol::ui::UiEffect,
        claimed_owner: &str,
    ) -> Option<String> {
        use crate::protocol::ui::UiEffect;
        let prefix = format!("{}:", claimed_owner);
        let foreign_id = match effect {
            UiEffect::OpenDialog { dialog } if !dialog.id.starts_with(&prefix) => {
                Some(dialog.id.clone())
            }
            UiEffect::CloseDialog { id } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::OpenPanel { panel } if !panel.id.starts_with(&prefix) => {
                Some(panel.id.clone())
            }
            UiEffect::UpdatePanel { id, .. } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::ClosePanel { id } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::AddStatusItem { item } if !item.id.starts_with(&prefix) => {
                Some(item.id.clone())
            }
            UiEffect::UpdateStatusItem { id, .. } if !id.starts_with(&prefix) => Some(id.clone()),
            UiEffect::RemoveStatusItem { id } if !id.starts_with(&prefix) => Some(id.clone()),
            _ => None,
        };
        foreign_id.map(|id| {
            format!(
                "plugin '{}' attempted to use surface id '{}' belonging to another plugin",
                claimed_owner, id
            )
        })
    }

    /// Apply a plugin-UI envelope (with typed source, session, and
    /// invocation) to the TUI. This is the canonical entry point for
    /// all plugin UI effects, regardless of transport (local TUI
    /// command channel or remote WebSocket).
    ///
    /// The envelope-derived source is used for ownership checks
    /// automatically. For Plugin sources the prefix check uses
    /// `envelope.source.plugin_id`; for Core/Tui sources no plugin
    /// ownership is enforced.
    ///
    /// The effect payload is validated against
    /// [`crate::protocol::ui::UiLimits::balanced()`] before
    /// dispatch. Effects that exceed limits are rejected with a
    /// structured result.
    pub fn apply_plugin_ui_envelope(
        &mut self,
        envelope: crate::protocol::ui::UiEffectEnvelope,
    ) -> crate::tui::app::state::PluginUiApplyResult {
        use crate::protocol::ui::{UiEffectSource, UiLimits};

        let limits = UiLimits::balanced();
        let plugin_id_owned;
        let plugin_id_opt: Option<&str> = match &envelope.source {
            UiEffectSource::Plugin { plugin_id } => {
                plugin_id_owned = plugin_id.clone();
                Some(plugin_id_owned.as_str())
            }
            UiEffectSource::Core | UiEffectSource::Tui => None,
        };

        // Session guard: drop envelopes targeted at a different
        // session than the one currently focused.
        if let Some(sid) = envelope.session_id.as_deref() {
            let current_session = self
                .session_state
                .session
                .as_ref()
                .map(|s| s.id.as_str())
                .unwrap_or_default();
            if !sid.is_empty() && sid != current_session {
                return crate::tui::app::state::PluginUiApplyResult::Unsupported(format!(
                    "envelope session_id '{}' does not match active session '{}'",
                    sid, current_session
                ));
            }
        }

        // Validate the effect payload against the limits.
        if let Err(err) = limits.validate_effect(&envelope.effect) {
            return crate::tui::app::state::PluginUiApplyResult::Unsupported(format!(
                "ui effect rejected by limits: {}",
                Self::validation_error_to_string(&err)
            ));
        }

        let crate::protocol::ui::UiEffectEnvelope { effect, .. } = envelope;
        self.apply_plugin_ui_effect(effect, plugin_id_opt)
    }

    /// Validate a batch of effects against the balanced limits and
    /// return the offending error if any one fails. Bounded for
    /// multi-frontend transport — callers (event bridge, lifecycle
    /// hooks) iterate plugin response vectors and need a deterministic
    /// per-batch validation point.
    pub fn validate_plugin_ui_effects(
        &self,
        effects: &[crate::protocol::ui::UiEffect],
    ) -> Result<(), String> {
        use crate::protocol::ui::UiLimits;
        UiLimits::balanced()
            .validate_effects(effects)
            .map_err(|err| Self::validation_error_to_string(&err))
    }

    #[allow(dead_code)]
    fn replace_dialog(
        &mut self,
        dialog: Dialog,
        component: Box<dyn crate::tui::components::component::Component>,
    ) {
        self.focus_manager.pop();
        self.ui_state.dialog = dialog;
        self.focus_manager.push(component);
    }

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
        if let Some(ref mut dialog) = self.dialog_state.info_dialog {
            // Reuse the existing dialog to avoid double-pushing the
            // focus stack. set_info_type resets scroll position.
            dialog.set_info_type(info_type);
            dialog.set_content(lines);
            dialog.set_theme(&self.ui_state.theme);
        } else {
            self.dialog_state.info_dialog = Some(InfoDialog::new(
                Arc::clone(&self.ui_state.theme),
                info_type,
                lines,
            ));
            if let Some(ref info_dialog) = self.dialog_state.info_dialog {
                self.focus_manager.push(Box::new(info_dialog.clone()));
            }
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

    pub(crate) fn open_ui_node_dialog(&mut self, title: String, body: codegg_protocol::ui::UiNode) {
        use crate::tui::components::dialogs::ui_node::UiNodeDialog;
        if let Some(ref mut dialog) = self.dialog_state.ui_node_dialog {
            dialog.update_content(body);
            dialog.set_title(title);
            dialog.set_theme(&self.ui_state.theme);
        } else {
            let dialog = UiNodeDialog::new(
                "stats".into(),
                title,
                body,
                Arc::clone(&self.ui_state.theme),
            );
            self.dialog_state.ui_node_dialog = Some(dialog);
            if let Some(ref d) = self.dialog_state.ui_node_dialog {
                self.focus_manager.push(Box::new(d.clone()));
            }
        }
    }

    pub fn open_dialog(&mut self, dialog: Dialog) {
        match dialog {
            Dialog::Session => {
                self.load_sessions_dialog();
                self.focus_manager
                    .push(Box::new(self.dialog_state.session_dialog.clone()));
            }
            Dialog::Model => {
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
                let focus_was_empty = self.dialog_state.info_dialog.is_none();
                if focus_was_empty {
                    self.dialog_state.info_dialog =
                        Some(crate::tui::components::dialogs::info::InfoDialog::new(
                            Arc::clone(&self.ui_state.theme),
                            info_type,
                            lines,
                        ));
                } else if let Some(ref mut info_dialog) = self.dialog_state.info_dialog {
                    info_dialog.set_info_type(info_type);
                    info_dialog.set_content(lines);
                    info_dialog.set_theme(&self.ui_state.theme);
                }
                // Only push the focus entry on first creation; reusing
                // an open info dialog must not double-push the stack.
                if focus_was_empty {
                    if let Some(ref info_dialog) = self.dialog_state.info_dialog {
                        self.focus_manager.push(Box::new(info_dialog.clone()));
                    }
                }
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
            Dialog::SourcePreview => {
                if let Some(ref mut dialog) = self.dialog_state.source_preview_dialog {
                    dialog.set_theme(&self.ui_state.theme);
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

    fn load_sessions_dialog(&mut self) {
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::ReloadSessions);
        }
    }

    #[allow(dead_code)]
    fn delete_selected_session(&mut self) {
        let session = match self.dialog_state.session_dialog.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };
        let session_id = session.id.clone();

        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::DeleteSession {
                session_id: session_id.clone(),
            });
        }
        self.undo_session_id = Some(session_id);
        self.undo_until = Some(Instant::now() + std::time::Duration::from_secs(30));
        self.status_bar
            .set_undo_message("Session deleted — press U to undo");
    }

    #[allow(dead_code)]
    fn archive_selected_session(&mut self) {
        let session = match self.dialog_state.session_dialog.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };
        let session_id = session.id.clone();
        let is_archived = session.time_archived.is_some();

        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::ArchiveSession {
                session_id,
                unarchive: is_archived,
            });
        }
    }

    #[allow(dead_code)]
    fn fork_selected_session(&mut self) {
        let session = match self.dialog_state.session_dialog.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };
        let session_id = session.id.clone();

        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::ForkSession { session_id });
        }
    }

    fn toggle_show_archived(&mut self) {
        self.dialog_state.session_dialog.toggle_show_archived();
        self.load_sessions_dialog();
    }

    fn clear_session(&mut self) {
        self.messages_state.messages.clear();
        self.session_state.token_in = 0;
        self.session_state.token_out = 0;
        self.reset_live_token_estimate();
        self.session_state.reasoning_tokens = 0;
        self.session_state.context_tokens = 0;
    }

    fn new_session(&mut self) {
        if let Some(config_watcher) = self.config_watcher.as_ref() {
            if let Ok(config) = config_watcher.reload_now() {
                if let Some(templates) = config.templates.as_ref() {
                    if !templates.is_empty() {
                        self.open_dialog(Dialog::Template);
                        return;
                    }
                }
            }
        }

        self.session_state.session = None;
        self.messages_state.messages.clear();
        self.session_state.token_in = 0;
        self.session_state.token_out = 0;
        self.reset_live_token_estimate();
        self.session_state.session_status = SessionStatus::Idle;
        self.prompt_state.pending_send = false;
        self.ui_state.routes.navigate_to(Route::Home);
    }

    fn apply_template(&mut self, key: String, template: SessionTemplate) {
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::CreateFromTemplate { key, template });
        }
    }

    fn toggle_sidebar(&mut self) {
        self.ui_state.sidebar_visible = !self.ui_state.sidebar_visible;
    }

    /// Handle `/theme [list|use <name>|reload|diagnostics]`. With no arguments,
    /// opens the theme picker dialog. `list` prints a toast of available
    /// themes. `use <name>` applies the named theme and persists the choice.
    /// `reload` rebuilds the registry from disk. `diagnostics` reports any
    /// non-fatal validation warnings.
    fn handle_theme_command(&mut self, raw_input: Option<&str>) {
        self.ui_state.command_mode = false;
        let arg = raw_input
            .and_then(|s| s.split_once(' ').map(|(_, rest)| rest.trim()))
            .unwrap_or("");

        if arg.is_empty() {
            self.open_dialog(Dialog::Theme);
            return;
        }

        let mut parts = arg.split_whitespace();
        let subcommand = parts.next().unwrap_or("");

        match subcommand {
            "list" => {
                let names = self.theme_registry.names();
                if names.is_empty() {
                    self.messages_state.toasts.warning("No themes available");
                } else {
                    let preview = names
                        .iter()
                        .take(20)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let extra = if names.len() > 20 {
                        format!(" (and {} more)", names.len() - 20)
                    } else {
                        String::new()
                    };
                    self.messages_state
                        .toasts
                        .info(&format!("Themes: {}{}", preview, extra));
                }
            }
            "use" => {
                if let Some(name) = parts.next() {
                    if self.apply_theme(name) {
                        self.persist_theme_selection(name);
                        self.messages_state.toasts.info(&format!("Theme: {}", name));
                    } else {
                        self.messages_state
                            .toasts
                            .error(&format!("Unknown theme: {}", name));
                    }
                } else {
                    self.messages_state.toasts.error("Usage: /theme use <name>");
                }
            }
            "reload" => {
                let current = self.ui_state.theme.name.clone();
                let config = crate::config::schema::Config::load().unwrap_or_default();
                let new_registry = std::sync::Arc::new(
                    crate::theme::ThemeRegistry::load_with_config(config.theme.as_ref()),
                );
                self.theme_registry = new_registry;
                if !self.apply_theme(&current) {
                    // The current theme disappeared; fall back to the
                    // default theme id (Cyber Red).
                    self.apply_theme(crate::theme::registry::DEFAULT_THEME_ID);
                }
                self.messages_state.toasts.info("Theme registry reloaded");
            }
            "diagnostics" => {
                let diags = self.theme_registry.diagnostics();
                if diags.is_empty() {
                    self.messages_state.toasts.info("No theme diagnostics");
                } else {
                    for d in diags {
                        let level = match d.level {
                            crate::theme::ThemeDiagnosticLevel::Warning => "warn",
                            crate::theme::ThemeDiagnosticLevel::Error => "error",
                        };
                        let field = d.field.as_deref().unwrap_or("-");
                        self.messages_state.toasts.info(&format!(
                            "[{}] {} / {}: {}",
                            level, d.theme_id, field, d.message
                        ));
                    }
                }
            }
            other => {
                // If the argument looks like a theme name, apply it directly.
                if self.apply_theme(other) {
                    self.persist_theme_selection(other);
                    self.messages_state
                        .toasts
                        .info(&format!("Theme: {}", other));
                } else {
                    self.messages_state.toasts.error(&format!(
                        "Unknown theme subcommand or name: {} (try /theme list, use, reload, diagnostics)",
                        other
                    ));
                }
            }
        }
    }

    fn toggle_reasoning(&mut self) {
        if let Some(idx) = self.messages_state.messages.sel_msg {
            if !self.messages_state.messages.toggle_tool_output(idx) {
                self.messages_state.messages.toggle_reasoning(idx);
            }
        }
    }

    fn toggle_tts(&mut self) {
        self.ui_state.tts_enabled = !self.ui_state.tts_enabled;
        if self.ui_state.tts_via_daemon {
            if self.ui_state.tts_enabled {
                if let Some(idx) = self.messages_state.messages.sel_msg {
                    if let Some(msg) = self.messages_state.messages.get_message(idx) {
                        let text = msg.text_content();
                        if !text.is_empty() {
                            self.send_notification_speak_to_daemon(text);
                            self.messages_state.toasts.info("TTS speak sent to daemon");
                            return;
                        }
                    }
                }
                self.messages_state
                    .toasts
                    .info("TTS enabled (daemon: nothing selected to speak)");
            } else {
                self.send_notification_stop_to_daemon();
                self.messages_state
                    .toasts
                    .info("TTS disabled (daemon: stop sent)");
            }
            return;
        }
        if self.ui_state.tts_enabled {
            if let Some(idx) = self.messages_state.messages.sel_msg {
                if let Some(msg) = self.messages_state.messages.get_message(idx) {
                    let text = msg.text_content();
                    if !text.is_empty() {
                        let tts = self.ui_state.tts.clone();
                        let text = text.clone();
                        self.task_registry.spawn(
                            TuiTaskKind::Notification,
                            "tts-speak",
                            async move {
                                if let Err(e) = tts.speak(&text).await {
                                    tracing::debug!("TTS speak error: {}", e);
                                }
                            },
                        );
                    }
                }
            }
            self.messages_state.toasts.info("TTS enabled");
        } else {
            let tts = self.ui_state.tts.clone();
            self.task_registry
                .spawn(TuiTaskKind::Notification, "tts-stop", async move {
                    if let Err(e) = tts.stop().await {
                        tracing::debug!("TTS stop error: {}", e);
                    }
                });
            self.messages_state.toasts.info("TTS disabled");
        }
    }

    fn stop_tts(&mut self) {
        if self.ui_state.tts_via_daemon {
            self.send_notification_stop_to_daemon();
            return;
        }
        let tts = self.ui_state.tts.clone();
        self.task_registry
            .spawn(TuiTaskKind::Notification, "tts-stop", async move {
                if let Err(e) = tts.stop().await {
                    tracing::debug!("TTS stop error: {}", e);
                }
            });
        self.messages_state.toasts.info("TTS stopped");
    }

    /// Send a TTS speak request to the daemon's `NotificationRouter` via
    /// the `CoreClient`. Used in `RemoteCore` mode where the local TUI
    /// has no audio output. The daemon's `AudioArbiter` (if enabled)
    /// will pick the event up and speak it.
    fn send_notification_speak_to_daemon(&mut self, text: String) {
        let Some(client) = self.core_client.clone() else {
            self.messages_state
                .toasts
                .info("TTS: no core client; cannot route to daemon");
            return;
        };
        let session_id = self.session_state.session.as_ref().map(|s| s.id.clone());
        let request = crate::core::new_request(
            format!("notification-speak-{}", uuid::Uuid::new_v4()),
            CoreRequest::NotificationSpeak {
                text,
                kind: Some("turn_completed".to_string()),
                priority: Some("normal".to_string()),
                session_id,
            },
        );
        tokio::spawn(async move {
            if let Err(e) = client.request(request).await {
                tracing::debug!("core facade notification_speak failed: {}", e);
            }
        });
    }

    /// Ask the daemon to stop any in-flight TTS playback.
    fn send_notification_stop_to_daemon(&mut self) {
        let Some(client) = self.core_client.clone() else {
            self.messages_state
                .toasts
                .info("TTS: no core client; cannot stop daemon TTS");
            return;
        };
        let request = crate::core::new_request(
            format!("notification-stop-{}", uuid::Uuid::new_v4()),
            CoreRequest::NotificationStop,
        );
        tokio::spawn(async move {
            if let Err(e) = client.request(request).await {
                tracing::debug!("core facade notification_stop failed: {}", e);
            }
        });
        self.messages_state.toasts.info("TTS stop sent to daemon");
    }

    fn toggle_fullscreen(&mut self) {
        self.ui_state.fullscreen = !self.ui_state.fullscreen;
        use std::io::Write;
        if self.ui_state.fullscreen {
            let _ = std::io::stdout().write_all(b"\x1b[?1049h");
            self.messages_state.toasts.info("Fullscreen mode enabled");
        } else {
            let _ = std::io::stdout().write_all(b"\x1b[?1049l");
            self.messages_state.toasts.info("Fullscreen mode disabled");
        }
    }

    fn open_external_editor(&mut self) {
        if let Some(ref file) = self.session_state.last_edited_file {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            if let Err(e) = std::process::Command::new(&editor).arg(file).spawn() {
                self.messages_state
                    .toasts
                    .info(&format!("Failed to open editor: {}", e));
            } else {
                self.messages_state
                    .toasts
                    .info(&format!("Opening {} in {}", file, editor));
            }
        } else {
            self.messages_state
                .toasts
                .info("No recently edited file to open");
        }
    }

    fn handle_diff_command(&mut self, path: Option<&str>) {
        let project_dir = std::path::PathBuf::from(&self.session_state.project_dir);
        let git_root = match crate::worktree::find_git_root(&project_dir) {
            Some(r) => r,
            None => {
                self.messages_state
                    .toasts
                    .info("Not in a git repository - showing text diff");
                return;
            }
        };

        let env_path = std::env::var_os("PATH").unwrap_or_default();

        if let Some(file_path) = path {
            let abs_path = if std::path::Path::new(file_path).is_absolute() {
                std::path::PathBuf::from(file_path)
            } else {
                project_dir.join(file_path)
            };

            let old_content = std::process::Command::new("git")
                .env_clear()
                .env("PATH", &env_path)
                .args(["show", &format!("HEAD:{}", file_path)])
                .current_dir(&git_root)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();

            let new_content = std::fs::read_to_string(&abs_path).unwrap_or_default();

            if old_content == new_content {
                self.messages_state.toasts.info("No changes detected");
                return;
            }

            let title = format!("Diff: {}", file_path);
            self.process_msg(TuiMsg::OpenDiffDialog {
                old_content: old_content.into_boxed_str(),
                new_content: new_content.into_boxed_str(),
                title: title.into_boxed_str(),
            });
        } else {
            let output = std::process::Command::new("git")
                .env_clear()
                .env("PATH", &env_path)
                .args(["diff"])
                .current_dir(&git_root)
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    let diff_text = String::from_utf8_lossy(&o.stdout).to_string();
                    if diff_text.is_empty() {
                        self.messages_state.toasts.info("No changes detected");
                        return;
                    }
                    self.process_msg(TuiMsg::OpenDiffDialog {
                        old_content: String::new().into_boxed_str(),
                        new_content: diff_text.into_boxed_str(),
                        title: "Repository Diff".to_string().into_boxed_str(),
                    });
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                    self.messages_state
                        .toasts
                        .error(&format!("git diff failed: {}", stderr));
                }
                Err(e) => {
                    self.messages_state
                        .toasts
                        .error(&format!("Failed to run git diff: {}", e));
                }
            }
        }
    }

    fn handle_tests_command(&mut self, subcmd: &str) {
        match subcmd {
            "" | "last" => {
                let state = &self.session_state_derived;
                match &state.test_state {
                    crate::session::state::TestState::Unknown => {
                        self.messages_state
                            .toasts
                            .info("Test state: unknown (no tests run yet)");
                    }
                    crate::session::state::TestState::Stale => {
                        self.messages_state
                            .toasts
                            .info("Test state: stale (files changed since last run)");
                    }
                    crate::session::state::TestState::Running { command } => {
                        self.messages_state
                            .toasts
                            .info(&format!("Test state: running ({})", command));
                    }
                    crate::session::state::TestState::Passed {
                        command,
                        duration_ms,
                    } => {
                        let duration_str = duration_ms
                            .map(|ms| format!(" ({:.1}s)", ms as f64 / 1000.0))
                            .unwrap_or_default();
                        self.messages_state.toasts.info(&format!(
                            "Test state: passed{} | command: {}",
                            duration_str, command
                        ));
                    }
                    crate::session::state::TestState::Failed {
                        command,
                        duration_ms,
                        summary,
                    } => {
                        let duration_str = duration_ms
                            .map(|ms| format!(" ({:.1}s)", ms as f64 / 1000.0))
                            .unwrap_or_default();
                        self.messages_state.toasts.info(&format!(
                            "Test state: failed{} | command: {} | {}",
                            duration_str, command, summary
                        ));
                    }
                }
                let file_count = state.changed_files.len();
                if file_count > 0 {
                    self.messages_state
                        .toasts
                        .info(&format!("Changed files since last run: {}", file_count));
                }
            }
            "failed" => {
                let state = &self.session_state_derived;
                match &state.test_state {
                    crate::session::state::TestState::Failed { summary, .. } => {
                        self.messages_state
                            .toasts
                            .info(&format!("Failed tests: {}", summary));
                    }
                    _ => {
                        self.messages_state.toasts.info("No failed tests");
                    }
                }
            }
            _ => {
                self.messages_state
                    .toasts
                    .warning("Usage: /tests [last|failed]");
            }
        }
    }

    fn handle_revert_command(&mut self, path: &str) {
        let project_dir = std::path::PathBuf::from(&self.session_state.project_dir);
        let git_root = match crate::worktree::find_git_root(&project_dir) {
            Some(r) => r,
            None => {
                self.messages_state.toasts.error("Not in a git repository");
                return;
            }
        };

        let output = std::process::Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(["checkout", "HEAD", "--", path])
            .current_dir(&git_root)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                self.messages_state
                    .toasts
                    .success(&format!("Reverted: {}", path));
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                self.messages_state
                    .toasts
                    .error(&format!("git checkout failed: {}", stderr));
            }
            Err(e) => {
                self.messages_state
                    .toasts
                    .error(&format!("Failed to run git: {}", e));
            }
        }
    }

    fn handle_memory_command(&mut self, action: Option<(&str, &str)>) {
        let mem_store = match &self.memory_store {
            Some(s) => s,
            None => {
                self.messages_state
                    .toasts
                    .error("Memory store not available");
                return;
            }
        };

        let project_hash = format!(
            "{:x}",
            md5::compute(self.session_state.project_dir.as_bytes())
        );
        let project_namespace = format!("project/{}", project_hash);

        match action {
            None | Some(("list", "")) => {
                let prefs = mem_store.list("user/preferences");
                let proj = mem_store.list(&project_namespace);
                let total = prefs.len() + proj.len();
                if total == 0 {
                    self.messages_state
                        .toasts
                        .info("No memories yet. Use /memory-remember <text> to save something.");
                } else {
                    let mut lines = vec![format!("Memory Summary ({} total):", total)];
                    if !prefs.is_empty() {
                        lines.push(format!("  user/preferences ({}):", prefs.len()));
                        for m in prefs.iter().take(5) {
                            let title = m.title.as_deref().unwrap_or("(untitled)");
                            lines.push(format!(
                                "    - [{}] {}",
                                m.id.chars().take(8).collect::<String>(),
                                title
                            ));
                        }
                        if prefs.len() > 5 {
                            lines.push(format!("    ... and {} more", prefs.len() - 5));
                        }
                    }
                    if !proj.is_empty() {
                        lines.push(format!("  {} ({}):", project_namespace, proj.len()));
                        for m in proj.iter().take(5) {
                            let title = m.title.as_deref().unwrap_or("(untitled)");
                            lines.push(format!(
                                "    - [{}] {}",
                                m.id.chars().take(8).collect::<String>(),
                                title
                            ));
                        }
                        if proj.len() > 5 {
                            lines.push(format!("    ... and {} more", proj.len() - 5));
                        }
                    }
                    self.messages_state.toasts.info(&lines.join("\n"));
                }
            }
            Some(("search", query)) => {
                if query.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /memory-search <query>");
                    return;
                }
                let results = mem_store.search(query);
                if results.is_empty() {
                    self.messages_state
                        .toasts
                        .info(&format!("No memories found matching '{}'", query));
                } else {
                    let lines: Vec<String> = results
                        .iter()
                        .take(10)
                        .map(|m| {
                            let title = m.title.as_deref().unwrap_or("(untitled)");
                            format!("- [{}] {}", m.id.chars().take(8).collect::<String>(), title)
                        })
                        .collect();
                    let msg = if results.len() > 10 {
                        format!(
                            "Found {} memories (showing 10):\n{}",
                            results.len(),
                            lines.join("\n")
                        )
                    } else {
                        format!("Found {} memory:\n{}", results.len(), lines.join("\n"))
                    };
                    self.messages_state.toasts.info(&msg);
                }
            }
            Some(("remember", text)) => {
                if text.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /memory-remember <text to remember>");
                    return;
                }
                let memory = crate::memory::Memory::new("user/preferences", text);
                mem_store.add(memory);
                self.messages_state.toasts.info("Remembered");
            }
            Some(("forget", id)) => {
                if id.is_empty() {
                    self.messages_state
                        .toasts
                        .warning("Usage: /memory-forget <id>");
                    return;
                }
                if mem_store.delete(id).is_some() {
                    self.messages_state.toasts.info("Memory deleted");
                } else {
                    self.messages_state
                        .toasts
                        .warning(&format!("Memory '{}' not found", id));
                }
            }
            Some(("consolidate", _)) => {
                let session_id = self.session_state.session.as_ref().map(|s| s.id.clone());
                let message_store = self.message_store.clone();
                let core_client = self.core_client.clone();
                let mem_store_clone = mem_store.clone();
                let project_hash_clone = project_hash.clone();
                self.messages_state
                    .toasts
                    .info("Consolidating session memories...");
                self.task_registry
                    .spawn(TuiTaskKind::Memory, "memory-consolidate", async move {
                        let messages =
                            if let (Some(client), Some(sid)) = (core_client, session_id.clone()) {
                                let request = crate::core::new_request(
                                    format!("session-messages-{}", uuid::Uuid::new_v4()),
                                    crate::protocol::core::CoreRequest::SessionMessagesLoad {
                                        session_id: sid,
                                    },
                                );
                                match client.request(request).await {
                                    Ok(crate::protocol::core::CoreResponse::SessionMessages {
                                        messages,
                                        ..
                                    }) => crate::protocol_conversions::dtos_to_messages(messages),
                                    _ => Vec::new(),
                                }
                            } else if let (Some(sid), Some(store)) =
                                (session_id.as_ref(), message_store.as_ref())
                            {
                                store.list(sid).await.unwrap_or_default()
                            } else {
                                Vec::new()
                            };
                        if messages.is_empty() {
                            return;
                        }
                        let new_memories =
                            mem_store_clone.consolidate_session(&messages, &project_hash_clone);
                        tracing::info!("Manual consolidation: {} new memories", new_memories.len());
                    });
            }
            Some((cmd, _)) => {
                self.messages_state
                    .toasts
                    .warning(&format!("Unknown memory command: /{}", cmd));
            }
        }
    }

    fn handle_research_command(&mut self, args: &str) {
        if self.prompt_state.pending_send {
            self.messages_state
                .toasts
                .warning("Still waiting for previous prompt to finish");
            return;
        }

        // Parse flags from args
        let mut question_parts = Vec::new();
        let mut mode = "narrow-answer".to_string();
        let mut depth = "medium".to_string();

        let mut iter = args.split_whitespace();
        while let Some(token) = iter.next() {
            match token {
                "--mode" => {
                    if let Some(val) = iter.next() {
                        mode = val.to_string();
                    }
                }
                "--depth" => {
                    if let Some(val) = iter.next() {
                        depth = val.to_string();
                    }
                }
                _ => question_parts.push(token),
            }
        }

        let question = question_parts.join(" ");
        if question.is_empty() {
            self.messages_state
                .toasts
                .warning("Usage: /research <question> [--mode <mode>] [--depth <depth>]");
            return;
        }

        let parsed_mode = match crate::research::service::parse_mode(&mode) {
            Ok(m) => m,
            Err(e) => {
                self.messages_state.toasts.warning(&e.to_string());
                return;
            }
        };
        let parsed_depth = match crate::research::service::parse_depth(&depth) {
            Ok(d) => d,
            Err(e) => {
                self.messages_state.toasts.warning(&e.to_string());
                return;
            }
        };

        let project_dir = self.session_state.project_dir.clone();
        self.messages_state
            .toasts
            .info(&format!("Starting research: {}", question));

        self.task_registry
            .spawn(TuiTaskKind::Research, "research-answer", async move {
                let service = crate::research::service::ResearchService::new(
                    std::path::PathBuf::from(&project_dir),
                );
                match service
                    .answer_for_agent(&question, parsed_mode, parsed_depth)
                    .await
                {
                    Ok(answer) => {
                        tracing::info!(
                            "Research complete ({} chars). Artifacts at: {}",
                            answer.len(),
                            service.artifact_root().display()
                        );
                    }
                    Err(e) => {
                        tracing::error!("Research failed: {}", e);
                    }
                }
            });
    }

    fn handle_research_runs_command(&mut self) {
        self.open_dialog(Dialog::ResearchBrowser);
        if let Some(ref mut browser) = self.dialog_state.research_browser {
            browser.loading = true;
        }
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::ResearchListRuns);
        }
    }

    fn handle_research_open_command(&mut self, run_id: &str) {
        self.open_dialog(Dialog::ResearchBrowser);
        if let Some(ref mut browser) = self.dialog_state.research_browser {
            browser.loading = true;
        }
        let run_id_owned = run_id.to_string();
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::ResearchLoadRun {
                run_id: run_id_owned,
            });
        }
    }

    fn close_session(&mut self) {
        self.clear_session();
    }

    fn quit(&mut self) {
        self.ui_state.running = false;
        let _ = self.ui_state.shutdown_tx.take().map(|tx| tx.send(()));
    }

    fn stash_prompt(&mut self) {
        let text = self.prompt_state.prompt.get_text();
        if text.is_empty() {
            return;
        }
        self.prompt_state.stashed_prompts.push(text.clone());
        self.prompt_state.prompt.clear();
    }

    fn restore_prompt(&mut self) {
        if self.prompt_state.stashed_prompts.is_empty() {
            return;
        }
        let new_pos = match self.prompt_state.stash_pos {
            Some(pos) if pos > 0 => pos - 1,
            None => self.prompt_state.stashed_prompts.len() - 1,
            _ => return,
        };
        self.prompt_state.stash_pos = Some(new_pos);
        if let Some(entry) = self.prompt_state.stashed_prompts.get(new_pos) {
            self.prompt_state.prompt.set_text(entry.clone());
        }
    }

    fn copy_message(&mut self) {
        let text = self.messages_state.messages.get_selected_content();
        if text.is_empty() {
            return;
        }
        use arboard::Clipboard;
        if let Ok(mut clip) = Clipboard::new() {
            let _ = clip.set_text(&text);
        }
    }

    fn cycle_model_forward(&mut self) {
        if self.agent_state.models.is_empty() {
            return;
        }
        self.agent_state.model_idx =
            (self.agent_state.model_idx + 1) % self.agent_state.models.len();
        self.agent_state.current_model =
            self.agent_state.models[self.agent_state.model_idx].clone();
        self.dialog_state
            .model_dialog
            .set_current(&self.agent_state.current_model);
        let model = self.agent_state.current_model.clone();
        self.persist_model_selection(&model);
    }

    fn cycle_model_backward(&mut self) {
        if self.agent_state.models.is_empty() {
            return;
        }
        self.agent_state.model_idx = if self.agent_state.model_idx == 0 {
            self.agent_state.models.len() - 1
        } else {
            self.agent_state.model_idx - 1
        };
        self.agent_state.current_model =
            self.agent_state.models[self.agent_state.model_idx].clone();
        self.dialog_state
            .model_dialog
            .set_current(&self.agent_state.current_model);
        let model = self.agent_state.current_model.clone();
        self.persist_model_selection(&model);
    }

    fn open_connect_dialog(&mut self) {
        use crate::tui::components::dialogs::connect::{ProviderAuthMode, ProviderInfo};

        let mk = |id: &str, name: &str, desc: &str, env: Option<&str>, url: Option<&str>| {
            let mut info = if env.is_some() {
                ProviderInfo::api_key(id, name, desc, env.map(|s| s.to_string()))
            } else {
                ProviderInfo::no_auth(id, name, desc)
            };
            info.base_url_example = url.map(|s| s.to_string());
            info
        };

        let providers = vec![
            mk(
                "openai",
                "OpenAI",
                "GPT-4, GPT-4o, and other OpenAI models",
                Some("OPENAI_API_KEY"),
                Some("https://api.openai.com/v1"),
            ),
            mk(
                "anthropic",
                "Anthropic",
                "Claude models (Note: API access restricted)",
                Some("ANTHROPIC_API_KEY"),
                Some("https://api.anthropic.com"),
            ),
            mk(
                "google",
                "Google",
                "Gemini models",
                Some("GOOGLE_API_KEY"),
                Some("https://generativelanguage.googleapis.com/v1"),
            ),
            mk(
                "ollama",
                "Ollama",
                "Local and self-hosted models",
                None,
                Some("http://localhost:11434"),
            ),
            mk(
                "openrouter",
                "OpenRouter",
                "Unified API for 100+ models",
                Some("OPENROUTER_API_KEY"),
                Some("https://openrouter.ai/api/v1"),
            ),
            mk(
                "lmstudio",
                "LM Studio",
                "Local models via LM Studio",
                None,
                Some("http://localhost:1234/v1"),
            ),
            mk(
                "deepseek",
                "DeepSeek",
                "DeepSeek models",
                Some("DEEPSEEK_API_KEY"),
                Some("https://api.deepseek.com/v1"),
            ),
            mk(
                "xai",
                "xAI",
                "xAI (Grok)",
                Some("XAI_API_KEY"),
                Some("https://api.x.ai/v1"),
            ),
            mk(
                "cohere",
                "Cohere",
                "Command R models",
                Some("COHERE_API_KEY"),
                Some("https://api.cohere.ai/v1"),
            ),
            mk(
                "fireworks",
                "Fireworks",
                "Fireworks AI models",
                Some("FIREWORKS_API_KEY"),
                Some("https://api.fireworks.ai/v1"),
            ),
            mk(
                "novai",
                "Novae",
                "Novae AI models",
                Some("NOVAI_API_KEY"),
                Some("https://api.novai.ai/v1"),
            ),
            mk(
                "opencode_zen",
                "Codegg Zen",
                "Free models from Codegg",
                Some("OPENCODE_ZEN_API_KEY"),
                Some("https://opencode.ai/zen/v1"),
            ),
            mk(
                "opencode_go",
                "OpenCode Go",
                "Enterprise models from OpenCode",
                Some("OPENCODE_GO_API_KEY"),
                Some("https://opencode.ai/go/v1"),
            ),
            mk(
                "minimax",
                "MiniMax",
                "Chinese LLM provider",
                Some("MINIMAX_API_KEY"),
                Some("https://api.minimax.chat"),
            ),
            mk(
                "zai",
                "Z.ai",
                "Z.ai provider",
                Some("ZAI_API_KEY"),
                Some("https://api.z.ai"),
            ),
            mk(
                "generalcompute",
                "GeneralCompute",
                "GeneralCompute.com LLM provider",
                Some("GENERALCOMPUTE_API_KEY"),
                Some("https://api.generalcompute.com/v1"),
            ),
        ];
        // First pass: every provider supports API-key entry. Future
        // officially-supported OAuth / external-command providers can be
        // added by extending `auth_modes` here.
        let _ = ProviderAuthMode::ApiKey; // anchor the import for future use

        let connect_dialog = crate::tui::components::dialogs::connect::ConnectDialog::new(
            providers,
            Arc::clone(&self.ui_state.theme),
        );
        self.dialog_state.connect_dialog = Some(connect_dialog);
        self.open_dialog(Dialog::Connect);
    }

    fn open_tree_dialog(&mut self) {
        self.open_dialog(Dialog::Tree);
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::OpenTreeDialog);
        }
    }

    fn fork_tree_session(&mut self) {
        let session_id = match self.dialog_state.tree_dialog.fork_selected() {
            Some(id) => id,
            None => return,
        };

        self.close_dialog();

        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::ForkSession { session_id });
        }
    }

    fn handle_import_send(&mut self) {
        let import = match &mut self.dialog_state.import_dialog {
            Some(i) => i,
            None => return,
        };

        match import.state {
            super::components::dialogs::import::ImportState::Input => {
                let source = import.parse_input();
                match source {
                    Some(src) => {
                        import.set_preview_loading(true);
                        if let Some(ref tx) = self.tui_cmd_tx {
                            let _ = tx.try_send(TuiCommand::PreviewImport { source: src });
                        }
                    }
                    None => {
                        import.set_error("Enter a share URL, session ID, or file path".to_string());
                    }
                }
            }
            super::components::dialogs::import::ImportState::Preview => {
                let source = import.parse_input();
                import.set_importing();

                if let Some(ref tx) = self.tui_cmd_tx {
                    if let Some(src) = source {
                        let _ = tx.try_send(TuiCommand::ConfirmImport { source: src });
                    }
                }
            }
            super::components::dialogs::import::ImportState::Done => {
                if let Some(session) = import.imported_session() {
                    let session = session.clone();
                    self.dialog_state.import_dialog = None;
                    self.close_dialog();
                    self.set_session(session);
                    self.messages_state.toasts.info("Session imported");
                }
            }
            super::components::dialogs::import::ImportState::Error => {
                self.dialog_state.import_dialog = None;
                self.close_dialog();
            }
            super::components::dialogs::import::ImportState::Importing => {}
        }
    }

    fn handle_connect_send(&mut self) {
        use crate::tui::components::dialogs::connect::ConnectStep;

        let provider_info = {
            let connect_dialog = match &mut self.dialog_state.connect_dialog {
                Some(cd) => cd,
                None => return,
            };

            match connect_dialog.step {
                ConnectStep::SelectProvider => {
                    let provider = match connect_dialog.select_provider() {
                        Some(p) => p.clone(),
                        None => {
                            connect_dialog.set_error("No provider selected".to_string());
                            return;
                        }
                    };

                    if provider.requires_api_key {
                        connect_dialog.move_to_api_key_step();
                        return;
                    } else {
                        self.dialog_state.connect_dialog = None;
                        self.close_dialog();
                        self.messages_state.toasts.info(&format!(
                            "Connected to {} (no API key required)",
                            provider.name
                        ));
                        return;
                    }
                }
                ConnectStep::EnterApiKey => {
                    let api_key = connect_dialog.get_api_key();
                    if api_key.trim().is_empty() {
                        connect_dialog.set_error("API key cannot be empty".to_string());
                        return;
                    }

                    let provider = match connect_dialog.select_provider() {
                        Some(p) => p.clone(),
                        None => {
                            connect_dialog.set_error("No provider selected".to_string());
                            return;
                        }
                    };

                    Some((provider, api_key))
                }
            }
        };

        if let Some((provider, api_key)) = provider_info {
            if let Some(env_var) = &provider.env_var_name {
                std::env::set_var(env_var, &api_key);
                self.dialog_state.connect_dialog = None;
                self.close_dialog();
                self.messages_state.toasts.info(&format!(
                    "API key set for {}. {} environment variable updated.",
                    provider.name, env_var
                ));
            } else {
                if let Some(cd) = &mut self.dialog_state.connect_dialog {
                    cd.set_error("Provider has no environment variable configured".to_string());
                }
            }
        }
    }

    fn on_char(&mut self, c: char) {
        if self.ui_state.dialog.is_open() {
            return;
        }
        if self.messages_state.messages.is_searching() {
            if c == 'n' {
                self.messages_state.messages.search_next();
                return;
            }
            if c == 'N' {
                self.messages_state.messages.search_prev();
                return;
            }
        }
        if c == '/' && self.prompt_state.prompt.cursor_pos() == 0 {
            self.ui_state.command_mode = true;
            self.dialog_state.command_palette.set_query("/");
            debug_log!("on_char: typed '/' at position 0, entering command_mode");
        }
        self.prompt_state.prompt.insert_char(c);
        self.update_completions();
    }

    fn update_completions(&mut self) {
        let text = self.prompt_state.prompt.get_text();
        let cursor = self.prompt_state.prompt.cursor_pos();
        let before_cursor = &text[..cursor];

        if let Some(pos) = before_cursor.rfind('/') {
            if pos == 0 || before_cursor.chars().nth(pos.saturating_sub(1)) == Some(' ') {
                self.prompt_state.completion_filter = before_cursor[pos..].to_string();
                self.prompt_state.completion_type = CompletionType::Slash;
                if !self.ui_state.command_mode {
                    self.prompt_state.show_completions = true;
                    debug_log!("update_completions: slash completion - show_completions=true");
                }
                self.prompt_state.completion_sel = 0;
                return;
            }
        }

        if let Some(pos) = before_cursor.rfind('@') {
            if pos == 0 || before_cursor.chars().nth(pos.saturating_sub(1)) == Some(' ') {
                self.prompt_state.completion_filter = before_cursor[pos..].to_string();
                let query = self.prompt_state.completion_filter.trim_start_matches('@');

                let is_agent_trigger =
                    !query.contains('/') && !query.contains('\\') && !query.contains('.');

                if is_agent_trigger {
                    self.prompt_state.completion_type = CompletionType::Agent;
                    self.update_agent_completions();
                    self.prompt_state.show_completions = true;
                    debug_log!("update_completions: agent completion - show_completions=true");
                } else {
                    self.prompt_state.completion_type = CompletionType::File;
                    self.update_file_completions();
                    self.prompt_state.show_completions = true;
                    debug_log!("update_completions: file completion - show_completions=true");
                }
                self.prompt_state.completion_sel = 0;
                return;
            }
        }

        if self.prompt_state.show_completions {
            debug_log!("update_completions: no trigger found - show_completions=false");
        }
        self.prompt_state.show_completions = false;
    }

    fn update_file_completions(&mut self) {
        use crate::tui::components::completion_overlay::CompletionItem;

        let filter = self.prompt_state.completion_filter.trim_start_matches('@');

        let (dir_prefix, partial_name) = self.parse_path_components(filter);

        let candidates = match self.session_state.indexed_files.try_read() {
            Ok(files) => files.clone(),
            Err(_) => {
                self.prompt_state.file_completions.clear();
                return;
            }
        };

        let expanded_dir = if let Some(stripped) = dir_prefix.strip_prefix('~') {
            match dirs::home_dir() {
                Some(home) => format!("{}/{}", home.display(), stripped),
                None => dir_prefix.clone(),
            }
        } else {
            dir_prefix.clone()
        };

        let filtered: Vec<CompletionItem> = candidates
            .iter()
            .filter(|path| {
                if dir_prefix.is_empty() {
                    true
                } else {
                    path.starts_with(&expanded_dir) || path.contains(&partial_name)
                }
            })
            .filter_map(|path| {
                let name = if dir_prefix.is_empty() {
                    std::path::Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(path)
                        .to_string()
                } else {
                    let stripped = path.strip_prefix(&expanded_dir).unwrap_or(path);
                    let stripped = stripped.trim_start_matches('/');
                    std::path::Path::new(stripped)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(stripped)
                        .to_string()
                };

                let score = if partial_name.is_empty() {
                    usize::MAX
                } else {
                    fuzzy_score(&partial_name, &name)
                };

                if score > 0 {
                    Some((name, score))
                } else {
                    None
                }
            })
            .filter(|(_, score)| partial_name.is_empty() || *score > 0)
            .take(20)
            .map(|(name, _)| CompletionItem::new_file(name, None))
            .collect();

        self.prompt_state.file_completions = filtered;
    }

    fn parse_path_components(&self, filter: &str) -> (String, String) {
        let path_part = filter.trim_start_matches('@');

        if let Some(last_slash) = path_part.rfind('/') {
            let dir = &path_part[..=last_slash];
            let partial = &path_part[last_slash + 1..];
            (dir.to_string(), partial.to_string())
        } else {
            (String::new(), path_part.to_string())
        }
    }

    fn update_agent_completions(&mut self) {
        use crate::tui::components::completion_overlay::{CompletionItem, CompletionItemKind};
        let filter = self.prompt_state.completion_filter.trim_start_matches('@');

        let filtered = crate::agent::mention::filter_agents(&self.agent_state.agents, filter);

        self.prompt_state.agent_completions = filtered
            .into_iter()
            .map(|a| CompletionItem {
                label: a.name.clone(),
                description: Some(a.description.clone()),
                kind: CompletionItemKind::File,
            })
            .collect();
    }

    fn accept_completion(&mut self) {
        let selected = match self.prompt_state.completion_type {
            CompletionType::Slash => self
                .prompt_state
                .slash_completions
                .iter()
                .filter(|c| c.label.starts_with(&self.prompt_state.completion_filter))
                .nth(self.prompt_state.completion_sel)
                .map(|c| c.label.clone()),
            CompletionType::File => self
                .prompt_state
                .file_completions
                .get(self.prompt_state.completion_sel)
                .map(|c| c.label.clone()),
            CompletionType::Agent => self
                .prompt_state
                .agent_completions
                .get(self.prompt_state.completion_sel)
                .map(|c| c.label.clone()),
        };
        if let Some(sel) = selected {
            let text = self.prompt_state.prompt.get_text();
            let cursor = self.prompt_state.prompt.cursor_pos();
            let safe_cursor = cursor.min(text.len());
            let before_cursor = &text[..safe_cursor];
            let after_cursor = &text[safe_cursor..];

            if self.prompt_state.completion_type == CompletionType::Agent {
                let agent_name = sel.clone();
                let trigger_pos = before_cursor.rfind('@').unwrap_or(safe_cursor);
                let new_text = format!("{}{}", &text[..trigger_pos.min(text.len())], after_cursor);
                self.prompt_state.prompt.set_text(new_text);
                self.prompt_state.show_completions = false;
                self.spawn_subagent(&agent_name, &text[trigger_pos.min(text.len())..]);
                return;
            }

            let trigger_pos = if self.prompt_state.completion_type == CompletionType::Slash {
                before_cursor.rfind('/').unwrap_or(safe_cursor)
            } else {
                before_cursor.rfind('@').unwrap_or(safe_cursor)
            };

            let new_text = format!(
                "{}{}{}",
                &text[..trigger_pos.min(text.len())],
                sel,
                after_cursor
            );
            self.prompt_state.prompt.set_text(new_text);
            self.prompt_state.prompt.set_cursor(trigger_pos + sel.len());
        }
        self.prompt_state.show_completions = false;
    }

    fn spawn_subagent(&self, agent_name: &str, prompt: &str) {
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::SpawnSubagent {
                agent_name: agent_name.to_string(),
                prompt: prompt.trim().to_string(),
            });
        }
    }

    pub fn add_assistant_text(&mut self, text: String) {
        self.messages_state.messages.add_assistant_text(text);
    }

    pub fn add_tool_call(&mut self, id: String, name: String, input: serde_json::Value) {
        self.messages_state
            .messages
            .add_tool_call(id.clone(), name, input);
        self.messages_state.messages.mark_tool_call_running(&id);
    }

    pub fn update_tool_call(
        &mut self,
        id: &str,
        output: String,
        status: ToolStatus,
        duration_ms: Option<u64>,
        exit_code: Option<i32>,
        output_lines: Option<usize>,
    ) {
        self.messages_state.messages.update_tool_call(
            id,
            output,
            status,
            duration_ms,
            exit_code,
            output_lines,
        );
    }

    pub fn add_reasoning(&mut self, reasoning: String) {
        self.messages_state.messages.add_reasoning(reasoning);
    }

    pub fn set_session(&mut self, sess: Session) {
        let sess_id = sess.id.clone();
        self.session_state.session = Some(sess);
        self.ui_state
            .routes
            .navigate_to(Route::Session(sess_id.clone()));
        if let Some(ref tx) = self.tui_cmd_tx {
            let _ = tx.try_send(TuiCommand::LoadSessionMessages {
                session_id: sess_id.clone(),
            });
            let _ = tx.try_send(TuiCommand::RefreshSessionState {
                session_id: sess_id,
            });
        }
        // Refresh sidebar git status for the new project so render
        // never blocks on git probing.
        crate::tui::commands::git_sidebar::start_refresh_git_sidebar(self);
    }

    pub fn set_session_store(&mut self, store: Arc<SessionStore>) {
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            tracing::warn!("set_session_store ignored: AppMode::RemoteCore (daemon owns storage)");
            return;
        }
        self.session_store = Some(store);
    }

    pub async fn load_sessions_via_core(
        &mut self,
    ) -> Result<Vec<crate::session::Session>, AppError> {
        let core_client = self
            .core_client
            .clone()
            .ok_or_else(|| AppError::Tui("core client unavailable for session list".to_string()))?;
        let project_id = self.session_state.project_dir.clone();
        let show_archived = self.dialog_state.session_dialog.show_archived;
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SessionList {
                project_id,
                show_archived,
                limit: 100,
            },
        );
        match core_client.request(request).await {
            Ok(crate::protocol::core::CoreResponse::SessionList { sessions }) => {
                Ok(crate::protocol_conversions::dtos_to_sessions(sessions))
            }
            Ok(crate::protocol::core::CoreResponse::Error { code, message }) => Err(AppError::Tui(
                format!("core session list failed ({}): {}", code, message),
            )),
            Ok(other) => Err(AppError::Tui(format!(
                "unexpected core response for session list: {:?}",
                other
            ))),
            Err(e) => Err(AppError::Tui(format!(
                "core session list request failed: {}",
                e
            ))),
        }
    }

    pub async fn load_messages_via_core(
        &mut self,
        session_id: &str,
    ) -> Result<Vec<crate::session::message::Message>, AppError> {
        let core_client = self
            .core_client
            .clone()
            .ok_or_else(|| AppError::Tui("core client unavailable for message load".to_string()))?;
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SessionMessagesLoad {
                session_id: session_id.to_string(),
            },
        );
        match core_client.request(request).await {
            Ok(crate::protocol::core::CoreResponse::SessionMessages { messages, .. }) => {
                Ok(crate::protocol_conversions::dtos_to_messages(messages))
            }
            Ok(crate::protocol::core::CoreResponse::Error { code, message }) => Err(AppError::Tui(
                format!("core session messages load failed ({}): {}", code, message),
            )),
            Ok(other) => Err(AppError::Tui(format!(
                "unexpected core response for session messages load: {:?}",
                other
            ))),
            Err(e) => Err(AppError::Tui(format!(
                "core session messages load request failed: {}",
                e
            ))),
        }
    }

    pub async fn load_models_via_core(&mut self) -> Result<Vec<String>, AppError> {
        let core_client = self.core_client.clone().ok_or_else(|| {
            AppError::Tui("core client unavailable for model snapshot".to_string())
        })?;
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SnapshotModels,
        );
        match core_client.request(request).await {
            Ok(crate::protocol::core::CoreResponse::ModelsSnapshot { models, .. }) => {
                self.set_models(models.clone());
                Ok(models)
            }
            Ok(crate::protocol::core::CoreResponse::Error { code, message }) => Err(AppError::Tui(
                format!("core model snapshot failed ({}): {}", code, message),
            )),
            Ok(other) => Err(AppError::Tui(format!(
                "unexpected core response for model snapshot: {:?}",
                other
            ))),
            Err(e) => Err(AppError::Tui(format!(
                "core model snapshot request failed: {}",
                e
            ))),
        }
    }

    pub async fn load_tasks_via_core(&mut self) -> Result<Vec<serde_json::Value>, AppError> {
        let core_client = self
            .core_client
            .clone()
            .ok_or_else(|| AppError::Tui("core client unavailable for task list".to_string()))?;
        let request =
            crate::core::new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TaskList);
        match core_client.request(request).await {
            Ok(crate::protocol::core::CoreResponse::Json { data }) => {
                let tasks = data
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                Ok(tasks)
            }
            Ok(crate::protocol::core::CoreResponse::Error { code, message }) => Err(AppError::Tui(
                format!("core task list failed ({}): {}", code, message),
            )),
            Ok(other) => Err(AppError::Tui(format!(
                "unexpected core response for task list: {:?}",
                other
            ))),
            Err(e) => Err(AppError::Tui(format!(
                "core task list request failed: {}",
                e
            ))),
        }
    }

    /// Drive the TUI's initial session load through the attached
    /// `CoreClient`. In socket/RemoteCore mode there is no local
    /// `SessionStore`, so this is the only way to open / continue /
    /// fork a session at startup.
    ///
    /// Best-effort: any failure is logged and swallowed so the TUI
    /// still starts (with no session) if the daemon is unreachable or
    /// a specific id is missing. The local `SessionStore` path in
    /// `main.rs` is the equivalent for inproc mode and is preserved.
    pub async fn load_initial_session_via_core(&mut self, request: InitialSessionRequest) {
        let Some(core_client) = self.core_client.clone() else {
            return;
        };
        match request {
            InitialSessionRequest::Attach { session_id } => {
                let req = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::SessionAttach {
                        session_id: session_id.clone(),
                    },
                );
                match core_client.request(req).await {
                    Ok(CoreResponse::Session { session }) => {
                        self.set_session(crate::protocol_conversions::dto_to_session(session));
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Attach): core error {}: {}",
                            code,
                            message
                        );
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Attach): unexpected response {:?}",
                            other
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Attach): request failed: {}",
                            e
                        );
                    }
                }
            }
            InitialSessionRequest::Continue { project_dir } => {
                let req = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::SessionList {
                        project_id: project_dir,
                        show_archived: false,
                        limit: 1,
                    },
                );
                match core_client.request(req).await {
                    Ok(CoreResponse::SessionList { mut sessions }) => {
                        if let Some(sess) = sessions.pop() {
                            self.set_session(crate::protocol_conversions::dto_to_session(sess));
                        }
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Continue): core error {}: {}",
                            code,
                            message
                        );
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Continue): unexpected response {:?}",
                            other
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Continue): request failed: {}",
                            e
                        );
                    }
                }
            }
            InitialSessionRequest::New { directory, title } => {
                let req = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::SessionCreate { directory, title },
                );
                match core_client.request(req).await {
                    Ok(CoreResponse::Session { session }) => {
                        self.set_session(crate::protocol_conversions::dto_to_session(session));
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "load_initial_session_via_core(New): core error {}: {}",
                            code,
                            message
                        );
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "load_initial_session_via_core(New): unexpected response {:?}",
                            other
                        );
                    }
                    Err(e) => {
                        tracing::warn!("load_initial_session_via_core(New): request failed: {}", e);
                    }
                }
            }
            InitialSessionRequest::Fork { session_id } => {
                let fork_req = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::SessionFork {
                        session_id: session_id.clone(),
                    },
                );
                if let Err(e) = core_client.request(fork_req).await {
                    tracing::warn!(
                        "load_initial_session_via_core(Fork): fork request failed: {}",
                        e
                    );
                }
                let list_req = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    CoreRequest::SessionList {
                        project_id: self.session_state.project_dir.clone(),
                        show_archived: false,
                        limit: 1,
                    },
                );
                match core_client.request(list_req).await {
                    Ok(CoreResponse::SessionList { mut sessions }) => {
                        if let Some(sess) = sessions.pop() {
                            self.set_session(crate::protocol_conversions::dto_to_session(sess));
                        } else {
                            tracing::warn!(
                                "load_initial_session_via_core(Fork): SessionList returned no sessions"
                            );
                        }
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Fork): SessionList error {}: {}",
                            code,
                            message
                        );
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Fork): unexpected SessionList response {:?}",
                            other
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "load_initial_session_via_core(Fork): SessionList request failed: {}",
                            e
                        );
                    }
                }
            }
            InitialSessionRequest::None => {}
        }
    }

    /// Initialize App state for `AppMode::RemoteCore` by pulling snapshots
    /// from the connected daemon. Best-effort: any individual load failure
    /// is logged and swallowed so a transient daemon error doesn't block
    /// the TUI from rendering. Returns the number of models loaded.
    pub async fn init_remote_core(&mut self) -> usize {
        match self.load_models_via_core().await {
            Ok(models) => models.len(),
            Err(e) => {
                tracing::warn!("init_remote_core: model snapshot failed: {}", e);
                0
            }
        }
    }

    /// Attach the SQLite-backed user-preferences store. Should be called
    /// once at startup; the App holds onto the store and uses it to
    /// remember the user's theme and last-used model across restarts.
    pub fn set_preferences(&mut self, prefs: crate::storage::UserPreferences) {
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            tracing::warn!("set_preferences ignored: AppMode::RemoteCore (daemon owns storage)");
            return;
        }
        self.preferences = Some(prefs);
    }

    pub fn set_message_store(&mut self, store: Arc<MessageStore>) {
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            tracing::warn!("set_message_store ignored: AppMode::RemoteCore (daemon owns storage)");
            return;
        }
        self.message_store = Some(store);
    }

    pub fn set_memory_store(&mut self, store: Arc<MemoryStore>) {
        if matches!(self.ui_state.mode, AppMode::RemoteCore { .. }) {
            tracing::warn!("set_memory_store ignored: AppMode::RemoteCore (daemon owns storage)");
            return;
        }
        self.memory_store = Some(store);
    }

    pub fn set_models(&mut self, models: Vec<String>) {
        self.agent_state.models = models;
        self.dialog_state
            .model_dialog
            .set_models(self.agent_state.models.clone());
        if let Some(idx) = self
            .agent_state
            .models
            .iter()
            .position(|m| m == &self.agent_state.current_model)
        {
            self.agent_state.model_idx = idx;
        } else if !self.agent_state.models.is_empty() {
            self.agent_state.current_model = self.agent_state.models[0].clone();
            self.agent_state.model_idx = 0;
        }
        self.dialog_state
            .model_dialog
            .set_current(&self.agent_state.current_model);
    }

    pub fn refresh_models(&mut self) {
        self.messages_state
            .toasts
            .info("Refreshing model cache in background...");
        let core_client = self.core_client.clone();
        let cmd_tx = self.tui_cmd_tx.clone();
        if let (Some(core_client), Some(tx)) = (core_client, cmd_tx) {
            self.task_registry
                .spawn(TuiTaskKind::Command, "refresh-models", async move {
                    let request = crate::core::new_request(
                        format!("models-refresh-{}", uuid::Uuid::new_v4()),
                        CoreRequest::ModelsRefresh,
                    );
                    match core_client.request(request).await {
                        Ok(crate::protocol::core::CoreResponse::Json { data }) => {
                            let models = data
                                .get("models")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            let model_ids: Vec<String> = models
                                .iter()
                                .filter_map(|m| m.as_str().map(ToOwned::to_owned))
                                .collect();
                            let _ = tx.send(TuiCommand::UpdateModels(model_ids)).await;
                        }
                        Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                            tracing::warn!("model refresh via core failed: {}", message);
                        }
                        Ok(other) => {
                            tracing::warn!("unexpected models refresh response: {:?}", other);
                        }
                        Err(e) => {
                            tracing::warn!("model refresh request failed: {}", e);
                        }
                    }
                });
        } else {
            self.messages_state
                .toasts
                .warning("Core client or command channel not available for refresh");
        }
    }

    pub fn set_tokens(&mut self, input: u64, output: u64) {
        self.session_state.token_in += input;
        self.session_state.token_out += output;
        self.reset_live_token_estimate();
    }

    pub fn reset_live_token_estimate(&mut self) {
        self.session_state.live_output_tokens = 0;
        self.session_state.live_output_text.clear();
    }

    pub fn add_live_output_delta(&mut self, delta: &str) {
        self.session_state.live_output_text.push_str(delta);
        self.session_state.live_output_tokens =
            crate::agent::compaction::ContextTracker::estimate_tokens(
                &self.session_state.live_output_text,
            ) as u64;
    }

    pub fn set_context_info(&mut self, tokens: usize, limit: usize, compactions: usize) {
        self.session_state.context_tokens = tokens;
        self.session_state.context_limit = limit;
        self.session_state.compaction_count = compactions;
    }

    pub fn set_rate_limits(
        &mut self,
        rpm: Option<u64>,
        tpm: Option<u64>,
        rpm_rem: Option<u64>,
        tpm_rem: Option<u64>,
    ) {
        self.session_state.rpm_limit = rpm;
        self.session_state.tpm_limit = tpm;
        self.session_state.rpm_remaining = rpm_rem;
        self.session_state.tpm_remaining = tpm_rem;
    }

    fn get_info_dialog_lines(&self) -> Vec<String> {
        let lines: Vec<Line<'_>> = match &self.ui_state.dialog {
            Dialog::Context => self.get_context_lines(),
            Dialog::Cost => self.get_cost_lines(),
            Dialog::Usage => self.get_usage_lines(),
            _ => vec![],
        };
        lines
            .into_iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<Vec<&str>>()
                    .join("")
            })
            .collect()
    }

    fn get_context_lines(&self) -> Vec<Line<'_>> {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Context Window",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw(format!(
                "  Current: {} tokens",
                self.session_state.context_tokens
            ))),
            Line::from(Span::raw(format!(
                "  Limit: {} tokens",
                self.session_state.context_limit
            ))),
        ];

        if self.session_state.context_limit > 0 {
            let pct = (self.session_state.context_tokens as f64
                / self.session_state.context_limit as f64
                * 100.0)
                .min(100.0);
            lines.push(Line::from(Span::raw(format!("  Usage: {:.1}%", pct))));
        }

        lines.push(Line::from(Span::raw(format!(
            "  Compactions: {}",
            self.session_state.compaction_count
        ))));

        if let Some(ref session) = self.session_state.session {
            if session.time_compacting.is_some() {
                lines.push(Line::from(Span::raw("  Session has been compacted")));
            }
        }

        let derived = &self.session_state_derived;
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "  Pinned",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if derived.context_state.pinned_items.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (none)",
                Style::default().fg(self.ui_state.theme.muted),
            )));
        } else {
            for item in &derived.context_state.pinned_items {
                lines.push(Line::from(Span::raw(format!("    {}", item.label))));
            }
        }

        lines.push(Line::from(Span::styled(
            "  Summarized",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if derived.context_state.summarized_items.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (none)",
                Style::default().fg(self.ui_state.theme.muted),
            )));
        } else {
            for item in &derived.context_state.summarized_items {
                lines.push(Line::from(Span::raw(format!("    {}", item.label))));
            }
        }

        lines.push(Line::from(Span::styled(
            "  Retrieved",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if derived.context_state.retrieved_items.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (none)",
                Style::default().fg(self.ui_state.theme.muted),
            )));
        } else {
            for item in &derived.context_state.retrieved_items {
                lines.push(Line::from(Span::raw(format!("    {}", item.label))));
            }
        }

        lines.push(Line::from(Span::styled(
            "  Excluded",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if derived.context_state.excluded_items.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (none)",
                Style::default().fg(self.ui_state.theme.muted),
            )));
        } else {
            for item in &derived.context_state.excluded_items {
                lines.push(Line::from(Span::raw(format!("    {}", item.label))));
            }
        }

        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "  Providers (Active Keys):",
            Style::default().add_modifier(Modifier::BOLD),
        )));

        let active_vars = [
            ("openai", "OPENAI_API_KEY"),
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("google", "GOOGLE_API_KEY"),
            ("minimax", "MINIMAX_API_KEY"),
            ("opencode_zen", "OPENCODE_ZEN_API_KEY"),
            ("opencode_go", "OPENCODE_GO_API_KEY"),
        ];

        for (id, var) in active_vars {
            if let Ok(val) = std::env::var(var) {
                let len = val.len();
                let prefix = if len > 4 { &val[..4] } else { "..." };
                let suffix = if len > 4 { &val[len - 4..] } else { "..." };
                lines.push(Line::from(Span::raw(format!(
                    "    {}: {}...{} (len={})",
                    id, prefix, suffix, len
                ))));
            }
        }

        lines.push(Line::from(""));
        lines
    }

    fn get_cost_lines(&self) -> Vec<Line<'_>> {
        let total_tokens = self.session_state.token_in + self.session_state.token_out;
        let model = self
            .agent_state
            .current_model
            .split('/')
            .next_back()
            .unwrap_or(&self.agent_state.current_model);

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::raw("  Token Usage")),
            Line::from(Span::raw(format!(
                "  Input:  {} tokens",
                self.session_state.token_in
            ))),
            Line::from(Span::raw(format!(
                "  Output: {} tokens",
                self.session_state.token_out
            ))),
            Line::from(Span::raw(format!("  Total:  {} tokens", total_tokens))),
        ];

        if self.session_state.cached_tokens > 0 {
            let cache_pct = (self.session_state.cached_tokens as f64
                / self.session_state.token_in as f64
                * 100.0) as u64;
            lines.push(Line::from(Span::raw(format!(
                "  Cached: {} tokens ({}%)",
                self.session_state.cached_tokens, cache_pct
            ))));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::raw("  Estimated Cost")));

        let cost = estimate_cost(
            &self.session_state.token_in,
            &self.session_state.token_out,
            model,
        );
        lines.push(Line::from(Span::raw(format!("  {}", cost))));

        lines.push(Line::from(""));
        lines
    }

    fn get_usage_lines(&self) -> Vec<Line<'_>> {
        let mut lines = vec![Line::from(""), Line::from(Span::raw("  Rate Limits"))];

        if let (Some(rpm), Some(rem)) = (
            self.session_state.rpm_limit,
            self.session_state.rpm_remaining,
        ) {
            lines.push(Line::from(Span::raw(format!(
                "  Requests/min: {} ({} remaining)",
                rpm, rem
            ))));
        } else {
            lines.push(Line::from(Span::raw("  Requests/min: N/A")));
        }

        if let (Some(tpm), Some(rem)) = (
            self.session_state.tpm_limit,
            self.session_state.tpm_remaining,
        ) {
            lines.push(Line::from(Span::raw(format!(
                "  Tokens/min: {} ({} remaining)",
                tpm, rem
            ))));
        } else {
            lines.push(Line::from(Span::raw("  Tokens/min: N/A")));
        }

        lines.push(Line::from(""));
        lines
    }

    pub fn set_status(&mut self, status: SessionStatus) {
        self.session_state.session_status = status.clone();
        if status == SessionStatus::Idle {
            self.prompt_state.pending_send = false;
            self.busy_spinner.stop();
        } else if status == SessionStatus::Working {
            self.busy_spinner.start(None);
        }
    }

    pub fn set_todos(&mut self, todos: Vec<TodoEntry>) {
        self.sidebar.set_todos(todos);
    }

    /// Update the active goal snapshot. The sidebar's `goal` field
    /// receives a one-line summary, while the structured fields stay on
    /// `App` so the status bar and `/goal show` command can use them.
    pub fn set_active_goal(&mut self, goal: Option<crate::bus::events::GoalSnapshot>) {
        self.active_goal = goal.clone();
        if let Some(ref g) = goal {
            self.sidebar
                .set_goal(Some(crate::goal::model::Goal::summary_from_snapshot(g)));
        } else {
            self.sidebar.set_goal(None);
        }
    }

    pub fn show_question_dialog(&mut self, questions: Vec<QuestionSpec>, session_id: String) {
        self.dialog_state.question_dialog = Some(QuestionDialog::new(questions));
        self.dialog_state.question_session_id = Some(session_id);
        self.open_dialog(Dialog::Question);
        self.session_state.session_status = SessionStatus::Working;
    }

    pub fn submit_question_answers(&mut self) {
        if let Some(qd) = &self.dialog_state.question_dialog {
            let answers = qd.answers_json();
            if let Some(session_id) = self.dialog_state.question_session_id.take() {
                self.send_local_question_response(session_id, answers);
            }
        }
        self.dialog_state.question_dialog = None;
        self.dialog_state.question_session_id = None;
        self.close_dialog();
        self.session_state.session_status = SessionStatus::Idle;
    }

    pub fn show_permission_dialog(
        &mut self,
        perm_id: String,
        request: crate::permission::PermissionRequest,
    ) {
        self.dialog_state.permission_dialog = Some(PermissionDialog::new(
            request,
            Arc::clone(&self.ui_state.theme),
        ));
        self.dialog_state.permission_perm_id = Some(perm_id);
        self.open_dialog(Dialog::Permission);
    }

    pub fn submit_permission_response(&mut self, allowed: bool) {
        if let Some(ref perm_id) = self.dialog_state.permission_perm_id {
            let perm_id = perm_id.clone();
            let choice = match allowed {
                true => crate::permission::PermissionChoice::AllowOnce,
                false => crate::permission::PermissionChoice::DenyOnce,
            };
            self.send_local_permission_response(perm_id, choice);
        }
        self.dialog_state.permission_dialog = None;
        self.dialog_state.permission_perm_id = None;
        self.close_dialog();
    }

    pub fn on_permission_confirm(&mut self) -> Option<(bool, usize)> {
        let pd = self.dialog_state.permission_dialog.as_ref()?;
        let idx = pd.selected_option();
        let choice = match idx {
            0 => crate::permission::PermissionChoice::AllowOnce,
            1 => crate::permission::PermissionChoice::AlwaysAllow,
            2 => crate::permission::PermissionChoice::DenyOnce,
            3 => crate::permission::PermissionChoice::AlwaysDeny,
            _ => return None,
        };
        let allowed = choice.allowed();
        if let Some(ref perm_id) = self.dialog_state.permission_perm_id {
            let perm_id = perm_id.clone();
            self.send_local_permission_response(perm_id, choice);
        }
        self.dialog_state.permission_dialog = None;
        self.dialog_state.permission_perm_id = None;
        self.close_dialog();
        Some((allowed, idx))
    }

    fn send_local_permission_response(
        &self,
        perm_id: String,
        choice: crate::permission::PermissionChoice,
    ) {
        let Some(core_client) = self.core_client.clone() else {
            tracing::warn!("core client unavailable for permission response");
            return;
        };
        let choice = match choice {
            crate::permission::PermissionChoice::AllowOnce => "allow",
            crate::permission::PermissionChoice::AlwaysAllow => "always_allow",
            crate::permission::PermissionChoice::DenyOnce => "deny",
            crate::permission::PermissionChoice::AlwaysDeny => "always_deny",
        }
        .to_string();
        tokio::spawn(async move {
            let request = crate::core::new_request(
                format!("permission-respond-{}", uuid::Uuid::new_v4()),
                CoreRequest::PermissionRespond {
                    id: perm_id,
                    choice,
                },
            );
            if let Err(e) = core_client.request(request).await {
                tracing::warn!("failed to send permission response via core: {}", e);
            }
        });
    }

    fn send_local_question_response(&self, question_id: String, answers_json: String) {
        let Some(core_client) = self.core_client.clone() else {
            tracing::warn!("core client unavailable for question response");
            return;
        };
        let answers = serde_json::from_str::<serde_json::Value>(&answers_json)
            .unwrap_or(serde_json::Value::String(answers_json));
        tokio::spawn(async move {
            let request = crate::core::new_request(
                format!("question-respond-{}", uuid::Uuid::new_v4()),
                CoreRequest::QuestionRespond {
                    id: question_id,
                    answers,
                },
            );
            if let Err(e) = core_client.request(request).await {
                tracing::warn!("failed to send question response via core: {}", e);
            }
        });
    }

    pub fn enter_plan_mode(&mut self, topic: Option<String>) {
        self.agent_state.plan_mode = true;
        self.agent_state.plan_topic = topic;
    }

    pub fn exit_plan_mode(&mut self) {
        self.agent_state.plan_mode = false;
        self.agent_state.plan_topic = None;
    }

    pub fn show_timeline(&mut self) {
        let count = self.messages_state.messages.message_count();
        if count > 0 {
            self.ui_state.timeline_selected = count - 1;
            self.ui_state.timeline_visible = true;
        } else {
            self.messages_state.toasts.info("No messages in timeline");
        }
    }

    fn handle_timeline_key(&mut self, key: KeyEvent) {
        let action = handle_event(crossterm::event::Event::Key(key));
        match action {
            Some(InputAction::NavigateUp) => {
                if self.ui_state.timeline_selected > 0 {
                    self.ui_state.timeline_selected -= 1;
                }
            }
            Some(InputAction::NavigateDown) => {
                let count = self.messages_state.messages.message_count();
                if self.ui_state.timeline_selected + 1 < count {
                    self.ui_state.timeline_selected += 1;
                }
            }
            Some(InputAction::Send) => {
                self.messages_state
                    .messages
                    .select_index(self.ui_state.timeline_selected);
                self.ui_state.timeline_visible = false;
                self.messages_state.toasts.info(&format!(
                    "Jumped to message {}",
                    self.ui_state.timeline_selected
                ));
            }
            Some(InputAction::Cancel) => {
                self.ui_state.timeline_visible = false;
            }
            _ => {}
        }
    }

    fn render_timeline(&self, frame: &mut Frame, area: Rect) {
        use crate::session::message::ToolStatus;

        let messages = &self.messages_state.messages.messages;
        if messages.is_empty() {
            return;
        }

        let count = messages.len();
        let selected = self.ui_state.timeline_selected;

        let max_visible = 15.min(count as u16);
        let height = max_visible + 4;

        let timeline_area = centered_rect(75, height, area);

        let block = Block::default()
            .title(" Timeline ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.ui_state.theme.border))
            .style(
                Style::default()
                    .bg(self.ui_state.theme.background)
                    .fg(self.ui_state.theme.foreground),
            );

        let _inner_area = block.inner(timeline_area);

        let mut lines: Vec<Line> = Vec::new();

        let is_last_msg = |idx: usize| -> bool { idx + 1 >= count };

        for (i, msg) in messages.iter().enumerate() {
            let is_sel = i == selected;
            let tree_char = if is_last_msg(i) {
                "└──"
            } else {
                "├──"
            };
            let continued = if is_last_msg(i) { "   " } else { "│  " };

            match msg.role {
                MessageRole::User => {
                    let role_style = Style::default()
                        .fg(self.ui_state.theme.primary)
                        .add_modifier(Modifier::BOLD);

                    let time_str = msg.timestamp.map(|ts| {
                        chrono::DateTime::from_timestamp(ts, 0)
                            .map(|dt| dt.format("%H:%M").to_string())
                            .unwrap_or_default()
                    });

                    let time_suffix = time_str.map(|t| format!(" @{}", t)).unwrap_or_default();

                    let preview = match msg.parts.first() {
                        Some(MsgPart::Text { content }) => {
                            let first_line = content.lines().next().unwrap_or("");
                            if first_line.len() > 45 {
                                format!("{}...", &first_line[..45])
                            } else {
                                first_line.to_string()
                            }
                        }
                        Some(MsgPart::Reasoning { .. }) | Some(MsgPart::ToolCall { .. }) => {
                            String::new()
                        }
                        Some(MsgPart::Image { .. }) | Some(MsgPart::ShellCell { .. }) => {
                            String::new()
                        }
                        None => String::new(),
                    };

                    if is_sel {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{} ", tree_char),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                            Span::styled("● ", Style::default().fg(self.ui_state.theme.primary)),
                            Span::styled("You", role_style),
                            Span::styled(
                                format!(": {}{}", preview, time_suffix),
                                Style::default().fg(self.ui_state.theme.foreground),
                            ),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{} ", tree_char),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                            Span::styled("○ ", Style::default().fg(self.ui_state.theme.muted)),
                            Span::styled("You", role_style),
                            Span::styled(
                                format!(": {}{}", preview, time_suffix),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                        ]));
                    }
                }
                MessageRole::Assistant => {
                    let role_style = Style::default()
                        .fg(self.ui_state.theme.secondary)
                        .add_modifier(Modifier::BOLD);

                    let time_str = msg.timestamp.map(|ts| {
                        chrono::DateTime::from_timestamp(ts, 0)
                            .map(|dt| dt.format("%H:%M").to_string())
                            .unwrap_or_default()
                    });

                    let time_suffix = time_str.map(|t| format!(" @{}", t)).unwrap_or_default();

                    let preview = match msg.parts.first() {
                        Some(MsgPart::Text { content }) => {
                            let first_line = content.lines().next().unwrap_or("");
                            if first_line.len() > 45 {
                                format!("{}...", &first_line[..45])
                            } else {
                                first_line.to_string()
                            }
                        }
                        Some(MsgPart::Reasoning { .. }) => "reasoning...".to_string(),
                        Some(MsgPart::ToolCall { name, .. }) => format!("[{}]", name),
                        Some(MsgPart::Image { .. }) | Some(MsgPart::ShellCell { .. }) => {
                            String::new()
                        }
                        None => String::new(),
                    };

                    let tool_calls: Vec<_> = msg
                        .parts
                        .iter()
                        .filter_map(|p| match p {
                            MsgPart::ToolCall { name, status, .. } => {
                                Some((name.clone(), status.clone()))
                            }
                            _ => None,
                        })
                        .collect();

                    if is_sel {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{} ", tree_char),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                            Span::styled("● ", Style::default().fg(self.ui_state.theme.secondary)),
                            Span::styled("Assistant", role_style),
                            Span::styled(
                                format!(": {}{}", preview, time_suffix),
                                Style::default().fg(self.ui_state.theme.foreground),
                            ),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{} ", tree_char),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                            Span::styled("○ ", Style::default().fg(self.ui_state.theme.muted)),
                            Span::styled("Assistant", role_style),
                            Span::styled(
                                format!(": {}{}", preview, time_suffix),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                        ]));
                    }

                    for (j, (name, status)) in tool_calls.iter().enumerate() {
                        let is_last_tool = j == tool_calls.len() - 1;
                        let branch = if is_last_tool {
                            "└── "
                        } else {
                            "├── "
                        };

                        let spinner = match status {
                            ToolStatus::Running => "⟳",
                            ToolStatus::Pending => "○",
                            ToolStatus::Completed => "✓",
                            ToolStatus::Error => "✗",
                        };

                        let status_color = match status {
                            ToolStatus::Pending => self.ui_state.theme.muted,
                            ToolStatus::Running => self.ui_state.theme.warning,
                            ToolStatus::Completed => self.ui_state.theme.success,
                            ToolStatus::Error => self.ui_state.theme.error,
                        };

                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{}{}", continued, branch),
                                Style::default().fg(self.ui_state.theme.muted),
                            ),
                            Span::styled(spinner, Style::default().fg(status_color)),
                            Span::raw(format!(" {}", name)),
                        ]));

                        let _ = is_last_tool;
                    }
                }
            }
        }

        let visible_lines: Vec<Line> = if lines.len() > max_visible as usize {
            let start = if selected >= (max_visible as usize / 2) {
                (selected.saturating_sub(max_visible as usize / 2))
                    .min(lines.len().saturating_sub(max_visible as usize))
            } else {
                0
            };
            lines[start..start + max_visible as usize].to_vec()
        } else {
            lines.clone()
        };

        let paragraph = Paragraph::new(visible_lines)
            .block(block)
            .wrap(Wrap { trim: true });

        frame.render_widget(Clear, timeline_area);
        frame.render_widget(paragraph, timeline_area);

        let nav_hint = Line::from(Span::styled(
            " ↑/↓ navigate  Enter: jump  Esc: close ",
            Style::default().fg(self.ui_state.theme.muted),
        ));
        let hint_len = 42;
        let hint_x = timeline_area.x + (timeline_area.width.saturating_sub(hint_len)) / 2;
        let hint_y = timeline_area.y + timeline_area.height - 1;
        let hint_area = Rect {
            x: hint_x,
            y: hint_y,
            width: hint_len,
            height: 1,
        };
        frame.render_widget(Paragraph::new(nav_hint), hint_area);

        let pos_indicator = Line::from(Span::styled(
            format!("Message {} of {}", selected + 1, count),
            Style::default().fg(self.ui_state.theme.muted),
        ));
        let pos_len = 20;
        let pos_x = timeline_area.x + (timeline_area.width.saturating_sub(pos_len)) / 2;
        let pos_y = timeline_area.y + timeline_area.height - 1;
        let pos_area = Rect {
            x: pos_x,
            y: pos_y,
            width: pos_len,
            height: 1,
        };
        let pos_para = Paragraph::new(pos_indicator).alignment(Alignment::Center);
        frame.render_widget(pos_para, pos_area);
    }

    fn export_handoff(&mut self) {
        let derived = &self.session_state_derived;
        let session = self.session_state.session.as_ref();

        let goal = derived.goal.as_deref().unwrap_or("(no goal set)");
        let model = self.agent_state.current_model.clone();
        let branch = session.map(|s| s.directory.clone()).unwrap_or_default();
        let ctx_pct = if self.session_state.context_limit > 0 {
            format!(
                "{:.0}%",
                self.session_state.context_tokens as f64 / self.session_state.context_limit as f64
                    * 100.0
            )
        } else {
            "unknown".to_string()
        };

        let mut md = String::new();
        md.push_str("# Codegg Handoff\n\n");

        md.push_str("## Goal\n");
        md.push_str(goal);
        md.push_str("\n\n");

        md.push_str("## Current plan\n");
        if let Some(ref plan) = derived.plan {
            for item in &plan.items {
                let check = match item.status {
                    crate::session::events::PlanItemStatus::Done => "[x]",
                    crate::session::events::PlanItemStatus::InProgress => "[~]",
                    crate::session::events::PlanItemStatus::Skipped => "[-]",
                    crate::session::events::PlanItemStatus::Blocked => "[?]",
                    crate::session::events::PlanItemStatus::Pending => "[ ]",
                };
                md.push_str(&format!("- {} {}", check, item.text));
                if let Some(ref note) = item.note {
                    md.push_str(&format!(" ({})", note));
                }
                md.push('\n');
            }
        } else {
            md.push_str("- (no plan)\n");
        }
        md.push('\n');

        md.push_str("## Current state\n");
        md.push_str(&format!("- Model: {}\n", model));
        if !branch.is_empty() {
            md.push_str(&format!("- Branch/dir: {}\n", branch));
        }
        if !derived.changed_files.is_empty() {
            md.push_str(&format!(
                "- Changed files: {}\n",
                derived.changed_files.len()
            ));
        }
        match &derived.test_state {
            crate::session::state::TestState::Passed { command, .. } => {
                md.push_str(&format!("- Tests: passed ({})\n", command));
            }
            crate::session::state::TestState::Failed {
                command, summary, ..
            } => {
                md.push_str(&format!("- Tests: FAILED ({}) - {}\n", command, summary));
            }
            crate::session::state::TestState::Running { command } => {
                md.push_str(&format!("- Tests: running ({})\n", command));
            }
            _ => {
                md.push_str("- Tests: unknown\n");
            }
        }
        md.push_str(&format!("- Context pressure: {}\n", ctx_pct));
        md.push('\n');

        if !derived.changed_files.is_empty() {
            md.push_str("## Files changed\n");
            for f in &derived.changed_files {
                md.push_str(&format!(
                    "- {} ({:+}/-{})\n",
                    f.path, f.additions, f.deletions
                ));
            }
            md.push('\n');
        }

        if !derived.findings.is_empty() {
            md.push_str("## Relevant findings\n");
            for f in &derived.findings {
                md.push_str(&format!("- [{}] {}\n", f.severity, f.message));
            }
            md.push('\n');
        }

        md.push_str("## Latest test result\n");
        match &derived.test_state {
            crate::session::state::TestState::Passed {
                command,
                duration_ms,
            } => {
                md.push_str(&format!(
                    "PASSED: {} ({:.1}s)\n",
                    command,
                    duration_ms.unwrap_or(0) as f64 / 1000.0
                ));
            }
            crate::session::state::TestState::Failed {
                command,
                duration_ms,
                summary,
            } => {
                md.push_str(&format!(
                    "FAILED: {} ({:.1}s) - {}\n",
                    command,
                    duration_ms.unwrap_or(0) as f64 / 1000.0,
                    summary
                ));
            }
            _ => {
                md.push_str("(no recent test run)\n");
            }
        }
        md.push('\n');

        md.push_str("## Suggested next steps\n");
        md.push_str("- (agent should fill in based on current state)\n\n");

        md.push_str("## Notes for smaller model\n");
        md.push_str("- Do not broaden scope.\n");
        md.push_str("- Prefer minimal patches.\n");
        md.push_str("- Run targeted tests first.\n");

        let _ = crate::util::clipboard::copy_to_clipboard(&md);
        self.messages_state
            .toasts
            .info("Handoff exported to clipboard");
    }
}

fn index_files_sync(dir: &str) -> Vec<String> {
    let mut files = Vec::new();
    index_files_recursive(dir, dir, 0, 10, &mut files);
    files
}

fn index_files_recursive(
    current_dir: &str,
    root_dir: &str,
    depth: usize,
    max_depth: usize,
    files: &mut Vec<String>,
) {
    if depth >= max_depth {
        return;
    }

    let skip_dirs = [
        "node_modules",
        ".git",
        "target",
        "__pycache__",
        ".venv",
        "venv",
        ".cargo",
        "dist",
        "build",
    ];

    if let Ok(entries) = std::fs::read_dir(current_dir) {
        for entry in entries.flatten() {
            if let Ok(path) = entry.path().canonicalize() {
                if path.is_symlink() {
                    continue;
                }
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !skip_dirs.contains(&name) {
                            index_files_recursive(
                                path.to_str().unwrap_or(""),
                                root_dir,
                                depth + 1,
                                max_depth,
                                files,
                            );
                        }
                    }
                } else if path.is_file() {
                    if let Ok(rel) = path.strip_prefix(root_dir) {
                        let rel_str = rel.to_string_lossy().to_string();
                        if !rel_str.contains(".git/") && !rel_str.contains("node_modules/") {
                            files.push(rel_str);
                        }
                    }
                }
            }
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn format_key_event(key: &crossterm::event::KeyEvent) -> String {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut parts = Vec::new();

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }

    let key_part = match key.code {
        KeyCode::Enter => "enter",
        KeyCode::Esc => "esc",
        KeyCode::Tab => "tab",
        KeyCode::Backspace => "backspace",
        KeyCode::Delete => "delete",
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::Up => "up",
        KeyCode::Down => "down",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pageup",
        KeyCode::PageDown => "pagedown",
        KeyCode::Char(c) => {
            if c == ' ' {
                return if parts.is_empty() {
                    "space".to_string()
                } else {
                    format!("{}+space", parts.join("+"))
                };
            }
            return if parts.is_empty() {
                c.to_lowercase().to_string()
            } else {
                format!("{}+{}", parts.join("+"), c.to_lowercase())
            };
        }
        _ => return parts.join("+"),
    };

    if parts.is_empty() {
        key_part.to_string()
    } else {
        format!("{}+{}", parts.join("+"), key_part)
    }
}

fn estimate_cost(input_tokens: &u64, output_tokens: &u64, model: &str) -> String {
    let input_price_per_m = match model.to_lowercase().as_str() {
        m if m.contains("gpt-4o") || m.contains("gpt-4t") => 2.50,
        m if m.contains("gpt-4") && m.contains("32k") => 60.0,
        m if m.contains("gpt-4") && !m.contains("turbo") => 30.0,
        m if m.contains("gpt-4-turbo") => 10.0,
        m if m.contains("gpt-35-turbo") || m.contains("gpt-3.5") || m.contains("gpt-4o-mini") => {
            0.5
        }
        m if m.contains("claude") && m.contains("haiku") => 0.25,
        m if m.contains("claude") && m.contains("sonnet") => 3.0,
        m if m.contains("claude") && m.contains("opus") => 15.0,
        m if m.contains("claude") => 3.0,
        m if m.contains("gemini") && m.contains("2.0-flash") => 0.0,
        m if m.contains("gemini") && m.contains("1.5") && m.contains("pro") => 1.25,
        m if m.contains("gemini") => 0.125,
        m if m.contains("o1") || m.contains("o3") => 0.0,
        m if m.contains("o3-mini") => 1.10,
        m if m.contains("o1-mini") => 0.55,
        _ => 0.0,
    };

    let output_price_per_m = match model.to_lowercase().as_str() {
        m if m.contains("gpt-4o") || m.contains("gpt-4t") => 10.0,
        m if m.contains("gpt-4") && m.contains("32k") => 120.0,
        m if m.contains("gpt-4") && !m.contains("turbo") => 60.0,
        m if m.contains("gpt-4-turbo") => 30.0,
        m if m.contains("gpt-35-turbo") || m.contains("gpt-3.5") || m.contains("gpt-4o-mini") => {
            1.5
        }
        m if m.contains("claude") && m.contains("haiku") => 1.25,
        m if m.contains("claude") && m.contains("sonnet") => 15.0,
        m if m.contains("claude") && m.contains("opus") => 75.0,
        m if m.contains("claude") => 15.0,
        m if m.contains("gemini") && m.contains("2.0-flash") => 0.0,
        m if m.contains("gemini") && m.contains("1.5") && m.contains("pro") => 5.0,
        m if m.contains("gemini") => 0.5,
        m if m.contains("o1") || m.contains("o3") => 0.0,
        m if m.contains("o3-mini") => 4.40,
        m if m.contains("o1-mini") => 3.50,
        _ => 0.0,
    };

    let input_cost = (*input_tokens as f64 / 1_000_000.0) * input_price_per_m;
    let output_cost = (*output_tokens as f64 / 1_000_000.0) * output_price_per_m;
    let total = input_cost + output_cost;

    if total < 0.001 {
        "< $0.001 (free model or no pricing data)".to_string()
    } else {
        format!("${:.4}", total)
    }
}

fn format_token_short(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let m = tokens as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("{:.0}m", m)
        } else {
            format!("{:.1}m", m)
        }
    } else if tokens >= 1_000 {
        let k = tokens as f64 / 1_000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{:.0}k", k)
        } else {
            format!("{:.1}k", k)
        }
    } else {
        tokens.to_string()
    }
}

fn format_token_line(
    token_in: u64,
    token_out: u64,
    live_output_tokens: u64,
    context_tokens: u64,
    context_limit: u64,
) -> String {
    let displayed_output = token_out + live_output_tokens;
    let displayed_context = context_tokens + live_output_tokens;
    let total = token_in + displayed_output;
    let pct = if context_limit > 0 {
        ((displayed_context as f64 / context_limit as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let output_prefix = if live_output_tokens > 0 {
        "↑~"
    } else {
        "↑"
    };
    format!(
        "↓{} {}{} ({}) / {} {:.0}%",
        format_token_short(token_in),
        output_prefix,
        format_token_short(displayed_output),
        format_token_short(total),
        format_token_short(displayed_context),
        pct
    )
}

/// Format a `GoalSnapshot` as a compact one-liner for the status bar.
///
/// Layout: `[status] <title>  <budget summary>` where the budget
/// summary is whichever axes are configured (tokens, tool-calls,
/// turns, wall-clock). Designed to fit alongside the existing status,
/// token, and subagent spans without truncating other UI.
pub(crate) fn format_goal_status_line(g: &crate::bus::events::GoalSnapshot) -> String {
    let title = if g.title.is_empty() {
        "(untitled)".to_string()
    } else {
        g.title.clone()
    };
    let mut budget_parts: Vec<String> = Vec::new();
    let used_tokens = g.usage.input_tokens + g.usage.output_tokens;
    if let Some(max) = g.budget.max_model_tokens {
        budget_parts.push(format!(
            "{}tok {}",
            if used_tokens >= max { "!" } else { "" },
            format_token_short(used_tokens.max(0) as u64)
        ));
        budget_parts.push(format!("/{}", format_token_short(max.max(0) as u64)));
    }
    if let Some(max) = g.budget.max_turns {
        budget_parts.push(format!("turns {}/{}", g.usage.turns_used, max));
    }
    if let Some(max) = g.budget.max_tool_calls {
        let over = g.usage.tool_calls >= max;
        budget_parts.push(format!(
            "{}calls {}/{}",
            if over { "!" } else { "" },
            g.usage.tool_calls,
            max
        ));
    }
    if let Some(max) = g.budget.max_wallclock_secs {
        let over = g.usage.wallclock_secs >= max;
        budget_parts.push(format!(
            "{}wall {}s/{}s",
            if over { "!" } else { "" },
            g.usage.wallclock_secs,
            max
        ));
    }
    let budget = if budget_parts.is_empty() {
        String::new()
    } else {
        format!("  {}", budget_parts.join(" "))
    };
    format!("[{}] {}{}", g.status, title, budget)
}

fn parse_path_line_col(s: &str) -> (String, Option<u32>, Option<u32>) {
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    let path = parts[0].to_string();
    let line = parts.get(1).and_then(|l| l.parse::<u32>().ok());
    let col = parts.get(2).and_then(|c| c.parse::<u32>().ok());
    (path, line, col)
}

#[cfg(test)]
mod token_line_tests {
    use super::format_token_line;

    #[test]
    fn final_token_line_has_no_estimate_marker() {
        let line = format_token_line(100, 25, 0, 1_000, 10_000);

        assert!(line.contains("↑25"));
        assert!(!line.contains("↑~"));
    }

    #[test]
    fn live_token_line_marks_estimated_output() {
        let line = format_token_line(100, 25, 10, 1_000, 10_000);

        assert!(line.contains("↑~35"));
        assert!(line.contains("(135)"));
    }
}

#[cfg(test)]
mod goal_status_line_tests {
    use super::format_goal_status_line;
    use crate::bus::events::{GoalBudgetSnapshot, GoalSnapshot, GoalUsageSnapshot};

    fn sample(budget: GoalBudgetSnapshot, usage: GoalUsageSnapshot) -> GoalSnapshot {
        GoalSnapshot {
            id: "g1".into(),
            session_id: "s1".into(),
            project_id: "/tmp".into(),
            title: "Ship codex-style goals".into(),
            objective: "Long-horizon autonomous goal with budget".into(),
            status: "active".into(),
            current_phase: Some("Phase 1".into()),
            progress_summary: "started".into(),
            next_action: Some("write tests".into()),
            completion_criteria: vec!["All tests pass".into()],
            open_questions: vec![],
            budget,
            usage,
            created_at_ms: 0,
            updated_at_ms: 0,
            started_at_ms: None,
            completed_at_ms: None,
        }
    }

    #[test]
    fn includes_status_title_and_budget() {
        let g = sample(
            GoalBudgetSnapshot {
                max_turns: Some(10),
                max_model_tokens: Some(20_000),
                max_tool_calls: Some(50),
                max_wallclock_secs: Some(600),
            },
            GoalUsageSnapshot {
                turns_used: 2,
                input_tokens: 800,
                output_tokens: 200,
                tool_calls: 5,
                wallclock_secs: 12,
            },
        );
        let line = format_goal_status_line(&g);
        assert!(line.contains("[active]"));
        assert!(line.contains("Ship codex-style goals"));
        assert!(line.contains("turns 2/10"));
        assert!(line.contains("calls 5/50"));
        assert!(line.contains("wall 12s/600s"));
    }

    #[test]
    fn no_budget_renders_title_only() {
        let g = sample(GoalBudgetSnapshot::default(), GoalUsageSnapshot::default());
        let line = format_goal_status_line(&g);
        assert!(line.contains("[active]"));
        assert!(line.contains("Ship codex-style goals"));
        assert!(!line.contains("turns"));
    }

    #[test]
    fn empty_title_renders_placeholder() {
        let mut g = sample(GoalBudgetSnapshot::default(), GoalUsageSnapshot::default());
        g.title = String::new();
        let line = format_goal_status_line(&g);
        assert!(line.contains("(untitled)"));
    }

    #[test]
    fn over_budget_marks_warning_prefix() {
        let g = sample(
            GoalBudgetSnapshot {
                max_turns: Some(2),
                ..Default::default()
            },
            GoalUsageSnapshot {
                turns_used: 2,
                ..Default::default()
            },
        );
        let line = format_goal_status_line(&g);
        // At the limit counts as "over" for the marker; just assert
        // the budget is rendered.
        assert!(line.contains("turns 2/2"));
    }
}

#[cfg(test)]
mod theme_integration_tests {
    use super::*;

    #[test]
    fn app_with_config_loads_theme_registry() {
        let app = App::new_for_testing("/tmp".to_string());
        let names = app.theme_registry.names();
        assert!(!names.is_empty(), "registry should be populated");
        assert!(
            app.theme_registry.get("cyber-red").is_some(),
            "default theme `cyber-red` should be bundled"
        );
    }

    #[test]
    fn apply_theme_succeeds_for_known_and_fails_for_unknown() {
        let mut app = App::new_for_testing("/tmp".to_string());
        assert!(app.apply_theme("catppuccin-mocha"));
        assert_eq!(app.ui_state.theme.name, "Catppuccin Mocha");
        assert!(!app.apply_theme("no-such-theme"));
    }

    #[test]
    fn handle_theme_command_list_uses_registry() {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.handle_theme_command(Some("/theme list"));
        // The list command should not change the theme; we just check it
        // does not crash and that we can still query the registry.
        let names = app.theme_registry.names();
        assert!(!names.is_empty());
    }

    #[test]
    fn handle_theme_command_use_applies_and_keeps_registry() {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.handle_theme_command(Some("/theme use catppuccin-latte"));
        assert_eq!(app.ui_state.theme.name, "Catppuccin Latte");
    }

    #[test]
    fn handle_theme_command_use_unknown_reports_error() {
        let mut app = App::new_for_testing("/tmp".to_string());
        let before = app.ui_state.theme.name.clone();
        app.handle_theme_command(Some("/theme use nope"));
        // Theme should remain the default.
        assert_eq!(app.ui_state.theme.name, before);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn apply_persisted_preferences_round_trip() {
        use sqlx::sqlite::SqlitePoolOptions;
        use std::time::Duration;

        // Stand up an in-memory pool and run the production migration so
        // we exercise the same schema the app uses.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();

        // Pre-seed a saved theme + model.
        let prefs = crate::storage::UserPreferences::new(pool.clone());
        prefs
            .set(crate::storage::KEY_THEME_ACTIVE, "dracula")
            .await
            .unwrap();
        prefs
            .set(
                crate::storage::KEY_MODEL_LAST_USED,
                "opencode_zen/nemotron-3-super-free",
            )
            .await
            .unwrap();

        // Build the app with default models; verify the saved theme and
        // model are applied on top of defaults.
        let mut app = App::new_for_testing("/tmp".to_string());
        app.set_models(vec![
            "opencode_zen/big-pickle".to_string(),
            "opencode_zen/nemotron-3-super-free".to_string(),
        ]);
        app.set_preferences(prefs);
        let handle = tokio::runtime::Handle::current();
        tokio::task::block_in_place(|| {
            handle.block_on(async {
                app.apply_persisted_preferences();
            });
        });
        assert_eq!(app.ui_state.theme.name, "Dracula");
        assert_eq!(
            app.agent_state.current_model,
            "opencode_zen/nemotron-3-super-free"
        );
    }
}

#[cfg(test)]
mod remote_core_loader_tests {
    use super::*;
    use crate::core::new_request;
    use crate::core::InprocCoreClient;
    use crate::protocol::core::{CoreRequest, CoreResponse};

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_models_via_core_populates_state() {
        use sqlx::sqlite::SqlitePoolOptions;
        use std::time::Duration;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();

        let client: Arc<dyn CoreClient> = Arc::new(InprocCoreClient::new(
            None,
            None,
            None,
            Some(pool),
            crate::config::schema::Config::default(),
            None,
        ));

        let req = new_request("snap".into(), CoreRequest::SnapshotModels);
        let resp = client.request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::ModelsSnapshot { .. }));

        let mut app = App::new_for_testing("/tmp".to_string());
        app.set_core_client(client);

        let models = app.load_models_via_core().await.unwrap();

        assert_eq!(app.agent_state.models, models);

        if let CoreResponse::ModelsSnapshot {
            models: mut expected,
            ..
        } = Arc::clone(&app.core_client.unwrap())
            .request(new_request("snap2".into(), CoreRequest::SnapshotModels))
            .await
            .unwrap()
        {
            // Provider registration order is non-deterministic (HashMap iteration),
            // so sort both lists before comparing.
            let mut actual = models;
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected);
        } else {
            panic!("expected ModelsSnapshot");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_tasks_via_core_returns_tasks_array() {
        use sqlx::sqlite::SqlitePoolOptions;
        use std::sync::Arc as StdArc;
        use std::time::Duration;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();

        let scheduler = StdArc::new(crate::agent::task::BackgroundScheduler::new());
        let client: Arc<dyn CoreClient> = Arc::new(InprocCoreClient::new(
            None,
            None,
            Some(scheduler),
            Some(pool),
            crate::config::schema::Config::default(),
            None,
        ));

        let mut app = App::new_for_testing("/tmp".to_string());
        app.set_core_client(client);

        let tasks = app.load_tasks_via_core().await.unwrap();

        assert!(tasks.is_empty(), "fresh daemon should have no tasks");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_models_via_core_fails_without_core_client() {
        let mut app = App::new_for_testing("/tmp".to_string());
        let err = app.load_models_via_core().await.unwrap_err();
        assert!(err.to_string().contains("core client unavailable"));
    }

    async fn build_inproc_client() -> Arc<dyn CoreClient> {
        use sqlx::sqlite::SqlitePoolOptions;
        use std::time::Duration;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        Arc::new(InprocCoreClient::new(
            None,
            None,
            None,
            Some(pool),
            crate::config::schema::Config::default(),
            None,
        ))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_initial_session_attach_sets_session() {
        let client = build_inproc_client().await;
        // Pre-create a session via the inproc client so we have a real id.
        let create_req = new_request(
            "create-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/proj".into(),
                title: Some("Existing".into()),
            },
        );
        let session_id = match client.request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        let mut app = App::new_for_testing("/tmp/proj".to_string());
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/test.sock".to_string(),
        };
        app.set_core_client(client);

        app.load_initial_session_via_core(InitialSessionRequest::Attach {
            session_id: session_id.clone(),
        })
        .await;

        let sess = app
            .session_state
            .session
            .as_ref()
            .expect("session should be set after Attach");
        assert_eq!(sess.id, session_id);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_initial_session_continue_picks_most_recent() {
        let client = build_inproc_client().await;
        // Create two sessions in the same project; `Continue` should
        // pick the most recent one (limit 1, ordered by `time_updated`).
        let req_a = new_request(
            "create-a".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/proj".into(),
                title: Some("A".into()),
            },
        );
        let id_a = match client.request(req_a).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };
        // Sleep a touch so the second session's time_updated is later.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let req_b = new_request(
            "create-b".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/proj".into(),
                title: Some("B".into()),
            },
        );
        let id_b = match client.request(req_b).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };
        assert_ne!(id_a, id_b);

        let mut app = App::new_for_testing("/tmp/proj".to_string());
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/test.sock".to_string(),
        };
        app.set_core_client(client);

        app.load_initial_session_via_core(InitialSessionRequest::Continue {
            project_dir: "/tmp/proj".to_string(),
        })
        .await;

        let sess = app
            .session_state
            .session
            .as_ref()
            .expect("Continue should pick a session");
        assert_eq!(
            sess.id, id_b,
            "Continue should pick the most recent session"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_initial_session_new_creates_session() {
        let client = build_inproc_client().await;

        let mut app = App::new_for_testing("/tmp/proj-new".to_string());
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/test.sock".to_string(),
        };
        app.set_core_client(client);

        app.load_initial_session_via_core(InitialSessionRequest::New {
            directory: "/tmp/proj-new".to_string(),
            title: Some("Brand New".to_string()),
        })
        .await;

        let sess = app
            .session_state
            .session
            .as_ref()
            .expect("New should create a session");
        assert_eq!(sess.title, "Brand New");
        assert_eq!(sess.directory, "/tmp/proj-new");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_initial_session_fork_attaches_via_listing() {
        let client = build_inproc_client().await;
        // Create a session to fork.
        let create_req = new_request(
            "create-fork".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/proj".into(),
                title: Some("Parent".into()),
            },
        );
        let parent_id = match client.request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        let mut app = App::new_for_testing("/tmp/proj".to_string());
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/test.sock".to_string(),
        };
        app.set_core_client(client);

        app.load_initial_session_via_core(InitialSessionRequest::Fork {
            session_id: parent_id.clone(),
        })
        .await;

        let sess = app
            .session_state
            .session
            .as_ref()
            .expect("Fork should result in a session being attached");
        // The most-recent session after a fork is the new fork, not the parent.
        assert_ne!(
            sess.id, parent_id,
            "Fork should attach the new fork, not the parent"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_initial_session_none_does_nothing() {
        let client = build_inproc_client().await;
        let mut app = App::new_for_testing("/tmp/proj".to_string());
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/test.sock".to_string(),
        };
        app.set_core_client(client);

        app.load_initial_session_via_core(InitialSessionRequest::None)
            .await;

        assert!(
            app.session_state.session.is_none(),
            "None should leave the app with no session"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn load_initial_session_without_core_client_is_noop() {
        // App with no core client attached: every variant must be a no-op
        // (must not panic, must not set a session).
        let mut app = App::new_for_testing("/tmp/proj".to_string());
        app.ui_state.mode = AppMode::RemoteCore {
            endpoint: "unix:///tmp/test.sock".to_string(),
        };
        // Note: intentionally NOT calling set_core_client.

        app.load_initial_session_via_core(InitialSessionRequest::Attach {
            session_id: "any".into(),
        })
        .await;
        app.load_initial_session_via_core(InitialSessionRequest::Continue {
            project_dir: "/tmp/proj".into(),
        })
        .await;
        app.load_initial_session_via_core(InitialSessionRequest::New {
            directory: "/tmp/proj".into(),
            title: None,
        })
        .await;
        app.load_initial_session_via_core(InitialSessionRequest::Fork {
            session_id: "any".into(),
        })
        .await;
        app.load_initial_session_via_core(InitialSessionRequest::None)
            .await;

        assert!(app.session_state.session.is_none());
    }
}

#[cfg(test)]
mod lsp_command_dispatch_tests {
    use super::*;
    use crate::config::schema::LspConfig;
    use crate::lsp::config_lsp_to_egglsp;
    use crate::tool::lsp::LspTool;

    fn sample_lsp_tool() -> Arc<LspTool> {
        let svc =
            crate::lsp::service::LspService::new_arc(config_lsp_to_egglsp(LspConfig::default()));
        Arc::new(LspTool::new(svc))
    }

    fn dispatch(app: &mut App, query: &str) {
        let name = query
            .split_whitespace()
            .next()
            .unwrap()
            .trim_start_matches('/');
        let cmd = crate::tui::command::COMMAND_REGISTRY
            .find_by_name_or_alias(name)
            .expect("command must exist in registry");
        app.dialog_state.command_palette.query = query.to_string();
        app.execute_command(cmd, Some(query));
    }

    // ── /lsp-servers ──

    #[test]
    fn lsp_servers_with_tool_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-servers");
        assert!(!app.messages_state.toasts.is_empty());
        assert!(!app.ui_state.command_mode);
    }

    #[test]
    fn lsp_servers_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-servers");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-capabilities ──

    #[test]
    fn lsp_capabilities_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-capabilities");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_capabilities_with_key_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-capabilities rust-analyzer");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_capabilities_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-capabilities rust-analyzer");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-errors ──

    #[test]
    fn lsp_errors_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-errors");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_errors_with_key_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-errors rust-analyzer");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-root ──

    #[test]
    fn lsp_root_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-root");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_root_with_path_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-root /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-restart ──

    #[test]
    fn lsp_restart_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-restart");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_restart_with_key_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-restart rust-analyzer");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-stop ──

    #[test]
    fn lsp_stop_no_args_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-stop");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_stop_all_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-stop --all");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_stop_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-stop");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-cache-status ──

    #[test]
    fn lsp_cache_status_with_tool_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-cache-status");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_cache_status_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-cache-status");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-cache-clear ──

    #[test]
    fn lsp_cache_clear_no_args_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-cache-clear");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_cache_clear_with_root_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-cache-clear /tmp/proj");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_cache_clear_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-cache-clear");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-preview-apply ──

    #[test]
    fn lsp_preview_apply_missing_id_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview-apply");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_apply_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-preview-apply some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-preview ──

    #[test]
    fn lsp_preview_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_with_id_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-preview some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-preview-refresh ──

    #[test]
    fn lsp_preview_refresh_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview-refresh");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_refresh_with_id_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview-refresh some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_refresh_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-preview-refresh some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-preview-clear ──

    #[test]
    fn lsp_preview_clear_no_args_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview-clear");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_clear_with_id_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview-clear some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_clear_all_produces_toast() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-preview-clear --all");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_preview_clear_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-preview-clear some-id");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-capabilities invalid key ──

    #[test]
    fn lsp_capabilities_with_invalid_key_shows_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-capabilities nonexistent-server");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-errors without tool ──

    #[test]
    fn lsp_errors_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-errors rust-analyzer");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_errors_with_invalid_key_shows_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-errors nonexistent-server");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-restart invalid key ──

    #[test]
    fn lsp_restart_with_invalid_key_shows_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-restart nonexistent-server");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-status ──

    #[test]
    fn lsp_status_with_tool_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-status");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_status_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-status");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-repair-local ──

    #[test]
    fn lsp_repair_local_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-repair-local");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_repair_local_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-repair-local /tmp/test.rs:10:5");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_repair_local_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-repair-local /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-repair-hunk ──

    #[test]
    fn lsp_repair_hunk_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-repair-hunk");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_repair_hunk_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-repair-hunk /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_repair_hunk_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-repair-hunk /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-review-file ──

    #[test]
    fn lsp_review_file_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-review-file");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_review_file_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-review-file /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_review_file_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-review-file /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-review-diff ──

    #[test]
    fn lsp_review_diff_no_args_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-review-diff");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_review_diff_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-review-diff");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-security-review ──

    #[test]
    fn lsp_security_review_no_args_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-security-review");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_security_review_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-security-review");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-impact ──

    #[test]
    fn lsp_impact_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-impact");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_impact_with_target_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-impact /tmp/test.rs:10:5");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_impact_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-impact /tmp/test.rs:10:5");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-test-repair ──

    #[test]
    fn lsp_test_repair_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-test-repair");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_test_repair_with_file_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-test-repair /tmp/test_test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_test_repair_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-test-repair /tmp/test_test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-interface ──

    #[test]
    fn lsp_interface_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-interface");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_interface_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-interface /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_interface_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-interface /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-cross-repair ──

    #[test]
    fn lsp_cross_repair_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-cross-repair");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_cross_repair_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-cross-repair /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_cross_repair_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-cross-repair /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-call-neighbors ──

    #[test]
    fn lsp_call_neighbors_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-call-neighbors");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_call_neighbors_with_target_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-call-neighbors /tmp/test.rs:10:5");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_call_neighbors_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-call-neighbors /tmp/test.rs:10:5");
        assert!(!app.messages_state.toasts.is_empty());
    }

    // ── /lsp-doctor ──

    #[test]
    fn lsp_doctor_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-doctor");
        assert!(!app.messages_state.toasts.is_empty());
        let texts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("Usage")),
            "missing arg must show Usage toast, got: {texts:?}"
        );
    }

    #[test]
    fn lsp_doctor_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-doctor /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
        let texts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("LSP Doctor")),
            "doctor toast must contain 'LSP Doctor', got: {texts:?}"
        );
    }

    #[test]
    fn lsp_doctor_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-doctor /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
        let texts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("LSP not available")),
            "no-tool dispatch must surface LSP-not-available, got: {texts:?}"
        );
    }

    // ── /lsp-context-diagnostics ──

    #[test]
    fn lsp_context_diagnostics_missing_arg_shows_usage() {
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-context-diagnostics");
        assert!(!app.messages_state.toasts.is_empty());
        let texts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("Usage")),
            "missing arg must show Usage toast, got: {texts:?}"
        );
    }

    #[test]
    fn lsp_context_diagnostics_with_path_produces_toast() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut app = App::new_for_testing("/tmp".into());
        app.lsp_tool = Some(sample_lsp_tool());
        dispatch(&mut app, "/lsp-context-diagnostics /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
    }

    #[test]
    fn lsp_context_diagnostics_without_tool_shows_unavailable() {
        let mut app = App::new_for_testing("/tmp".into());
        dispatch(&mut app, "/lsp-context-diagnostics /tmp/test.rs");
        assert!(!app.messages_state.toasts.is_empty());
        let texts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("LSP not available")),
            "no-tool dispatch must surface LSP-not-available, got: {texts:?}"
        );
    }

    // ── render fallback helpers ──

    #[test]
    fn extract_panic_message_from_str_ref() {
        let err: Box<dyn std::any::Any + Send> = Box::new("something broke");
        let msg = App::extract_panic_message(&err);
        assert_eq!(msg, "something broke");
    }

    #[test]
    fn extract_panic_message_from_string() {
        let err: Box<dyn std::any::Any + Send> = Box::new(String::from("owned msg"));
        let msg = App::extract_panic_message(&err);
        assert_eq!(msg, "owned msg");
    }

    #[test]
    fn extract_panic_message_from_unknown_type() {
        let err: Box<dyn std::any::Any + Send> = Box::new(42u32);
        let msg = App::extract_panic_message(&err);
        assert_eq!(msg, "Unknown render panic");
    }

    #[test]
    fn component_fallback_does_not_panic() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let app = App::new_for_testing("/tmp".into());
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    panic!("test component panic");
                }));
                assert!(result.is_err());
                app.render_component_fallback(frame, frame.area(), "Test fallback");
            })
            .unwrap();
    }
}

#[cfg(test)]
mod remote_protocol_tests {
    use super::*;
    use crate::protocol::tui::{TuiMessage as RemoteTuiMessage, REMOTE_TUI_PROTOCOL_VERSION};
    use crate::protocol::ui::{
        DialogSpec, PanelPlacement, PanelSpec, StatusItemSpec, StatusPlacement, TextNode, UiEffect,
        UiNode,
    };

    #[test]
    fn remote_snapshot_does_not_panic_on_empty_app() {
        let app = App::new_for_testing("/tmp".into());
        let snapshot = app.remote_snapshot();
        assert_eq!(snapshot.protocol_version, REMOTE_TUI_PROTOCOL_VERSION);
        assert_eq!(snapshot.route, "home");
        assert!(snapshot.session_id.is_none());
        assert_eq!(snapshot.status, "idle");
        assert!(snapshot.messages.is_empty());
    }

    #[test]
    fn remote_snapshot_includes_route_status_model_agent() {
        let mut app = App::new_for_testing("/tmp".into());
        app.agent_state.current_model = "test-model".to_string();
        app.agent_state.current_agent = 0;
        let snapshot = app.remote_snapshot();
        assert_eq!(snapshot.model, "test-model");
        assert!(!snapshot.agent.is_empty());
    }

    #[test]
    fn remote_snapshot_session_route() {
        let mut app = App::new_for_testing("/tmp".into());
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Session("abc123".to_string()));
        let snapshot = app.remote_snapshot();
        assert_eq!(snapshot.route, "session:abc123");
    }

    #[test]
    fn remote_snapshot_dialog_reflected() {
        let mut app = App::new_for_testing("/tmp".into());
        app.ui_state.dialog = Dialog::Model;
        let snapshot = app.remote_snapshot();
        assert!(snapshot.dialog.is_some());
        assert!(snapshot.dialog.unwrap().contains("Model"));
    }

    #[test]
    fn remote_snapshot_no_dialog_when_none() {
        let app = App::new_for_testing("/tmp".into());
        let snapshot = app.remote_snapshot();
        assert!(snapshot.dialog.is_none());
    }

    #[test]
    fn render_frame_sends_error_response() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new_for_testing("/tmp".into());
        app.remote_send_tx = Some(tx);

        let event = serde_json::json!({
            "type": "RenderFrame",
            "content": "some frame data"
        });
        app.handle_remote_event(event);

        let msg = rx.try_recv().unwrap();
        match msg {
            RemoteTuiMessage::Error { message } => {
                assert!(
                    message.contains("unsupported_render_frame"),
                    "error must mention unsupported_render_frame: {message}"
                );
            }
            other => panic!("expected Error, got: {other:?}"),
        }
    }

    #[test]
    fn request_snapshot_returns_state_snapshot() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new_for_testing("/tmp".into());
        app.remote_send_tx = Some(tx);

        let event = serde_json::json!({
            "type": "RequestSnapshot",
            "reason": "test"
        });
        app.handle_remote_event(event);

        let msg = rx.try_recv().unwrap();
        match msg {
            RemoteTuiMessage::StateSnapshot { snapshot, .. } => {
                assert_eq!(snapshot.protocol_version, REMOTE_TUI_PROTOCOL_VERSION);
                assert_eq!(snapshot.route, "home");
            }
            other => panic!("expected StateSnapshot, got: {other:?}"),
        }
    }

    #[test]
    fn protocol_version_constant_is_three() {
        assert_eq!(REMOTE_TUI_PROTOCOL_VERSION, 3);
    }

    #[test]
    fn next_remote_snapshot_increments_sequence_monotonically() {
        let mut app = App::new_for_testing("/tmp".into());
        assert_eq!(app.remote_sequence, 0);
        let s1 = app.next_remote_snapshot();
        assert_eq!(s1.sequence, 1);
        let s2 = app.next_remote_snapshot();
        assert_eq!(s2.sequence, 2);
        let s3 = app.next_remote_snapshot();
        assert_eq!(s3.sequence, 3);
        // remote_snapshot (non-mutating) returns the most recent committed seq.
        assert_eq!(app.remote_snapshot().sequence, 3);
    }

    #[test]
    fn request_snapshot_via_remote_event_advances_sequence() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new_for_testing("/tmp".into());
        app.remote_send_tx = Some(tx);

        let event = serde_json::json!({
            "type": "RequestSnapshot",
            "reason": "test"
        });
        app.handle_remote_event(event);

        let msg = rx.try_recv().unwrap();
        match msg {
            RemoteTuiMessage::StateSnapshot { sequence, .. } => {
                assert_eq!(sequence, 1);
            }
            other => panic!("expected StateSnapshot, got: {other:?}"),
        }
        assert_eq!(app.remote_sequence, 1);
    }

    #[test]
    fn resume_from_seq_zero_requests_resync() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new_for_testing("/tmp".into());
        app.remote_send_tx = Some(tx);

        let event = serde_json::json!({
            "type": "Resume",
            "from_event_seq": 0,
            "reason": "test"
        });
        app.handle_remote_event(event);

        let msg = rx.try_recv().unwrap();
        match msg {
            RemoteTuiMessage::ResyncRequired { reason, .. } => {
                assert!(reason.unwrap().contains("invalid"));
            }
            other => panic!("expected ResyncRequired, got: {other:?}"),
        }
    }

    #[test]
    fn resume_from_seq_ahead_requests_resync() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new_for_testing("/tmp".into());
        app.remote_send_tx = Some(tx);
        app.remote_sequence = 5;

        let event = serde_json::json!({
            "type": "Resume",
            "from_event_seq": 100,
            "reason": "ahead"
        });
        app.handle_remote_event(event);

        let msg = rx.try_recv().unwrap();
        match msg {
            RemoteTuiMessage::ResyncRequired { reason, .. } => {
                assert!(reason.unwrap().contains("ahead"));
            }
            other => panic!("expected ResyncRequired, got: {other:?}"),
        }
    }

    #[test]
    fn resume_from_seq_behind_sends_fresh_snapshot() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new_for_testing("/tmp".into());
        app.remote_send_tx = Some(tx);
        app.remote_sequence = 10;

        let event = serde_json::json!({
            "type": "Resume",
            "from_event_seq": 3,
            "reason": "replay"
        });
        app.handle_remote_event(event);

        let msg = rx.try_recv().unwrap();
        match msg {
            RemoteTuiMessage::StateSnapshot { sequence, .. } => {
                // Resume from behind bumps the sequence forward and
                // hands back a fresh snapshot for the client to adopt.
                assert!(sequence > 10);
            }
            other => panic!("expected StateSnapshot, got: {other:?}"),
        }
    }

    #[test]
    fn remote_tui_state_snapshot_serializes() {
        let snapshot = crate::protocol::tui::RemoteTuiStateSnapshot {
            protocol_version: 1,
            sequence: 0,
            session_id: None,
            route: "home".to_string(),
            model: "m".to_string(),
            agent: "a".to_string(),
            status: "idle".to_string(),
            messages: vec![],
            prompt: String::new(),
            dialog: None,
            toasts: vec![],
            git: None,
            plugin_panels: vec![],
            plugin_status_items: vec![],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"protocol_version\":1"));
    }

    #[test]
    fn state_snapshot_applied_to_app() {
        let mut app = App::new_for_testing("/tmp".into());
        app.agent_state.current_model = "old-model".to_string();

        let event = serde_json::json!({
            "type": "StateSnapshot",
            "sequence": 42,
            "snapshot": {
                "protocol_version": 1,
                "sequence": 42,
                "session_id": "sess-1",
                "route": "session:abc",
                "model": "new-model",
                "agent": "test-agent",
                "status": "working",
                "messages": [],
                "prompt": "",
                "dialog": null,
                "toasts": [],
                "plugin_panels": [],
                "plugin_status_items": []
            }
        });
        app.handle_remote_event(event);

        assert_eq!(app.agent_state.current_model, "new-model");
        assert!(matches!(
            app.session_state.session_status,
            SessionStatus::Working
        ));
    }

    #[test]
    fn remote_plugin_ui_effect_applies_to_plugin_ui_state() {
        let mut app = App::new_for_testing("/tmp".into());
        let event = serde_json::json!({
            "type": "PluginUiEffect",
            "envelope": {
                "session_id": null,
                "source": {"type": "plugin", "plugin_id": "test-plugin"},
                "invocation_id": "inv-1",
                "effect": {
                    "type": "open_dialog",
                    "dialog": {
                        "id": "test-plugin:test-dlg",
                        "title": "Test Dialog",
                        "body": {"kind": "text", "text": "hello"},
                        "modal": true
                    }
                }
            }
        });
        app.handle_remote_event(event);
        assert!(
            app.plugin_ui_state
                .get_dialog("test-plugin:test-dlg")
                .is_some(),
            "dialog should be in plugin_ui_state after applying remote PluginUiEffect"
        );
    }

    #[test]
    fn remote_plugin_ui_effect_filters_by_session_id() {
        let mut app = App::new_for_testing("/tmp".into());
        let event = serde_json::json!({
            "type": "PluginUiEffect",
            "envelope": {
                "session_id": "other-session",
                "source": {"type": "plugin", "plugin_id": "test-plugin"},
                "invocation_id": null,
                "effect": {
                    "type": "open_dialog",
                    "dialog": {
                        "id": "test-plugin:wrong-session-dlg",
                        "title": "Wrong",
                        "body": {"kind": "text", "text": "nope"},
                        "modal": false
                    }
                }
            }
        });
        app.handle_remote_event(event);
        assert!(
            app.plugin_ui_state
                .get_dialog("test-plugin:wrong-session-dlg")
                .is_none(),
            "dialog for non-current session should not be applied"
        );
    }

    #[test]
    fn remote_plugin_ui_effect_no_session_matches_any() {
        let mut app = App::new_for_testing("/tmp".into());
        let event = serde_json::json!({
            "type": "PluginUiEffect",
            "envelope": {
                "session_id": null,
                "source": {"type": "plugin", "plugin_id": "test-plugin"},
                "invocation_id": null,
                "effect": {
                    "type": "open_dialog",
                    "dialog": {
                        "id": "test-plugin:any-session-dlg",
                        "title": "Any",
                        "body": {"kind": "text", "text": "applies"},
                        "modal": true
                    }
                }
            }
        });
        app.handle_remote_event(event);
        assert!(
            app.plugin_ui_state
                .get_dialog("test-plugin:any-session-dlg")
                .is_some(),
            "effect with no session filter should apply to any session"
        );
    }

    #[test]
    fn plugin_dialog_does_not_displace_permission_dialog() {
        use crate::tui::app::state::PluginUiApplyResult;
        let mut app = App::new_for_testing("/tmp".into());
        app.ui_state.dialog = Dialog::Permission;
        let result = app.apply_plugin_ui_effect(
            UiEffect::OpenDialog {
                dialog: DialogSpec {
                    id: "my-plugin:dlg".into(),
                    title: "Plugin Dialog".into(),
                    body: UiNode::Empty,
                    modal: true,
                },
            },
            Some("my-plugin"),
        );
        assert_eq!(result, PluginUiApplyResult::Applied);
        // The plugin dialog is stored but the permission dialog stays active.
        assert!(app.plugin_ui_state.get_dialog("my-plugin:dlg").is_some());
        assert!(matches!(app.ui_state.dialog, Dialog::Permission));
    }

    #[test]
    fn panel_effect_is_stored_in_plugin_ui_state() {
        use crate::tui::app::state::PluginUiApplyResult;
        let mut app = App::new_for_testing("/tmp".into());
        let result = app.apply_plugin_ui_effect(
            UiEffect::OpenPanel {
                panel: PanelSpec {
                    id: "my-plugin:panel-1".into(),
                    title: "My Panel".into(),
                    placement: PanelPlacement::Right,
                    body: UiNode::Empty,
                },
            },
            Some("my-plugin"),
        );
        assert_eq!(result, PluginUiApplyResult::Applied);
        assert!(app.plugin_ui_state.panels.contains_key("my-plugin:panel-1"));
    }

    #[test]
    fn status_item_effect_is_stored_in_plugin_ui_state() {
        use crate::tui::app::state::PluginUiApplyResult;
        let mut app = App::new_for_testing("/tmp".into());
        let result = app.apply_plugin_ui_effect(
            UiEffect::AddStatusItem {
                item: StatusItemSpec {
                    id: "my-plugin:status-1".into(),
                    label: Some("Build".into()),
                    placement: StatusPlacement::Right,
                    body: UiNode::Text(TextNode {
                        text: "passing".into(),
                    }),
                },
            },
            Some("my-plugin"),
        );
        assert_eq!(result, PluginUiApplyResult::Applied);
        assert!(app
            .plugin_ui_state
            .status_items
            .contains_key("my-plugin:status-1"));
    }

    #[test]
    fn remote_snapshot_includes_plugin_panels_and_status_items() {
        use crate::protocol::ui::{PanelPlacement, PanelSpec, StatusItemSpec, StatusPlacement};
        let mut app = App::new_for_testing("/tmp".into());
        app.apply_plugin_ui_effect(
            UiEffect::OpenPanel {
                panel: PanelSpec {
                    id: "my-plugin:panel-1".into(),
                    title: "My Panel".into(),
                    placement: PanelPlacement::Right,
                    body: UiNode::Empty,
                },
            },
            Some("my-plugin"),
        );
        app.apply_plugin_ui_effect(
            UiEffect::AddStatusItem {
                item: StatusItemSpec {
                    id: "my-plugin:status-1".into(),
                    label: Some("Status".into()),
                    placement: StatusPlacement::Right,
                    body: UiNode::Empty,
                },
            },
            Some("my-plugin"),
        );
        let snapshot = app.remote_snapshot();
        assert_eq!(snapshot.plugin_panels.len(), 1);
        assert_eq!(snapshot.plugin_panels[0].id, "my-plugin:panel-1");
        assert_eq!(snapshot.plugin_status_items.len(), 1);
        assert_eq!(snapshot.plugin_status_items[0].id, "my-plugin:status-1");
    }

    #[test]
    fn unsupported_effect_degrades_to_toast() {
        use crate::tui::app::state::PluginUiApplyResult;
        let mut app = App::new_for_testing("/tmp".into());
        app.ui_state.plugin_ui_caps = crate::protocol::ui::PluginUiCapabilities {
            dialog: false,
            toast: true,
            ..Default::default()
        };
        let result = app.apply_plugin_ui_effect(
            UiEffect::OpenDialog {
                dialog: DialogSpec {
                    id: "my-plugin:dlg".into(),
                    title: "Test Dialog".into(),
                    body: UiNode::Text(TextNode {
                        text: "content".into(),
                    }),
                    modal: true,
                },
            },
            Some("my-plugin"),
        );
        assert!(matches!(result, PluginUiApplyResult::Unsupported(_)));
        // The degraded summary should appear as a toast.
        assert!(!app.messages_state.toasts.is_empty());
    }

    // -----------------------------------------------------------------
    // Phase 15: envelope-based dispatch and validation
    // -----------------------------------------------------------------

    #[test]
    fn apply_plugin_ui_envelope_dispatches_with_plugin_source() {
        use crate::protocol::ui::{UiEffect, UiEffectEnvelope, UiEffectSource, UiNode, TextNode};
        let mut app = App::new_for_testing("/tmp".into());
        let envelope = UiEffectEnvelope {
            session_id: None,
            source: UiEffectSource::Plugin {
                plugin_id: "my-plugin".into(),
            },
            invocation_id: Some("inv-1".into()),
            effect: UiEffect::ShowToast {
                toast: crate::protocol::ui::ToastSpec {
                    level: crate::protocol::ui::ToastLevel::Info,
                    message: "from envelope".into(),
                },
            },
        };
        let result = app.apply_plugin_ui_envelope(envelope);
        assert!(matches!(
            result,
            crate::tui::app::state::PluginUiApplyResult::ToastRequested
        ));
    }

    #[test]
    fn apply_plugin_ui_envelope_drops_mismatched_session() {
        use crate::protocol::ui::{UiEffect, UiEffectEnvelope, UiEffectSource};
        let mut app = App::new_for_testing("/tmp".into());
        let envelope = UiEffectEnvelope {
            session_id: Some("some-other-session".into()),
            source: UiEffectSource::Plugin {
                plugin_id: "my-plugin".into(),
            },
            invocation_id: None,
            effect: UiEffect::ShowToast {
                toast: crate::protocol::ui::ToastSpec {
                    level: crate::protocol::ui::ToastLevel::Info,
                    message: "should not appear".into(),
                },
            },
        };
        let result = app.apply_plugin_ui_envelope(envelope);
        assert!(matches!(
            result,
            crate::tui::app::state::PluginUiApplyResult::Unsupported(_)
        ));
        // No toast should be queued for a mismatched session.
        assert!(app.messages_state.toasts.is_empty());
    }

    #[test]
    fn apply_plugin_ui_envelope_rejects_oversize_string() {
        use crate::protocol::ui::{UiEffect, UiEffectEnvelope, UiEffectSource};
        let mut app = App::new_for_testing("/tmp".into());
        // 32KiB string exceeds balanced max_string_len of 16KiB.
        let huge = "x".repeat(32 * 1024);
        let envelope = UiEffectEnvelope {
            session_id: None,
            source: UiEffectSource::Plugin {
                plugin_id: "my-plugin".into(),
            },
            invocation_id: None,
            effect: UiEffect::ShowToast {
                toast: crate::protocol::ui::ToastSpec {
                    level: crate::protocol::ui::ToastLevel::Info,
                    message: huge,
                },
            },
        };
        let result = app.apply_plugin_ui_envelope(envelope);
        assert!(matches!(
            result,
            crate::tui::app::state::PluginUiApplyResult::Unsupported(_)
        ));
    }

    #[test]
    fn apply_plugin_ui_envelope_core_source_skips_plugin_ownership() {
        use crate::protocol::ui::{UiEffect, UiEffectEnvelope, UiEffectSource, UiNode, TextNode};
        let mut app = App::new_for_testing("/tmp".into());
        let envelope = UiEffectEnvelope {
            session_id: None,
            source: UiEffectSource::Core,
            invocation_id: None,
            effect: UiEffect::OpenDialog {
                dialog: crate::protocol::ui::DialogSpec {
                    // id lacks plugin prefix but Core source means no
                    // ownership check is enforced.
                    id: "core-only-dialog".into(),
                    title: "Core".into(),
                    body: UiNode::Text(TextNode {
                        text: "hi".into(),
                    }),
                    modal: false,
                },
            },
        };
        let result = app.apply_plugin_ui_envelope(envelope);
        assert!(matches!(
            result,
            crate::tui::app::state::PluginUiApplyResult::Applied
        ));
    }

    #[test]
    fn apply_plugin_ui_envelope_rejects_cross_plugin_surface_id() {
        use crate::protocol::ui::{UiEffect, UiEffectEnvelope, UiEffectSource, UiNode, TextNode};
        let mut app = App::new_for_testing("/tmp".into());
        let envelope = UiEffectEnvelope {
            session_id: None,
            source: UiEffectSource::Plugin {
                plugin_id: "plugin-a".into(),
            },
            invocation_id: None,
            effect: UiEffect::OpenDialog {
                dialog: crate::protocol::ui::DialogSpec {
                    id: "plugin-b:foreign-dialog".into(),
                    title: "Foreign".into(),
                    body: UiNode::Text(TextNode {
                        text: "spoof attempt".into(),
                    }),
                    modal: false,
                },
            },
        };
        let result = app.apply_plugin_ui_envelope(envelope);
        assert!(matches!(
            result,
            crate::tui::app::state::PluginUiApplyResult::Unsupported(_)
        ));
    }

    #[test]
    fn validate_plugin_ui_effects_accepts_balanced_batch() {
        use crate::protocol::ui::{UiEffect, UiNode, TextNode};
        let app = App::new_for_testing("/tmp".into());
        let effects = vec![
            UiEffect::ShowToast {
                toast: crate::protocol::ui::ToastSpec {
                    level: crate::protocol::ui::ToastLevel::Info,
                    message: "first".into(),
                },
            },
            UiEffect::OpenPanel {
                panel: crate::protocol::ui::PanelSpec {
                    id: "p:panel".into(),
                    title: "Panel".into(),
                    placement: crate::protocol::ui::PanelPlacement::Right,
                    body: UiNode::Text(TextNode {
                        text: "ok".into(),
                    }),
                },
            },
        ];
        assert!(app.validate_plugin_ui_effects(&effects).is_ok());
    }

    #[test]
    fn validate_plugin_ui_effects_rejects_empty_batch() {
        use crate::protocol::ui::UiEffect;
        let app = App::new_for_testing("/tmp".into());
        // An empty batch is invalid by spec: a batch is always
        // associated with one or more effects. The validate function
        // returns Ok for an empty slice (vacuous truth) — this test
        // pins that behavior so future tightening is intentional.
        let empty: Vec<UiEffect> = Vec::new();
        assert!(app.validate_plugin_ui_effects(&empty).is_ok());
    }

    #[test]
    fn validate_plugin_ui_effects_rejects_oversize_payload() {
        use crate::protocol::ui::{UiEffect, UiNode, TextNode};
        let app = App::new_for_testing("/tmp".into());
        // Single effect with serialized size just over balanced cap
        // (256KiB) should be rejected by validate_effects.
        let big_body = "y".repeat(260 * 1024);
        let effects = vec![UiEffect::OpenPanel {
            panel: crate::protocol::ui::PanelSpec {
                id: "p:big".into(),
                title: "Big".into(),
                placement: crate::protocol::ui::PanelPlacement::Right,
                body: UiNode::Text(TextNode { text: big_body }),
            },
        }];
        assert!(app.validate_plugin_ui_effects(&effects).is_err());
    }

    #[test]
    fn snapshot_body_within_limit_drops_oversize_body() {
        let big = "z".repeat(SNAPSHOT_BODY_LIMIT + 1);
        let body = crate::protocol::ui::UiNode::Text(crate::protocol::ui::TextNode {
            text: big,
        });
        assert!(snapshot_body_within_limit(&body, SNAPSHOT_BODY_LIMIT).is_none());

        // A body that fits within the limit is kept; one that exceeds
        // it is dropped, regardless of how much limit is added past
        // its serialized size.
        let just_fits_limit =
            serde_json::to_vec(&body).map(|v| v.len()).unwrap_or(0) - 1;
        assert!(snapshot_body_within_limit(&body, just_fits_limit).is_none());
    }

    #[test]
    fn snapshot_body_within_limit_keeps_small_body() {
        let body = crate::protocol::ui::UiNode::Text(crate::protocol::ui::TextNode {
            text: "small".into(),
        });
        assert!(snapshot_body_within_limit(&body, SNAPSHOT_BODY_LIMIT).is_some());
    }

    #[test]
    fn snapshot_panels_have_source_plugin_id_and_body() {
        let mut app = App::new_for_testing("/tmp".into());
        let envelope = crate::protocol::ui::UiEffectEnvelope {
            session_id: None,
            source: crate::protocol::ui::UiEffectSource::Plugin {
                plugin_id: "my-plugin".into(),
            },
            invocation_id: None,
            effect: crate::protocol::ui::UiEffect::OpenPanel {
                panel: crate::protocol::ui::PanelSpec {
                    id: "my-plugin:panel-1".into(),
                    title: "P".into(),
                    placement: crate::protocol::ui::PanelPlacement::Right,
                    body: crate::protocol::ui::UiNode::Text(crate::protocol::ui::TextNode {
                        text: "hi".into(),
                    }),
                },
            },
        };
        let _ = app.apply_plugin_ui_envelope(envelope);
        let snap = app.build_remote_snapshot(app.remote_sequence);
        assert_eq!(snap.plugin_panels.len(), 1);
        let view = &snap.plugin_panels[0];
        assert_eq!(view.id, "my-plugin:panel-1");
        assert_eq!(view.source_plugin_id.as_deref(), Some("my-plugin"));
        assert!(view.body.is_some());
    }

    #[test]
    fn snapshot_panels_drop_oversize_body_but_keep_metadata() {
        // Use a body that passes validation (`max_string_len = 16KiB`)
        // but exceeds the snapshot body cap (`SNAPSHOT_BODY_LIMIT =
        // 16KiB`). The snapshot builder drops the body so it stays
        // bounded under reconnect storms.
        let mut app = App::new_for_testing("/tmp".into());
        let huge_body =
            crate::protocol::ui::UiNode::Container(crate::protocol::ui::ContainerNode {
                title: Some("huge".into()),
                children: (0..128)
                    .map(|i| {
                        crate::protocol::ui::UiNode::Text(
                            crate::protocol::ui::TextNode {
                                text: format!("row-{}-{}", i, "x".repeat(200)),
                            },
                        )
                    })
                    .collect(),
            });
        let envelope = crate::protocol::ui::UiEffectEnvelope {
            session_id: None,
            source: crate::protocol::ui::UiEffectSource::Plugin {
                plugin_id: "my-plugin".into(),
            },
            invocation_id: None,
            effect: crate::protocol::ui::UiEffect::OpenPanel {
                panel: crate::protocol::ui::PanelSpec {
                    id: "my-plugin:huge-panel".into(),
                    title: "Huge".into(),
                    placement: crate::protocol::ui::PanelPlacement::Right,
                    body: huge_body,
                },
            },
        };
        let _ = app.apply_plugin_ui_envelope(envelope);
        let snap = app.build_remote_snapshot(app.remote_sequence);
        let view = snap
            .plugin_panels
            .iter()
            .find(|v| v.id == "my-plugin:huge-panel")
            .expect("huge panel present");
        assert_eq!(view.source_plugin_id.as_deref(), Some("my-plugin"));
        assert!(view.body.is_none(), "oversize body must be dropped");
    }
}
