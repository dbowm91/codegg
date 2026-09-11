//! TUI effect requests and asynchronous completions.
//!
//! TuiCommand is the app/runtime boundary: component and user input enters
//! through TuiMsg, while this enum carries effect requests to the runtime and
//! typed completions back to the synchronous App state owner.

use super::types::{ConnectionLifecycleAction, TodoEntry};
use crate::config::schema::SessionTemplate;
use crate::protocol::core::CoreResponse;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

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
    /// Refresh all project runtime assets through the daemon-owned
    /// coordinator. Focused `/reload` aliases use this same variant.
    RefreshAssets,
    AssetRefreshFinished {
        report: Option<crate::protocol::core::AssetRefreshReportDto>,
        error: Option<String>,
    },
    /// Completion of a daemon-owned controlled LSP preview application.
    LspPreviewApplyFinished {
        session_id: String,
        result: Option<crate::protocol::lsp::LspPreviewApplyResultDto>,
        error: Option<String>,
    },
    ReloadSessions,
    OpenTreeDialog,
    /// Completion of an async project catalog refresh
    /// (`Multi-Project TUI milestone 1`). Carries the request id so
    /// stale completions (after a new refresh has begun) are dropped
    /// at apply time.
    ProjectCatalogRefreshed {
        request_id: u64,
        supported: bool,
        entries: Vec<crate::protocol::dto::ProjectSummaryDto>,
        truncated: bool,
        error: Option<String>,
    },
    /// Explicit user request to refresh the project catalog.
    RefreshProjectCatalog,
    PreviewImport {
        source: crate::tui::components::dialogs::import::ImportSource,
    },
    ConfirmImport {
        source: crate::tui::components::dialogs::import::ImportSource,
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
    HabitList {
        ready_only: bool,
    },
    HabitDismiss {
        id: String,
    },
    SkillPublish {
        id: String,
        target_scope: crate::skills::promotion::SkillTargetScope,
    },
    SkillPublishFinished {
        message: String,
        is_error: bool,
    },
    HabitResult {
        toast_message: String,
        is_error: bool,
    },
    CompactSession,
    OpenDiffDialog {
        old_content: Box<str>,
        new_content: Box<str>,
        title: Box<str>,
    },
    SendNotification {
        notification_type: crate::tui::components::notification::NotificationType,
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
    OpenRunDetailLoaded {
        dialog: crate::tui::components::dialogs::run_detail::RunDetailDialog,
    },
    OpenRunDetailError {
        error: String,
    },
    /// Completion for a daemon-owned historical run rerun request.
    RunRerunFinished {
        parent_run_id: String,
        child_job_id: Option<String>,
        error: Option<String>,
    },
    /// Completion: sessions have been reloaded from core.
    SessionsReloaded {
        request_id: u64,
        sessions: Vec<crate::protocol::dto::Session>,
        message_counts: std::collections::HashMap<String, usize>,
        error: Option<String>,
    },
    /// Completion for daemon-owned Eggpool provisioning. The result is
    /// secret-free; the API key is never carried in a TUI command.
    EggpoolConnectionFinished {
        operation_id: String,
        result: Result<crate::protocol::provider::CreateEggpoolConnectionResult, String>,
    },
    ConnectionRotationFinished {
        operation_id: String,
        result: Result<crate::protocol::provider::ConnectionRotateStatusDto, String>,
    },
    /// Completion: session messages have been loaded from core.
    SessionMessagesLoaded {
        request_id: u64,
        session_id: String,
        messages: Vec<crate::session::message::Message>,
        error: Option<String>,
    },
    /// Provider Connections Milestone 3: trigger the connection
    /// selection dialog refresh flow. Emitted when the dialog opens or
    /// when the user requests an explicit reload.
    SessionSelectionRefresh,
    /// Provider Connections Milestone 3: trigger an explicit
    /// selection-list load for a specific session ID. Handlers in the
    /// command runner spawn the actual `CoreRequest` calls.
    SessionSelectionLoad {
        session_id: String,
    },
    ConnectionLifecycle {
        action: ConnectionLifecycleAction,
        connection_id: String,
        expected_revision: u64,
    },
    /// Provider Connections Milestone 3: completion for the selection
    /// refresh flow. Carries the resolved selection, the redacted
    /// connection list, and (when known) the model catalog for the
    /// currently focused connection.
    SessionSelectionLoaded {
        session_id: String,
        selection: Option<crate::protocol::provider::SessionSelectionDto>,
        connections: Vec<crate::protocol::provider::ProviderConnectionSummaryDto>,
        models: Vec<crate::protocol::provider::SelectedModelDto>,
        focused_connection_id: Option<String>,
        error: Option<String>,
    },
    ConnectionLifecycleFinished {
        action: ConnectionLifecycleAction,
        connection_id: String,
        message: Option<String>,
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
    /// Show tool contracts and broker status. Synchronous, no
    /// background work needed.
    ToolContracts,
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
        staged_count: usize,
        unstaged_count: usize,
        untracked_count: usize,
        conflicted_count: usize,
        ahead: Option<i32>,
        behind: Option<i32>,
        error: Option<String>,
        /// Phase F: active operation family (merge/rebase/cherry-pick/revert/
        /// bisect/apply-mailbox/sequencer/unknown/none).
        operation_state_label: Option<String>,
        /// Phase F: legal recovery actions for the active state.
        available_actions: Vec<String>,
        /// Phase F: conflicted paths from the typed conflict model.
        conflicted_paths: Vec<String>,
    },
    RunHumanShell {
        command: String,
        promote_after: bool,
        cwd: PathBuf,
    },
    /// Run supervised tests from the /test slash command.
    TestRun {
        scope: String,
        args: String,
    },
    /// Completion: a supervised test run has finished.
    TestRunFinished {
        request_id: u64,
        report: Option<Box<crate::test_runner::TestReport>>,
        summary: Option<String>,
        error: Option<String>,
    },
    ShellEvent(crate::shell::ShellEvent),
    RegisterShellHandle {
        id: u64,
        handle: crate::shell::runtime::ShellHandle,
    },
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
    ShellExpand {
        id: u64,
        stream: String,
        range: Option<String>,
    },
    /// Render one interactive terminal view (M003). Synchronous: projects
    /// the controller state for `handle` into the terminal dialog.
    TerminalShow {
        handle: String,
    },
    /// Completion: an interactive terminal create has finished.
    TerminalCreateFinished {
        request_id: u64,
        handle: Option<String>,
        workspace_id: Option<String>,
        command_label: Option<String>,
        error: Option<String>,
    },
    /// Completion: an interactive terminal list has finished.
    TerminalListFinished {
        request_id: u64,
        processes: Option<Vec<crate::protocol::interactive_process::InteractiveProcessMetadata>>,
        error: Option<String>,
    },
    /// Completion: an interactive terminal attach has finished. Carries
    /// the caller-owned attachment plus the initial bounded chunk and/or
    /// a typed resync.
    TerminalAttachFinished {
        request_id: u64,
        handle: String,
        attachment_id: Option<String>,
        chunk: Option<crate::protocol::interactive_process::InteractiveOutputChunk>,
        resync: Option<crate::protocol::interactive_process::InteractiveResync>,
        error: Option<String>,
    },
    /// Completion: an interactive terminal resume has finished.
    TerminalResumeFinished {
        request_id: u64,
        handle: String,
        chunk: Option<crate::protocol::interactive_process::InteractiveOutputChunk>,
        resync: Option<crate::protocol::interactive_process::InteractiveResync>,
        error: Option<String>,
    },
    /// Completion: an interactive terminal input/resize/detach/terminate/
    /// remove operation has finished. `cols`/`rows` carry the daemon
    /// echo for resize; `exit_code`/`exit_signal` carry the outcome for
    /// terminate.
    TerminalOpFinished {
        request_id: u64,
        op: String,
        handle: String,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
        cols: Option<u16>,
        rows: Option<u16>,
        error: Option<String>,
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
    /// Completion of the nonblocking session creation required by a prompt
    /// submitted before a session existed.  The route and prompt are carried
    /// in the completion so apply never consults mutable current prompt state.
    PromptSessionCreated {
        request_id: u64,
        route: crate::tui::app::state::UiRouteToken,
        prompt: String,
        session: Option<crate::protocol::dto::Session>,
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
        workspace_root: PathBuf,
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
        removed_files: bool,
        install_path: Option<String>,
        warning: Option<String>,
        error: Option<String>,
    },
    /// Completion: a plugin install operation has finished.
    PluginInstallFinished {
        source: String,
        lines: Vec<String>,
        error: Option<String>,
    },
    /// Project Picker (Milestone 2): load project detail via ProjectGet.
    StartGetProject {
        request_id: u64,
        project_id: String,
        picker_generation: u64,
        picker_request_id: u64,
    },
    /// Project Picker (Milestone 2): ProjectGet completion.
    ProjectGetLoaded {
        request_id: u64,
        target_project_id: String,
        picker_generation: u64,
        picker_request_id: u64,
        result: Option<crate::protocol::dto::ProjectDetailsDto>,
        error: Option<String>,
    },
    /// Project Picker (Milestone 2): load sessions for a project tab.
    StartListProjectSessions {
        tab_id: crate::tui::app::state::ProjectTabId,
        project_id: String,
        workspace_id: String,
    },
    /// Project Picker (Milestone 2): session list completion.
    ProjectSessionsLoaded {
        request_id: u64,
        tab_id: crate::tui::app::state::ProjectTabId,
        project_id: String,
        workspace_id: String,
        sessions: Vec<crate::protocol::dto::Session>,
        error: Option<String>,
    },
    /// Project Picker (Milestone 2): register a workspace by path.
    StartRegisterWorkspace {
        path: String,
    },
    /// Project Picker (Milestone 2): workspace registration completion.
    WorkspaceRegistered {
        request_id: u64,
        picker_request_id: u64,
        workspace_id: Option<String>,
        error: Option<String>,
    },
    /// Project Picker (Milestone 2): register a project.
    StartRegisterProject {
        workspace_id: String,
        display_name: String,
        description: Option<String>,
        tags: Vec<String>,
    },
    /// Project Picker (Milestone 2): project registration completion.
    ProjectRegistered {
        request_id: u64,
        picker_request_id: u64,
        project_id: Option<String>,
        error: Option<String>,
    },
    /// Switch to the next project tab.
    NextProjectTab,
    /// Switch to the previous project tab.
    PreviousProjectTab,
    /// Close the active project tab.
    CloseProjectTab,
    /// Presence M002: explicit refresh of one project's collaborator
    /// presence. The handler performs capability negotiation + snapshot
    /// fetch through `CoreClient`.
    RefreshPresence {
        project_id: String,
    },
    /// Presence M002: completion of a snapshot fetch. Stale completions
    /// (wrong request id or reconnect epoch) are dropped at apply time.
    /// `unauthorized` covers `project_not_found` denials (indistinguishable
    /// from absent); `unsupported` covers older daemons without the
    /// presence capability. Both render the identical unavailable panel.
    PresenceSnapshotLoaded {
        request_id: u64,
        project_id: String,
        snapshot: Option<crate::protocol::core::PresenceSnapshotDto>,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Presence M002: liveness hint (`CoreEvent::PresenceUpdated`).
    /// Carries no collaborator detail; flags the project for a bounded
    /// re-fetch when it is not already loading.
    PresenceHint {
        project_id: String,
    },
    /// Milestone 4: trigger a manifest restore attempt.
    /// Dispatched at TUI startup after the manifest is loaded.
    ManifestRestoreRequested,
    /// Milestone 4: completion of a per-project ProjectGet fetch
    /// during manifest restore. Carries the request_id, the project
    /// id that was queried, and the result. Stale completions are
    /// dropped at apply time.
    ManifestRestoreProjectGetLoaded {
        request_id: u64,
        project_id: String,
        result: Option<crate::protocol::dto::ProjectDetailsDto>,
        error: Option<String>,
    },
    /// Milestone 4: completion of the manifest restore pipeline.
    /// Carries the restored plan as a list of (project_id,
    /// workspace_id, session_id) triples so the TUI can transition
    /// into the restored state without a separate async step.
    ManifestRestoreFinished {
        plan: crate::tui::app::state::restore::RestorePlanWire,
        pending_heavy_load: Option<String>,
        diagnostics: Vec<crate::tui::app::state::restore::RestoreDiagnosticWire>,
        daemon_capability_supported: bool,
    },
    /// Milestone 4: operator requested disabling manifest
    /// persistence.
    ManifestPersistenceDisable,
    /// Milestone 4: operator requested enabling manifest
    /// persistence.
    ManifestPersistenceEnable,
    /// Milestone 4: operator requested resetting manifest
    /// persistence.
    ManifestPersistenceReset,
    // M012 checked undo/reapply
    EditUndoLatest {
        session_id: String,
        workspace_id: String,
    },
    EditReapplyLatest {
        session_id: String,
        workspace_id: String,
    },
    EditUndo {
        checkpoint_id: String,
        session_id: String,
        workspace_id: String,
    },
    EditReapply {
        checkpoint_id: String,
        session_id: String,
        workspace_id: String,
    },
    EditCheckpointList {
        session_id: String,
        workspace_id: String,
    },
    EditUndoFinished {
        session_id: String,
        result: Option<crate::protocol::dto::EditRestoreResultDto>,
        error: Option<String>,
    },
    EditReapplyFinished {
        session_id: String,
        result: Option<crate::protocol::dto::EditRestoreResultDto>,
        error: Option<String>,
    },
    EditCheckpointListFinished {
        session_id: String,
        checkpoints: Vec<crate::protocol::dto::EditCheckpointSummaryDto>,
        error: Option<String>,
    },
    /// Presence M003: start an authorized read-only observation of one
    /// session. The handler performs capability negotiation + session
    /// subscribe through `CoreClient` on a registered task.
    StartObserve {
        project_id: String,
        session_id: String,
    },
    /// Presence M003: subscribe completion. Stale completions (wrong
    /// request id or reconnect epoch) are dropped at apply time.
    /// `unauthorized` covers `project_not_found` denials
    /// (indistinguishable from absent); `unsupported` covers older
    /// daemons without projection support. Both render identically.
    ObserveSubscribed {
        request_id: u64,
        project_id: String,
        session_id: String,
        subscription_id: Option<crate::protocol::projection::replay::ProjectionSubscriptionId>,
        cursor: Option<crate::protocol::projection::replay::ProjectionCursor>,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Presence M003: resume/replay continuation for the active
    /// observation (reconnect/lag/resync path).
    ObserveResumed {
        request_id: u64,
        session_id: String,
        cursor: Option<crate::protocol::projection::replay::ProjectionCursor>,
        last_delivered_seq: u64,
        error: Option<String>,
        unauthorized: bool,
        reconnect_epoch: u64,
    },
    /// Presence M003: explicit stop of the active observation. Carries
    /// the observer-owned subscription id (when present) so the handler
    /// can issue the authoritative unsubscribe best-effort.
    StopObserving,
    /// Presence M003: unsubscribe completion (best-effort; stale
    /// completions are ignored).
    ObserveUnsubscribed {
        subscription_id: Option<crate::protocol::projection::replay::ProjectionSubscriptionId>,
    },
    /// Project Collaboration M002: explicit refresh of one project's
    /// chat window (channel ensure + history through `CoreClient`).
    RefreshChat {
        project_id: String,
    },
    /// Project Collaboration M002: history-page completion for the
    /// active channel window.
    ChatHistoryLoaded {
        request_id: u64,
        project_id: String,
        channel_id: String,
        messages: Vec<crate::protocol::core::ChatMessageDto>,
        next_cursor: u64,
        truncated: bool,
        retention_floor_seq: u64,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M002: incremental-sync completion from the
    /// cached M001 cursor (or a bounded resync page).
    ChatSyncLoaded {
        request_id: u64,
        project_id: String,
        channel_id: String,
        messages: Vec<crate::protocol::core::ChatMessageDto>,
        next_cursor: u64,
        resync_required: bool,
        retention_floor_seq: u64,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M002: send completion. `draft` echoes the
    /// attempted body so failures retain it; `duplicate` marks idempotent
    /// retry convergence. No task-per-message spinner: stale protection
    /// is the reconnect epoch plus merge-by-id.
    ChatMessageSent {
        request_id: u64,
        project_id: String,
        channel_id: String,
        message: Option<crate::protocol::core::ChatMessageDto>,
        duplicate: bool,
        draft: String,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M002: edit completion (new revision).
    ChatEditFinished {
        request_id: u64,
        project_id: String,
        channel_id: String,
        message: Option<crate::protocol::core::ChatMessageDto>,
        message_id: String,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M002: redact completion (identity +
    /// revision; the body is `[REDACTED]` daemon-side).
    ChatRedactFinished {
        request_id: u64,
        project_id: String,
        channel_id: String,
        message_id: String,
        revision: u64,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M002: read-marker completion
    /// (forward-only; failures keep the local unread badge).
    ChatReadMarkerSet {
        project_id: String,
        channel_id: String,
        last_read_seq: u64,
        error: Option<String>,
    },
    /// Project Collaboration M002: composing-snapshot completion
    /// (content-free leases).
    ChatComposingLoaded {
        request_id: u64,
        project_id: String,
        channel_id: String,
        composing: Vec<crate::protocol::core::ChatComposingDto>,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M002: liveness hint (`ChatMessageCommitted`
    /// / `ChatComposingUpdated`). Carries no message detail; flags the
    /// project for a bounded re-fetch when it is not already loading.
    ChatHint {
        project_id: String,
        channel_id: String,
    },
    /// Project Collaboration M003: structured-action submit completion.
    /// Explicit typed operation only; free text never produces this.
    /// `duplicate` marks idempotent retry convergence (no second job).
    ChatActionSubmitted {
        request_id: u64,
        project_id: String,
        channel_id: String,
        action: Option<crate::protocol::core::ChatActionDto>,
        duplicate: bool,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M003: bounded action-list completion for
    /// the active channel (optionally filtered to one message).
    ChatActionListLoaded {
        request_id: u64,
        project_id: String,
        channel_id: String,
        actions: Vec<crate::protocol::core::ChatActionDto>,
        error: Option<String>,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    },
    /// Project Collaboration M003: action liveness hint
    /// (`ChatActionUpdated`). Flags the project for a bounded action
    /// re-fetch; receivers re-fetch through the authorized list path.
    ChatActionHint {
        project_id: String,
        channel_id: String,
        message_id: String,
    },
}

/// Send a command on the bounded TUI effect channel.
pub(crate) fn send_tui(tx: &mpsc::Sender<TuiCommand>, cmd: TuiCommand) -> bool {
    match tx.try_send(cmd) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!("TUI command channel full; dropping command");
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::warn!("TUI command channel closed; dropping command");
            false
        }
    }
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
