# TUI Module

The `tui` module provides the terminal user interface using Ratatui.

## Purpose

Renders the interactive terminal UI: message display, prompt input, modal
dialogs, sidebar, status bar, completions, and remote TUI protocol. Routes
user input through `TuiCommand` dispatch to session/memory/research/plugin
backends via `CoreClient`.

## Where It Lives

`src/tui/` — ~15,340 lines in `app/mod.rs` alone.

## How It Works

### Core Integration

The TUI routes session, history, task, memory, and worktree actions through
`CoreClient` so the same logic can run in-process, over stdio, or over a
socket transport.

Local transport selection is handled by `CoreRuntimeMode` (default
`DaemonClient`):

- `DaemonClient` (default) — connects to or auto-starts the user-scoped
  singleton daemon via `connect_or_start_daemon` (`src/core/instance.rs`).
  Uses `SocketCoreClient`.
- `StandaloneInproc` — keeps the core in the same process via
  `InprocCoreClient`. Requires `--standalone`.
- `StandaloneStdio` — spawns `codegg core-stdio` via `StdioCoreClient`.
  Requires `--stdio`.

Legacy `--core-transport inproc|stdio` flags still parse but emit a
deprecation warning.

### Async Command Pattern

High-latency `TuiCommand` handlers use a spawn-and-complete pattern:

1. **Start**: `start_*` function performs immediate UI mutation, clones
   inputs, and spawns a Tokio task. Lifecycle-tracked work uses
   `spawn_registered_tui_task`; `spawn_tui_task` is for fire-and-forget.
2. **Complete**: The spawned task sends a typed completion `TuiCommand`
   back through the command channel.
3. **Apply**: The event loop receives the completion and applies results
   to UI state synchronously.

**Stale protection**: `AsyncUiRequestState` request IDs. Completions with
a stale or cancelled ID are silently ignored.

**Converted handlers**: `ReloadSessions`, `LoadSessionMessages`,
`OpenTreeDialog`, `PreviewImport`, `ConfirmImport`, `ResearchListRuns`,
`ResearchLoadRun`, `ResearchLoadSection`, `MemorySummary`, `MemorySearch`,
`MemoryRemember`, `MemoryForget`, `RunDoctor`, all session mutations,
goal operations, task operations, worktree list, template create,
notification send, plugin commands, project-catalog refresh pair, test
run, asset refresh, provider connection lifecycle, and session selection.

**File diff pipeline**: `FileDiffStatsReady` uses a separate
`spawn_sidebar_diff_stats()` in `src/tui/file_diff.rs`. Bounded by
semaphore (max 2 concurrent), 1 MiB size caps, binary detection, and
stale-generation protection.

See `src/tui/async_cmd.rs` for `spawn_tui_task` and
`spawn_registered_tui_task`.

### AsyncUiRequestState

`AsyncUiRequestState` (`src/tui/app/state/async_request.rs:20`) is a
reusable state machine for async dialog lifecycle:

```rust
pub struct AsyncUiRequestState {
    request_id: u64,        // Monotonically increasing generation counter
    loading: bool,          // Whether a request is currently in flight
    cancelled: bool,        // Whether the current request was cancelled
    last_error: Option<String>, // Last error message from a failed request
}
```

**Methods**: `begin() -> u64`, `cancel()`, `finish(request_id) -> bool`,
`fail(request_id, error) -> bool`, `is_current(request_id) -> bool`,
`clear_loading()`, `is_loading()`, `is_cancelled()`, `request_id()`,
`last_error()`.

**DialogState fields using AsyncUiRequestState**:
`import_request`, `research_request`, `session_reload_request`,
`task_list_request`, `task_delete_request`, `worktree_list_request`,
`template_create_request`, `session_mutation_request`,
`session_messages_request`, `test_run_request`.

**Dialog close integration**: `close_dialog()` cancels async request
states for Import, ResearchBrowser, and Session dialogs.

**Completion semantics**: All async apply handlers follow:

```rust
if !app.dialog_state.<field>.finish(request_id) {
    return;  // stale or cancelled, ignore
}
```

Never mix `is_current()` + manual mutation; always use `finish`/`fail`.

### Background Task Lifecycle

TUI-owned background tasks tracked via `TuiTaskRegistry`
(`src/tui/task_lifecycle.rs:14`) on `App`.

**Key types**:
- `TuiTaskId(u64)` — monotonically increasing task ID
- `TuiTaskKind` — category enum: `Command`, `FileDiff`, `Shell`,
  `Research`, `Memory`, `Notification`, `SecurityReview`, `Indexer`,
  `GitStatus`, `Other`
- `TuiTaskRecord` — stores name, kind, started_at, abort_handle,
  completion flag

**Registry operations**: `spawn(kind, name, future)`, `cancel(id)`,
`cancel_kind(kind)`, `cancel_all()`, `reap_finished()`, `is_finished(id)`,
`active_count()`, `summary()`.

**Outcome accounting**: `completed_count` increments on `reap_finished`
only. `cancelled_count` increments on `cancel_kind`/`cancel`.
`panicked_count` is reserved (stays at 0 with abort-handle design).

**Integration**: `spawn_tui_task()` is fire-and-forget;
`spawn_registered_tui_task(tx, registry, kind, name, fut)` is tracked.

**Reaping**: Event loop calls `app.task_registry.reap_finished()` on
every iteration including idle.

**Shutdown**: `App::prepare_shutdown()` cancels all registered tasks and
kills shell handles.

**Diagnostics**: `/tui-stats` includes task registry stats and shell
handle count.

### Cached Git Sidebar State

Sidebar git metadata computed in background, never on render frame.

**Storage**: `GitSidebarState` (`src/tui/app/state/session.rs:134`)
holds `root`, `branch`, `dirty`, `staged_count`, `unstaged_count`,
`untracked_count`, `conflicted_count`, `ahead`, `behind`,
`operation_state_label`, `available_actions`, `conflicted_paths`,
`last_refreshed`, `loading`, `error`, and `generation: u64`.

**Refresh pipeline**:
1. `start_refresh_git_sidebar(app)` bumps generation via
   `git_sidebar.begin_refresh()`, spawns registered task.
2. Probe runs `egggit::status::repo_status` inside
   `tokio::time::timeout` (3s).
3. Probe posts `TuiCommand::GitSidebarRefreshFinished`.
4. `apply_git_sidebar_refresh` calls `git_sidebar.apply_refresh(...)` or
   `apply_refresh_error(...)`. Both return `false` for stale generations.

**Triggers**: `SelectSession`, `App::set_session`, session reload.

**Remote TUI**: `RemoteTuiStateSnapshot.git: Option<RemoteGitInfo>`
carries cached sidebar state to remote clients.

### Long Output → Info Dialog

`App::show_short_or_info(info_type, lines)` routes output to short toast
(≤3 lines) or scrollable `InfoDialog`. Dialog reused if already open.

### Remote TUI Snapshot Sequencing

`App::remote_sequence: u64` is monotonically increasing.

- `remote_snapshot()` — non-mutating, returns most recent snapshot.
- `next_remote_snapshot()` — mutating, increments and returns new snapshot.
- `build_remote_snapshot(sequence)` — builder parameterised by sequence.

**Resume semantics**: `from_event_seq == 0` → invalid resume;
`> remote_sequence` → client ahead; `<= remote_sequence` → fresh snapshot.

### Remote Plugin UI Effects

Two independent routes: `RemoteTuiMessage::PluginUiEffect` via WebSocket
and `AppEvent::PluginUiEffect` via `GlobalEventBus`. Both apply session
filtering before `apply_plugin_ui_effect()`.

### Synchronous Command Dispatch

All dispatch arms in `src/tui/runtime/command_dispatch.rs` are
`fn` (non-async). No `.await` points in the match. Handlers that need
async work use spawn-and-complete or fire-and-forget patterns.

## Key Types & APIs

### App (`src/tui/app/mod.rs:865`)

```rust
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
    pub run_store: Option<Arc<dyn RunStore>>,
    pub preferences: Option<UserPreferences>,
    pub core_client: Option<Arc<dyn CoreClient>>,
    pub config_watcher: Option<ConfigWatcher>,
    pub theme_registry: Arc<ThemeRegistry>,
    pub subagent_pool: Option<Arc<SubAgentPool>>,
    pub focus_manager: FocusManager,
    pub busy_spinner: SpinnerWidget,
    pub active_goal: Option<GoalSnapshot>,
    pub lsp_tool: Option<Arc<LspTool>>,
    pub security_review_running: Option<SecurityReviewTaskState>,
    pub shell_store: ShellOutputStore,
    pub command_run_store: CommandOutputStore,
    pub shell_handles: HashMap<u64, ShellHandle>,
    pub task_registry: TuiTaskRegistry,
    pub plugin_ui_state: PluginUiState,
    pub plugin_manager: Option<PluginManager>,
    pub remote_sequence: u64,
    // ... viewport, scroll, click, hover, border areas, etc.
}
```

### State Domains (`src/tui/app/state/`)

18 state modules:

| Module | Purpose |
|--------|---------|
| `ui.rs` | Theme, layout, routes, dialog, input mode, keybindings, TTS, diagnostics |
| `session.rs` | Session, token counts, changed files, git sidebar, rate limits |
| `agent.rs` | Agents, models, selection, plan mode, project asset snapshot |
| `dialog.rs` | All dialog instances, async request states, pending operations |
| `messages.rs` | Message history, toasts, spinner |
| `prompt.rs` | Prompt text, completions |
| `async_request.rs` | `AsyncUiRequestState` reusable state machine |
| `diagnostics.rs` | `TuiDiagnostics` runtime counters |
| `plugin_ui.rs` | Plugin dialog/panel/status storage |
| `project_tabs.rs` | Multi-project tab state (Milestone 1) |
| `project_picker.rs` | Project picker state (Milestone 2) |
| `view_switch.rs` | Active-view switch coordinator |
| `routing.rs` | Route routing helpers |
| `snapshot.rs` | Remote snapshot building |
| `persistence.rs` | State persistence |
| `restore.rs` | State restore |
| `manifest.rs` | Manifest handling |
| `projection_client.rs` | Projection client state |

### UiState (`src/tui/app/state/ui.rs:40`)

```rust
pub struct UiState {
    pub theme: Arc<Theme>,
    pub layout: TuiLayout,
    pub sidebar_visible: bool,
    pub auto_scroll: bool,
    pub show_thinking: bool,
    pub show_timestamps: bool,
    pub routes: RouteManager,
    pub dialog: Dialog,
    pub command_mode: bool,
    pub input_mode: InputMode,
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    pub help_lines: Vec<String>,
    pub bindings: HashMap<(KeyModifiers, KeyCode), InputAction>,
    pub keybinds: Option<KeybindConfig>,
    pub vim_mode: bool,
    pub mode: AppMode,           // Embedded | RemoteCore { endpoint }
    pub remote_status: Option<String>,
    pub running: bool,
    pub timeline_visible: bool,
    pub timeline_selected: usize,
    pub render_panic_count: usize,
    pub last_render_error: Option<String>,
    pub tts: Tts,
    pub tts_enabled: bool,
    pub fullscreen: bool,
    pub dirty_regions: Vec<Rect>,
    pub resize_debounce: Option<std::time::Instant>,
    pub tts_via_daemon: bool,
    pub diagnostics: TuiDiagnostics,
    pub plugin_ui_caps: PluginUiCapabilities,
}
```

### SessionState (`src/tui/app/state/session.rs:71`)

```rust
pub struct SessionState {
    pub session: Option<Session>,
    pub session_status: SessionStatus,
    pub token_in: u64,
    pub token_out: u64,
    pub live_output_tokens: u64,
    pub live_output_text: String,
    pub reasoning_tokens: usize,
    pub cached_tokens: u64,
    pub history: VecDeque<HistoryEntry>,
    pub history_pos: Option<usize>,
    pub indexed_files: Arc<RwLock<Vec<String>>>,
    pub project_dir: String,
    pub last_edited_file: Option<String>,
    pub changed_files: Vec<ChangedFile>,
    pub mcp_servers: Vec<(String, String)>,
    pub context_tokens: usize,
    pub context_limit: usize,
    pub compaction_count: usize,
    pub rpm_limit: Option<u64>,
    pub tpm_limit: Option<u64>,
    pub rpm_remaining: Option<u64>,
    pub tpm_remaining: Option<u64>,
    pub permission_pending: bool,
    pub subagent_count: usize,
    pub git_sidebar: GitSidebarState,
}
```

### AgentState (`src/tui/app/state/agent.rs:6`)

```rust
pub struct AgentState {
    pub snapshot: Option<Arc<ProjectAssetSnapshot>>,
    pub agents: Vec<Agent>,        // Legacy cache; prefer snapshot
    pub current_agent: usize,
    pub current_model: String,
    pub models: Vec<String>,
    pub model_idx: usize,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
}
```

### DialogState (`src/tui/app/state/dialog.rs:27`)

Always instantiated: `model_dialog`, `agent_dialog`, `session_dialog`,
`tree_dialog`, `command_palette`.

On-demand (Option): `theme_picker`, `question_dialog`,
`permission_dialog`, `keybind_dialog`, `mcp_dialog`, `share_dialog`,
`import_dialog`, `template_dialog`, `connect_dialog`,
`connection_selection_dialog`, `goto_dialog`, `plan_dialog`,
`diff_dialog`, `review_dialog`, `security_review_dialog`,
`source_preview_dialog`, `run_detail_dialog`, `research_browser`,
`help_dialog`, `info_dialog`, `ui_node_dialog`,
`shell_detail_dialog`, `project_picker`.

Async request states: `import_request`, `research_request`,
`session_reload_request`, `task_list_request`, `task_delete_request`,
`worktree_list_request`, `template_create_request`,
`session_mutation_request`, `session_messages_request`,
`test_run_request`.

Pending fields: `permission_perm_id`, `question_session_id`,
`pending_delete_session`, `pending_archive_session`,
`pending_bulk_delete`, `pending_bulk_delete_ids`,
`pending_bulk_archive`, `pending_bulk_archive_ids`,
`pending_shell_command`, `pending_connection_lifecycle`,
`shell_detail_id`.

Plugin dialogs stored in `PluginUiState`, not `DialogState`.
A single `Dialog::Plugin` variant handles all plugin dialogs.

### Dialog Variants (`src/tui/app/types.rs:2`)

```rust
pub enum Dialog {
    None, Model, Agent, Session, Help, Tree, Theme,
    Question, Permission, Mcp, Keybind,
    Share, Import, Template, Connect, ConnectionSelection,
    Context, Cost, Usage, Stats, Goto, Plan, Diff, Confirm,
    Review, ResearchBrowser, SecurityReview, SourcePreview,
    ShellShow, TaskList, WorktreeList, GoalShow, MemoryResults,
    DoctorReport, Plugin, RunDetail, ProjectPicker,
}
```

### DialogType (`src/tui/components/component.rs:22`)

```rust
pub enum DialogType {
    Share, Model, Agent, Session, Help, Tree, Theme,
    Permission, Mcp, Question, Diff, Import, Template,
    Connect, ConnectionSelection, Keybind,
    Context, Cost, Usage, Stats, Goto, Plan, Review, Confirm,
    ResearchBrowser, SecurityReview, SourcePreview, ShellShow,
    TaskList, WorktreeList, GoalShow, MemoryResults,
    DoctorReport, Plugin, RunDetail, None,
}
```

Note: `Dialog::ProjectPicker` has no corresponding `DialogType` variant;
the picker dialog component returns `DialogType::None`.

### Component Trait (`src/tui/components/component.rs:110`)

```rust
pub trait Component: Send + Any {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> { None }
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool { self.dialog_type().is_modal() }
    fn hit_test(&self, rel_y: usize) -> Option<usize> { None }
    fn set_selected(&mut self, idx: usize) {}
    fn focus_next(&mut self) {}
    fn focus_prev(&mut self) {}
    fn focusable_count(&self) -> usize { 1 }
    fn focused_index(&self) -> usize { 0 }
    fn set_focused(&mut self, idx: usize) {}
}
```

### FocusManager (`src/tui/components/component/focus.rs:14`)

```rust
pub struct FocusManager {
    stack: VecDeque<Box<dyn Component>>,
    focus_index: usize,
}
```

Key methods: `push(component)`, `pop()`, `top()`/`top_mut()`,
`handle_key(key)`, `active_dialog_type()`, `len()`.

### TuiMsg (`src/tui/app/types.rs:86`)

Internal messages from TUI to App. Key variants: `SubmitPrompt`,
`NavigateUp`/`Down`/`Left`/`Right`, `CycleAgent`, `OpenModelDialog`,
`OpenAgentDialog`, `OpenSessionDialog`, `OpenHelpDialog`, `OpenTreeDialog`,
`SelectModel`, `SelectAgent`, `SelectSession(Box<Session>)`,
`OpenDiffDialog`, `OpenShareDialog`, `OpenThemeDialog`,
`ExternalEditor`, `UndoDelete`, `ConfirmResult`, `ReviewOpenDiff`,
`ResearchOpenRun`, `ResearchRefreshRuns`, `ResearchLoadSection`.

### TuiCommand (`src/tui/app/types.rs`)

Async commands sent via channel. ~80 variants covering: session CRUD,
archive, fork, bulk ops, share, export, rename, undo delete, import,
template creation, session message loading, subagent spawn, task/worktree
operations, memory operations, goal lifecycle, research browser, doctor,
security review, git sidebar, plugin commands, test run, shell operations,
provider connection lifecycle, project catalog, file diff stats, and
completion/result variants for each async operation.

### Routes (`src/tui/route.rs`)

```rust
pub enum Route {
    Home,
    Session(String),
}
```

### InputMode (`src/tui/input.rs`)

```rust
pub enum InputMode {
    Insert,
    Normal,
}
```

### InputAction

Key events mapped to: `Send`, `Newline`, `Cancel`, `NavigateUp`/`Down`,
`SwitchAgent`, `SelectModel`, `ClearSession`, `NewSession`,
`FocusPrompt`, `StashPrompt`, `RestorePrompt`, `Char`, `Backspace`,
`Delete`, `CursorLeft`/`Right`/`Home`/`End`, `PageUp`, `PageDown`,
`Search`, `GoToTop`, `GoToBottom`.

## Directory Structure

```
tui/
├── app/
│   ├── mod.rs              # App struct, event loop, key handling (~15,340 lines)
│   ├── types.rs            # Dialog, TuiMsg, TuiCommand, SessionStatus, etc.
│   └── state/              # 18 state domain modules
│       ├── agent.rs        # AgentState (models, agents, snapshot)
│       ├── async_request.rs # AsyncUiRequestState
│       ├── diagnostics.rs  # TuiDiagnostics
│       ├── dialog.rs       # DialogState (all dialog instances)
│       ├── manifest.rs     # Manifest handling
│       ├── messages.rs     # MessagesState (messages, toasts, spinner)
│       ├── persistence.rs  # State persistence
│       ├── plugin_ui.rs    # PluginUiState
│       ├── project_picker.rs # Project picker (Milestone 2)
│       ├── project_tabs.rs # Multi-project tabs (Milestone 1)
│       ├── projection_client.rs # Projection client state
│       ├── prompt.rs       # PromptState (prompt, completions)
│       ├── restore.rs      # State restore
│       ├── routing.rs      # Route routing helpers
│       ├── session.rs      # SessionState (session, history, git info)
│       ├── snapshot.rs     # Remote snapshot building
│       ├── ui.rs           # UiState (theme, layout, routes, keybindings)
│       └── view_switch.rs  # Active-view switch coordinator
├── commands/               # 19 command handler submodules
│   ├── mod.rs              # Re-exports
│   ├── agents.rs           # Asset refresh, agent operations
│   ├── diagnostics.rs      # Doctor, diagnostics, tool contracts
│   ├── git_sidebar.rs      # Git sidebar refresh
│   ├── goals.rs            # Goal lifecycle, session state refresh
│   ├── import.rs           # Import preview, confirm
│   ├── manifest_restore.rs # Manifest restore operations
│   ├── memory.rs           # Memory summary, search, remember, forget
│   ├── plugin_management.rs # Plugin management operations
│   ├── plugins.rs          # Plugin command run, UI effect
│   ├── project_catalog.rs  # Project catalog refresh
│   ├── project_picker.rs   # Project picker navigation
│   ├── provider_connections.rs # Provider connection lifecycle
│   ├── research.rs         # Research list runs, load run, load section
│   ├── security.rs         # Security review dispatch
│   ├── session_selection.rs # Session selection refresh/load
│   ├── sessions.rs         # Session CRUD, archive, fork, bulk ops, rename, share
│   ├── shell.rs            # Shell list, include, rerun, kill, show, ask
│   ├── tasks.rs            # Task/worktree/template/notification/file-diff
│   └── test.rs             # Test run lifecycle
├── runtime/
│   ├── mod.rs              # Re-exports
│   ├── event_loop.rs       # Main event loop (select loop, render, terminal)
│   ├── command_dispatch.rs # Main TuiCommand dispatch match
│   ├── app_events.rs       # Bus event handling (AppEvent subscription)
│   └── render_recovery.rs  # Render panic recovery (progressive fallback)
├── components/
│   ├── component/
│   │   ├── component.rs    # Component trait, DialogType enum
│   │   ├── focus.rs        # FocusManager for modal focus stack
│   │   └── context.rs      # AppContext for overlay dialogs
│   ├── dialogs/            # Modal dialogs (all implement Component)
│   │   ├── agent.rs        # AgentDialog
│   │   ├── command.rs      # CommandPalette
│   │   ├── confirm.rs      # ConfirmDialog
│   │   ├── connect.rs      # ConnectDialog (Eggpool endpoint/API key)
│   │   ├── connection_selection.rs # ConnectionSelectionDialog
│   │   ├── diff.rs         # DiffDialog
│   │   ├── goto.rs         # GotoDialog
│   │   ├── help.rs         # HelpDialog
│   │   ├── import.rs       # ImportDialog
│   │   ├── info.rs         # InfoDialog (Context/Cost/Usage/Stats/etc.)
│   │   ├── keybind.rs      # KeybindDialog
│   │   ├── mcp.rs          # McpDialog
│   │   ├── model.rs        # ModelDialog
│   │   ├── permission.rs   # PermissionDialog
│   │   ├── plan.rs         # PlanDialog
│   │   ├── plugin.rs       # PluginDialog (generic plugin UI)
│   │   ├── project_picker.rs # ProjectPickerDialog
│   │   ├── question.rs     # QuestionDialog
│   │   ├── research.rs     # ResearchBrowserDialog
│   │   ├── review.rs       # ReviewDialog
│   │   ├── run_detail.rs   # RunDetailDialog
│   │   ├── security_review.rs # SecurityReviewDialog
│   │   ├── session.rs      # SessionDialog
│   │   ├── share.rs        # ShareDialog
│   │   ├── source_preview.rs # SourcePreviewDialog
│   │   ├── template.rs     # TemplateDialog
│   │   ├── theme.rs        # ThemePickerDialog
│   │   ├── tree.rs         # TreeDialog
│   │   └── ui_node.rs      # UiNodeDialog (generic, reuses Plugin slot)
│   ├── completion_overlay.rs # Slash/file/agent completion popups
│   ├── diff.rs             # DiffViewer
│   ├── help_overlay.rs     # Dead code (help is mode-aware via input.rs)
│   ├── image.rs            # ImageViewer (image rendering via ANSI)
│   ├── messages.rs         # MessagesWidget (message display, streaming)
│   ├── notification.rs     # NotificationManager
│   ├── plugin_renderer.rs  # Compat alias for UiNodeRenderer
│   ├── ui_node_renderer.rs # UiNodeRenderer (UiNode → ratatui/line)
│   ├── prompt.rs           # PromptWidget
│   ├── scroll.rs           # CenteredScroll
│   ├── sidebar.rs          # SidebarWidget
│   ├── spinner.rs          # SpinnerWidget
│   ├── status_bar.rs       # StatusBarWidget
│   ├── toast.rs            # ToastManager
│   └── tool_output.rs      # ToolOutput
├── input.rs                # Key event handling, keybindings, InputMode
├── layout.rs               # Layout calculations, TuiLayout
├── route.rs                # Route/RouteManager (Home, Session)
├── theme.rs                # TUI-local Theme (ratatui projection)
├── terminal.rs             # TerminalGuard lifecycle, AppTerminal
├── file_diff.rs            # Async diff stats for sidebar
├── task_lifecycle.rs       # TuiTaskRegistry for background task tracking
├── async_cmd.rs            # spawn_tui_task, spawn_registered_tui_task
├── command.rs              # Slash command registry (108 built-in)
├── ui_builders/            # Pure UiNode builder functions
│   ├── mod.rs
│   ├── stats.rs            # stats_node for /tui-stats
│   ├── plugins.rs          # Plugin management builders
│   └── shell.rs            # shell_detail_node for /shell-show
└── mod.rs                  # TUI entry point, module declarations, re-exports
```

### UiNode Builders (`ui_builders/`)

Pure functions converting domain data into `UiNode` trees. Free of `App`
and ratatui dependencies for testability.

| Module | Responsibility |
|--------|----------------|
| `stats.rs` | `stats_node(diagnostics, task_summary, shell_handles_count) -> UiNode` |
| `plugins.rs` | Re-exports plugin management builders from `crate::plugin::management_ui` |
| `shell.rs` | `shell_detail_node(entry) -> UiNode` for `/shell-show` |

### Shared `UiNode` Surface

The portable `UiNode` schema (`codegg_protocol::ui`) is used for both
plugin UI and selected first-party surfaces:

```
Domain data -> UiNode builder -> UiNodeRenderer -> ratatui / line output
```

- **Builders** live in `src/tui/ui_builders/`.
- **Renderer** (`UiNodeRenderer` in `src/tui/components/ui_node_renderer.rs`)
  is the single lowering adapter.
- **Generic dialog** (`UiNodeDialog` in `src/tui/components/dialogs/ui_node.rs`)
  accepts `UiNode`, supports scroll/page/jump, reuses `DialogType::Plugin`
  slot in FocusManager.

Use `UiNode` for: read-only informational surfaces (tables, key-value
lists, text/code dumps, scrollable summaries, plugin dialogs).

Do NOT use `UiNode` for: interactive components needing focus management
(permission, question, command palette, diff, tree, shell interactive).

## Configuration Surface

No direct TUI config keys in `opencode.jsonc`. TUI state is driven by:
- Theme selection via `[theme]` config
- Keybindings via `[keybindings]` config
- Vim mode via config
- Agent/model selection stored in session

## Invariants & Gotchas

- **Dialog::Info doesn't exist**: `Dialog::Info` is NOT in the Dialog
  enum. `components/dialogs/info.rs` exists but `InfoDialog` uses
  `DialogType::Context`, `Cost`, `Usage`, `Stats`, `ShellShow`, etc.
  via `dialog_type_for_info_type()`.
- **DialogType in component.rs**: `DialogType` lives in
  `src/tui/components/component/component.rs`, not `types.rs`.
- **Dialog::Plugin is generic**: A single `Dialog::Plugin` variant
  handles all plugin dialogs. `UiNodeDialog` also reuses this slot.
- **Dispatch arms are all non-async**: `command_dispatch.rs` has no
  `.await` points.
- **Git sidebar is cached, not live**: Render reads from
  `session_state.git_sidebar`; never shells out to git.
- **Remote TUI is event/state-driven**: `RenderFrame` is unsupported.
- **State domains are 18 modules**: Not the 6 listed in the doc header;
  the domain model expanded across multiple milestones.
- **Async command stale-completion tests**: Each guarded handler has a
  stale-completion test in `src/tui/mod.rs::async_cmd_tests`.

## Testing

```bash
cargo test --test tui_render      # 97 render regression tests
cargo test --test tui              # integration tests
```

### Render Regression Tests (`tests/tui_render.rs`)

Uses `ratatui::backend::TestBackend` across terminal sizes:
tiny (40×12), small (60×20), normal (100×32), wide (160×40),
tall (100×60).

Coverage: empty/home state, active session with messages, streaming,
tool calls, sidebar file changes, all 30+ dialog variants, completion
overlay, toasts, pathological content, component panic injection,
combined states.

Key patterns: `render_app_to_buffer(app, w, h)`,
`assert_render_ok(app, w, h)`, `text_in_buffer(buffer)`,
`buffer_contains(buffer, needle)`.

## Related Docs

- [agent.md](agent.md) — AgentLoop that processes TUI commands
- [bus.md](bus.md) — GlobalEventBus and event types
- [session.md](session.md) — Session storage
