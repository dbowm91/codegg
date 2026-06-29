# TUI Module

The `tui` module provides the terminal user interface using Ratatui.

## Overview

**Location**: `src/tui/`

**Key Responsibilities**:
- Terminal UI rendering with Ratatui
- Keyboard input handling via configurable keybindings
- Application state management across 6 state domains
- Layout and component rendering
- Notifications, dialogs, and FocusManager-based modal handling
- Core-backed session/history operations via `CoreClient`

## Core Integration

The TUI no longer talks directly to session storage for most migrated flows. Instead, it routes session, history, task, memory, and worktree actions through `CoreClient` so the same logic can run in-process, over stdio, or over a socket transport.

Local transport selection is handled by `--core-transport` or `CODEGG_CORE_TRANSPORT`:
- `inproc` keeps the core in the same process
- `stdio` spawns `codegg core-stdio`
- `socket` connects to a Unix socket endpoint supplied by `--core-endpoint` or `CODEGG_CORE_ENDPOINT`

## Async Command Pattern

High-latency `TuiCommand` handlers are converted to a spawn-and-complete pattern to keep the event loop responsive. The pattern:

1. **Start**: `start_*` function performs immediate UI mutation (sets loading state, adds toast), clones needed inputs, and spawns a Tokio task via `spawn_tui_task`.
2. **Complete**: The spawned task sends a typed completion `TuiCommand` (e.g., `SessionsReloaded`, `SessionMessagesLoaded`) back through the command channel.
3. **Apply**: The event loop receives the completion and applies results to UI state synchronously.

This ensures keyboard input, resize handling, streaming redraws, spinner animation, and toast expiry continue even while core requests are slow.

**Stale protection**: Operations that can be repeated rapidly (import preview, research loading) use a `request_id` / generation counter. Completions with a mismatched id are silently ignored.

**Converted handlers**: `ReloadSessions`, `LoadSessionMessages`, `OpenTreeDialog`, `PreviewImport`, `ConfirmImport`, `ResearchListRuns`, `ResearchLoadRun`, `ResearchLoadSection`, `MemorySummary`, `MemorySearch`, `MemoryRemember`, `MemoryForget`, `RunDoctor`, all session mutations (delete, archive, fork, bulk delete/archive/export, rename, undo delete, share, unshare, export), goal operations (show, checkpoint, budget, refresh session state), task operations (list, delete, schedule), worktree list, template create, and notification send.

**File diff pipeline** (related but distinct): `FileDiffStatsReady` uses a separate spawn-and-complete pattern via `spawn_sidebar_diff_stats()` in `src/tui/file_diff.rs`. It does not go through `spawn_tui_task`. The background worker is bounded by a semaphore (max 2 concurrent tasks), enforces 1 MiB size caps, binary detection, and stale-generation protection.

**Not converted** (remain synchronous in command dispatch): shell commands, security review, and other already-fast or already-spawned handlers.

See `src/tui/async_cmd.rs` for the `spawn_tui_task` helper.

### AsyncUiRequestState (Phase 10)

`AsyncUiRequestState` (`src/tui/app/state/async_request.rs`) is a small reusable state machine that replaces ad-hoc generation counters and boolean in-flight flags for dialog async lifecycle.

**Type:**
```rust
pub struct AsyncUiRequestState {
    request_id: u64,        // Monotonically increasing generation counter
    loading: bool,          // Whether a request is currently in flight
    cancelled: bool,        // Whether the current request was cancelled
    last_error: Option<String>, // Last error message from a failed request
}
```

**Methods:**
- `begin() -> u64` — increments request ID, sets loading, clears cancelled and previous error. Returns the new ID.
- `cancel()` — sets cancelled, clears loading, increments request ID to invalidate in-flight work.
- `finish(request_id) -> bool` — returns true (and clears loading) only if the ID is current and not cancelled. Stale completions return false.
- `fail(request_id, error) -> bool` — same guard as `finish`, but stores the error message on success.
- `is_current(request_id) -> bool` — checks if a request ID matches the current generation.
- `clear_loading()` — clears loading without modifying the request ID (useful for dialog close cleanup).

**DialogState fields using AsyncUiRequestState:**
- `import_request` — import preview/confirm
- `research_request` — research browser list/run/section loads
- `session_reload_request` — session list reload
- `task_list_request` — task listing
- `task_delete_request` — task deletion
- `worktree_list_request` — worktree listing
- `template_create_request` — template creation
- `session_mutation_request` — session delete/archive/fork/rename/share
- `session_messages_request` — session message loading

**Dialog close integration:** `close_dialog()` (`pub(crate)` for testability) cancels async request states for Import, ResearchBrowser, and Session dialogs, ensuring stale completions are ignored after dismissal.

### Background Task Lifecycle (Phase 7)

TUI-owned background tasks are tracked via [`TuiTaskRegistry`](src/tui/task_lifecycle.rs) on `App`.

**Key types:**
- `TuiTaskId(u64)` -- monotonically increasing task identifier
- `TuiTaskKind` -- category enum: `Command`, `FileDiff`, `Shell`, `Research`, `Memory`, `Notification`, `SecurityReview`, `Indexer`, `Other`
- `TuiTaskRecord` -- stores name, kind, started_at, abort_handle

**Registry operations:**
- `spawn(kind, name, future)` -- register and spawn a tracked task, returns `TuiTaskId`
- `cancel(id)` -- abort a specific task
- `cancel_kind(kind)` -- abort all tasks of a given kind
- `cancel_all()` -- abort all registered tasks
- `reap_finished()` -- remove completed tasks from the registry
- `active_count()` / `summary()` -- diagnostics

**Integration with spawn_tui_task:**
- `spawn_tui_task()` -- unchanged, fire-and-forget (no tracking)
- `spawn_registered_tui_task(tx, registry, kind, name, fut)` -- tracked variant, returns `Option<TuiTaskId>`

**Shutdown:** `App::prepare_shutdown()` cancels all registered tasks and kills shell handles. Called before `terminal_guard.restore()` in `run_event_loop`.

**Diagnostics:** `/tui-stats` now includes task registry stats (active counts by kind, oldest task, cancelled count) and shell handle count.

## Directory Structure

```
tui/
├── app/                    # Main application state
│   ├── mod.rs              # App struct, event loop, key handling
│   ├── types.rs            # Dialog, TuiMsg, TuiCommand, SessionStatus, etc.
│   └── state/              # State domains
│       ├── agent.rs        # AgentState (models, agents, selection)
│       ├── async_request.rs # AsyncUiRequestState (shared async dialog lifecycle)
│       ├── diagnostics.rs  # TuiDiagnostics (runtime counters)
│       ├── dialog.rs       # DialogState (dialog instances, dialog visibility)
│       ├── messages.rs     # MessagesState (message history, toasts, spinner)
│       ├── prompt.rs       # PromptState (prompt, completions)
│       ├── session.rs      # SessionState (session, history, git info)
│       └── ui.rs           # UiState (theme, layout, routes, keybindings)
├── commands/               # TUI command handlers (extracted from mod.rs)
│   ├── mod.rs              # Re-exports
│   ├── sessions.rs         # Session CRUD, archive, fork, bulk ops, rename, share, import
│   ├── tasks.rs            # Task list, delete, schedule
│   ├── goals.rs            # Goal set, pause, resume, clear, done, checkpoint, budget
│   ├── memory.rs           # Memory summary, search, remember, forget
│   ├── research.rs         # Research list runs, load run, load section
│   ├── import.rs           # Import preview, confirm
│   ├── shell.rs            # Shell list, include, rerun, kill
│   ├── security.rs         # Security review dispatch
│   └── diagnostics.rs      # Doctor, diagnostics commands
├── runtime/                # Runtime logic (extracted from mod.rs)
│   ├── mod.rs              # Re-exports
│   ├── command_dispatch.rs # Main command dispatch match (TuiCommand routing)
│   ├── app_events.rs       # Bus event handling (AppEvent subscription and dispatch)
│   └── render_recovery.rs  # Render panic recovery (progressive fallback logic)
├── components/             # UI widgets and components
│   ├── component/          # Component trait and FocusManager
│   │   ├── component.rs    # Component trait, DialogType enum (NOT mod.rs)
│   │   ├── focus.rs        # FocusManager for modal focus stack
│   │   └── context.rs      # AppContext for overlay dialogs
│   ├── dialogs/            # Modal dialogs (all implement Component trait)
│   │   ├── agent.rs        # AgentDialog
│   │   ├── command.rs      # CommandPalette
│   │   ├── confirm.rs      # ConfirmDialog
│   │   ├── connect.rs      # ConnectDialog (provider API key entry)
│   │   ├── diff.rs         # DiffDialog
│   │   ├── goto.rs         # GotoDialog (jump to message)
│   │   ├── help.rs         # HelpDialog
│   │   ├── import.rs       # ImportDialog (import sessions)
│   │   ├── info.rs         # InfoDialog (Context/Cost/Usage)
│   │   ├── keybind.rs      # KeybindDialog
│   │   ├── mcp.rs          # McpDialog (MCP server management)
│   │   ├── model.rs        # ModelDialog (model selection)
│   │   ├── permission.rs   # PermissionDialog
│   │   ├── plan.rs         # PlanDialog
│   │   ├── question.rs     # QuestionDialog
│   │   ├── research.rs     # ResearchBrowserDialog (research runs browser)
│   │   ├── review.rs       # ReviewDialog (diff review)
│   │   ├── session.rs      # SessionDialog
│   │   ├── share.rs        # ShareDialog (share sessions)
│   │   ├── template.rs     # TemplateDialog
│   │   ├── theme.rs         # ThemePickerDialog
│   │   └── tree.rs         # TreeDialog (session hierarchy)
│   ├── completion_overlay.rs # Slash/file/agent completion popups
│   ├── diff.rs             # DiffViewer (diff visualization)
│   ├── help_overlay.rs     # HelpOverlay (dead code — not imported; help is now mode-aware via input.rs)
│   ├── image.rs            # ImageViewer (image rendering via ANSI)
│   ├── messages.rs         # MessagesWidget (message display, streaming)
│   ├── notification.rs     # NotificationManager (desktop notifications)
│   ├── prompt.rs           # PromptWidget (input prompt)
│   ├── scroll.rs           # CenteredScroll (reusable scrolling)
│   ├── sidebar.rs          # SidebarWidget (side panel, git info, file changes with diff stats)
│   ├── spinner.rs          # SpinnerWidget (busy indicator)
│   ├── status_bar.rs       # StatusBarWidget (bottom status: status + tokens)
│   ├── toast.rs            # ToastManager (notifications)
│   └── tool_output.rs      # ToolOutput (tool execution output display)
├── input.rs                # Key event handling, keybindings, InputMode
├── layout.rs               # Layout calculations, TuiLayout
├── route.rs                # Route/RouteManager (Home, Session routes)
├── theme.rs                # Theme definitions (31 themes)
├── file_diff.rs            # Async diff stats computation for sidebar file changes
├── task_lifecycle.rs       # Task registry for lifecycle tracking (TuiTaskRegistry)
├── async_cmd.rs            # Async command spawn helpers (spawn_tui_task, spawn_registered_tui_task)
├── command.rs              # Slash command registry
└── mod.rs                  # TUI entry point, event loop (~1450 lines after decomposition)
```

### Commands Directory (`commands/`)

Command handlers extracted from the monolithic `mod.rs` into domain-specific submodules. Each submodule exports public handler functions that are called from `runtime/command_dispatch.rs`.

| Module | Responsibility |
|--------|---------------|
| `sessions.rs` | Session CRUD: delete, archive, fork, bulk ops, rename, share/unshare, export, reload |
| `tasks.rs` | Background task operations: list, delete, schedule |
| `goals.rs` | Goal lifecycle: set, pause, resume, clear, done, checkpoint, budget |
| `memory.rs` | Persistent memory: summary, search, remember, forget |
| `research.rs` | Research browser: list runs, load run, load section |
| `import.rs` | Session import: preview, confirm |
| `shell.rs` | Human shell: list, include, rerun, kill |
| `security.rs` | Security review dispatch |
| `diagnostics.rs` | Diagnostics: doctor, context diagnostics |

### Runtime Directory (`runtime/`)

Runtime logic extracted from `mod.rs` to separate concerns:

| Module | Responsibility |
|--------|---------------|
| `command_dispatch.rs` | The main `match cmd { ... }` dispatch that routes `TuiCommand` variants to handler functions in `commands/` |
| `app_events.rs` | Bus event handling: `AppEvent` subscription, match, and dispatch to appropriate state mutations |
| `render_recovery.rs` | Progressive render panic recovery logic (component fallbacks, root fallback, state reset) |

## State Domains

The `App` struct is organized into 6 state domains:

### UiState (`app/state/ui.rs`)

```rust
pub struct UiState {
    pub theme: Arc<Theme>,              // Current color theme
    pub layout: TuiLayout,              // Layout manager
    pub sidebar_visible: bool,          // Sidebar visibility
    pub auto_scroll: bool,              // Auto-scroll messages
    pub show_thinking: bool,            // Show reasoning/thinking
    pub show_timestamps: bool,          // Show message timestamps
    pub routes: RouteManager,           // Home/Session navigation
    pub dialog: Dialog,                 // Current dialog
    pub command_mode: bool,             // Slash command mode
    pub input_mode: InputMode,          // Insert/Normal (vim-style)
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    pub help_lines: Vec<String>,        // Help content (deprecated — generated dynamically by build_help_lines())
    pub bindings: HashMap<(KeyModifiers, KeyCode), InputAction>,
    pub keybinds: Option<KeybindConfig>, // Raw keybind config
    pub remote_mode: bool,              // Cline compatibility
    pub remote_status: Option<String>,  // Remote connection status
    pub running: bool,                  // Event loop running flag
    pub timeline_visible: bool,         // Timeline visibility
    pub timeline_selected: usize,       // Timeline selection index
    pub tts: Tts,                       // Text-to-speech
    pub tts_enabled: bool,
    pub fullscreen: bool,               // DEC 1049 alternate screen
    pub dirty_regions: Vec<Rect>,       // Partial redraw optimization
    pub render_panic_count: usize,
    pub last_render_error: Option<String>,
    pub resize_debounce: Option<std::time::Instant>, // Resize debounce timer
}
```

### SessionState (`app/state/session.rs`)

```rust
pub struct SessionState {
    pub session: Option<Session>,
    pub session_status: SessionStatus,  // Idle/Working/Error
    pub token_in: u64,
    pub token_out: u64,
    pub reasoning_tokens: usize,
    pub history: VecDeque<HistoryEntry>,
    pub history_pos: Option<usize>,     // History navigation position
    pub indexed_files: Arc<RwLock<Vec<String>>>,
    pub project_dir: String,
    pub last_edited_file: Option<String>, // Most recently edited file path
    pub changed_files: Vec<ChangedFile>,
    // DiffStatsState (src/tui/app/state/session.rs):
    // pub enum DiffStatsState {
    //     Pending { generation: u64 },
    //     Ready { generation: u64, additions: usize, deletions: usize },
    //     Skipped { generation: u64, reason: &'static str },
    //     Error { generation: u64, message: String },
    // }
    pub mcp_servers: Vec<(String, String)>,
    pub context_tokens: usize,
    pub context_limit: usize,
    pub compaction_count: usize,
    pub rpm_limit: Option<u64>,         // Requests per minute limit
    pub tpm_limit: Option<u64>,         // Tokens per minute limit
    pub rpm_remaining: Option<u64>,     // RPM remaining in current window
    pub tpm_remaining: Option<u64>,     // TPM remaining in current window
    pub permission_pending: bool,       // Permission dialog is pending
    pub subagent_count: usize,
}
```

### AgentState (`app/state/agent.rs`)

```rust
pub struct AgentState {
    pub agents: Vec<Agent>,              // Available agents
    pub current_agent: usize,           // Selected agent index
    pub current_model: String,          // Current model (provider/name)
    pub models: Vec<String>,            // Available models
    pub model_idx: usize,
    pub plan_mode: bool,                // Plan/build mode
    pub plan_topic: Option<String>,
}
```

### DialogState (`app/state/dialog.rs`)

Contains all dialog instances, including optional dialogs:
- Always instantiated: `model_dialog`, `agent_dialog`, `session_dialog`, `tree_dialog`, `command_palette`
- On-demand (modal dialogs): `theme_picker`, `question_dialog`, `permission_dialog`, `keybind_dialog`, `mcp_dialog`, `share_dialog`, `import_dialog`, `template_dialog`, `connect_dialog`, `goto_dialog`, `plan_dialog`, `diff_dialog`, `help_dialog`, `info_dialog`, `review_dialog`, `research_browser`

**Pending fields** (for tracking pending permission/question responses):
- `permission_perm_id: Option<String>` - permission ID when permission dialog is pending
- `question_session_id: Option<String>` - session ID when question dialog is pending

## Routes

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Route {
    #[default]
    Home,           // Welcome screen
    Session(String), // Active session view
}
```

## Dialog Variants

```rust
pub enum Dialog {
    None,
    Model, Agent, Session, Help, Tree, Theme,
    Question, Permission, Mcp, Keybind,
    Share, Import, Template, Connect,
    Context, Cost, Usage, Stats, Goto, Plan, Diff, Confirm,
    Review,              // Diff review dialog
    ResearchBrowser,     // Research browser for web research
}
```

## Input Handling

### InputMode

```rust
pub enum InputMode {
    #[default]
    Insert,  // Text input mode
    Normal,  // Navigation mode (vim-style)
}
```

### InputAction

Key events are mapped to InputAction via keybindings:
- `Send`, `Newline`, `Cancel` - submission
- `NavigateUp`, `NavigateDown` - selection
- `SwitchAgent`, `SelectModel`, `ClearSession`, `NewSession` - actions
- `FocusPrompt`, `StashPrompt`, `RestorePrompt` - prompt management
- `Char`, `Backspace`, `Delete`, `CursorLeft/Right/Home/End` - text input
- `PageUp`, `PageDown`, `Search`, `GoToTop`, `GoToBottom` - navigation

## Event Handling

### TuiMsg

Internal messages from TUI to App (in `app/types.rs`):

```rust
pub enum TuiMsg {
    SubmitPrompt, NavigateUp, NavigateDown, NavigateLeft, NavigateRight, CycleAgent,
    OpenModelDialog, OpenAgentDialog, OpenSessionDialog, OpenHelpDialog,
    SelectModel { model: String }, SelectAgent { agent_name: String },
    SelectSession(Box<Session>),  // Full Session object, not just session_id
    OpenDiffDialog { old_content: Box<str>, new_content: Box<str>, title: Box<str> },
    OpenShareDialog, OpenThemeDialog, ExternalEditor, UndoDelete,
    ConfirmResult(Option<bool>),  // Confirmed=true, Cancelled=false, Dismissed=None
    ReviewOpenDiff { path: String },  // Open review for file path
    ResearchOpenRun { run_id: String },  // Open research run
    ResearchRefreshRuns,  // Refresh research runs list
    ResearchLoadSection { run_id: String, section: String },  // Load research section
    // ... and many more
}
```
```

### TuiCommand

Async commands sent via channel (in `app/mod.rs`):

```rust
pub enum TuiCommand {
    DeleteSession { session_id: String },
    ArchiveSession { session_id: String, unarchive: bool },
    UndoDelete { session_id: String },
    ForkSession { session_id: String },
    ShareSession { session_id: String },
    UnshareSession { session_id: String },
    ExportSession { session_id: String },
    RenameSession { session_id: String, new_title: String },
    BulkDelete { session_ids: Vec<String> },
    BulkArchive { session_ids: Vec<String>, unarchive: bool },
    BulkExport { session_ids: Vec<String> },
    ReloadSessions,
    OpenTreeDialog,
    PreviewImport { source: ImportSource },
    ConfirmImport { source: ImportSource },
    CreateFromTemplate { key: String, template: SessionTemplate },
    LoadSessionMessages { session_id: String },
    SpawnSubagent { agent_name: String, prompt: String },
    ListTasks,
    DeleteTask { id: String },
    TaskSchedule { interval_secs: u64, message: String },
    WorktreeList,
    MemorySummary,
    MemorySearch { query: String },
    MemoryRemember { text: String },
    MemoryForget { id: String },
    CompactSession,
    OpenDiffDialog { old_content: Box<str>, new_content: Box<str>, title: Box<str> },
    SendNotification { notification_type: NotificationType, body: String },
    GoalSet { session_id, project_id, objective },
    GoalFromFile { session_id, project_id, path },
    GoalShow { session_id },
    GoalPause { session_id },
    GoalResume { session_id },
    GoalClear { session_id },
    GoalDone { session_id },
    GoalCheckpoint { session_id, project_id },
    GoalBudget { session_id, subcommand },  // "show" or "raise <axis> <n>"
    RefreshSessionState { session_id },
    UpdateModels(Vec<String>),
    SessionsReloaded { sessions: Vec<SessionDto>, message_counts: HashMap<String, usize>, error: Option<String> },
    SessionMessagesLoaded { session_id: String, messages: Vec<Message>, error: Option<String> },
    TreeDialogLoaded { current_session_id: Option<String>, nodes: Vec<TreeNode>, error: Option<String> },
    ImportPreviewLoaded { request_id: u64, session: Option<Session>, msg_count: usize, error: Option<String> },
    ImportConfirmed { request_id: u64, session: Option<Session>, error: Option<String> },
    ResearchRunsLoaded { request_id: u64, runs: Vec<ResearchRunSummary>, error: Option<String> },
    ResearchRunLoaded { request_id: u64, run_id: String, bundle: Option<Box<ResearchBundle>>, error: Option<String> },
    ResearchSectionLoaded { request_id: u64, section: String, content: Option<(ReportSection, String)>, error: Option<String> },
    MemoryResult { toast_message: String, is_error: bool },
    DoctorResult { summary: String, is_error: bool },
    FileDiffStatsReady { path: PathBuf, generation: u64, result: FileDiffStatsResult },
}
```

## Component Trait

All dialogs implement the `Component` trait from `src/tui/components/component.rs`:

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

### DialogType

```rust
pub enum DialogType {
    Share, Model, Agent, Session, Help, Tree, Theme, Permission,
    Mcp, Question, Diff, Import, Template, Connect, Keybind,
    Context, Cost, Usage, Stats, Goto, Plan, Confirm,
    Review,           // Diff review dialog
    ResearchBrowser,  // Research browser dialog
    None,
}
```

## FocusManager

Modal focus handling via stack in `components/component/focus.rs`:

```rust
pub struct FocusManager {
    stack: VecDeque<Box<dyn Component>>,
    focus_index: usize,
}
```

Key methods:
- `push(component)` - add dialog to stack
- `pop()` - remove top dialog
- `top()` / `top_mut()` - access top dialog
- `handle_key(key)` - delegate to top dialog
- `active_dialog_type()` - current dialog type

### Dialog Lifecycle

**Opening**: `open_dialog()` sets `ui_state.dialog` and pushes component to FocusManager

**Confirm dialogs**: `push_dialog()` creates temporary dialogs (like ConfirmDialog)

**Closing**: `close_dialog()` pops FocusManager and syncs `ui_state.dialog` from `active_dialog_type()`

## Terminal Lifecycle

Terminal setup and teardown is managed by `TerminalGuard` (`src/tui/terminal.rs`).

### Setup Order (in `TerminalGuard::enter()`)

1. Enter alternate screen
2. Enable raw mode
3. Enable bracketed paste
4. Enable mouse capture

### Teardown Order (in `TerminalGuard::restore()`)

1. Disable mouse capture
2. Disable bracketed paste
3. Disable raw mode
4. Leave alternate screen

`TerminalGuard::restore()` is idempotent. The `Drop` impl calls `restore()`. If any setup step fails, all previously enabled features are rolled back before returning the error.

## Logging and Diagnostics

### Logging Policy

Normal builds use `tracing` only. The `debug_log!` macro in `src/tui/mod.rs` was removed. Feature-gated `debug_log!` macros remain in `src/tui/app/mod.rs` and `src/tui/input.rs` behind the `debug-logging` feature flag. No `codegg_debug.log` file is created in the working directory during normal operation.

### Tracing Targets

TUI tracing events use these targets:

| Target | Module |
|--------|--------|
| `codegg::tui::events` | Event loop and bus subscription |
| `codegg::tui::session` | Session state transitions |
| `codegg::tui::input` | Key event handling and dispatch |
| `codegg::tui::render` | Render pipeline and panic recovery |
| `codegg::tui::loop` | Main loop timing and diagnostics |

### TuiDiagnostics

The `TuiDiagnostics` struct tracks runtime performance metrics:

| Metric | Description |
|--------|-------------|
| Slow loop iterations | Iterations exceeding 250ms |
| Slow render frames | Frames exceeding 16ms (streaming) or 100ms (always logged) |
| Slow command handlers | Command dispatch exceeding threshold |
| Dropped bus events | Broadcast receiver lag (missed events) |
| Render panic count | Number of render panics recovered |
| Component render panic count | Number of component-level render panics |
| Last render error | Most recent render panic message |

Recent slow commands, slow renders, and component render panics are stored in bounded ring buffers for inspection.

`/tui-stats` also reports background task lifecycle stats (active tasks by kind, oldest task, cancelled count) and shell handle count, appended to the diagnostics summary.

### Diagnostics Command

`/tui-stats` displays a summary of runtime diagnostics including slow iterations, dropped events, render panics, and recent slow command/render records.

### Render Panic Recovery

- **Component-level**: `App::render()` wraps risky surfaces (viewport, sidebar, dialog, completions, timeline) in `std::panic::catch_unwind`. A component panic renders a compact fallback in that region. `TuiDiagnostics` tracks `component_render_panic_count` and `recent_component_render_panics` for observability.
- **Root-level**: `run_event_loop` wraps `terminal.draw()` in `catch_unwind`. Recovery is progressive:
  - First root failure: log + render error screen
  - Repeated failures (≥1): hide optional overlays/dialogs
  - Final fallback (≥3 = `MAX_RENDER_PANICS`): reset minimal volatile UI state
- `clear_render_error()` resets only `render_panic_count` and `last_render_error`.
- `App::reset_state()` clears dialog, command_mode, timeline_visible, show_completions, completion_filter. Does NOT clear prompt text or search state.

## Rendering Flow

```
┌──────────────────────────────────────────────────────────────┐
│                        run_event_loop()                       │
│                                                               │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │ EventStream │───►│ on_key()    │───►│ process_msg()│      │
│  │ (keyboard)  │    │ (dispatch)  │    │ (TuiMsg)    │      │
│  └─────────────┘    └─────────────┘    └──────┬──────┘      │
│                                                │              │
│                                                ▼              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │ render()    │◄───│ App::render │◄───│ State       │      │
│  │ (Terminal)  │    │             │    │ mutations   │      │
│  └─────────────┘    └─────────────┘    └─────────────┘      │
└──────────────────────────────────────────────────────────────┘
```

### Render Order

1. **Header**: Agent name, model, session info, and active indicators
2. **Timeline**: Optional timeline panel (when `timeline_visible` is true)
3. **Viewport**: Messages (Home or Session view)
4. **Prompt**: Input area with status indicator, mode indicator
5. **Footer**: Status bar with: session status, goal indicator (`[status] title budget`), subagent count, token counts, keybinds
6. **Sidebar**: Optional session/agent info panel (if visible)
7. **Dialog**: Modal overlay via FocusManager (if open)
8. **Completions**: Slash/file/agent completion popup (if active)
9. **Toasts**: Notification messages (topmost)

## Event Subscriptions

TUI subscribes to `GlobalEventBus` for:

| Event | Handler Action |
|-------|---------------|
| `TextDelta` | Append to messages |
| `ReasoningDelta` | Add reasoning text |
| `ToolCallStarted` | Add tool call entry |
| `ToolResult` | Update tool call status |
| `AgentFinished` | Update session status, trigger memory consolidation |
| `PermissionPending` | Show permission dialog |
| `QuestionPending` | Show question dialog |
| `FileChanged` | Cheap state mutation (mark Pending, update sidebar), spawn background diff via `spawn_sidebar_diff_stats()` |
| `SubagentStarted/Progress/Completed/Failed` | Show toasts |
| `CompactionTriggered` | Show toast |
| `TodoUpdated` | Update sidebar todo list |
| `GoalUpdated` | Update `app.active_goal`, refresh status bar |
| `GoalUsageUpdated` | Update usage on `app.active_goal` |
| `GoalBudgetLimited` | Show budget-limited toast |
| `GoalCompleted` | Clear active goal, show completion toast |

## Keyboard Shortcuts

Help text is mode-aware (Phase 5). The `/help` dialog content is generated by
`build_help_lines(vim_mode, active_mode)` in `src/tui/input.rs`, not hardcoded.
`HelpMode` (Insert/Normal/Command/Dialog) and `HelpEntry` types centralize help
metadata. `default_help_entries()` provides the base list; `help_entries_for_mode()`
filters entries by the active mode. In **insert mode**, only modifier-based shortcuts
(Ctrl+*, Shift+*) are shown as shortcuts — bare `?`, `/`, `j`, `k` insert text. In
**normal mode**, bare navigation keys (`j`, `k`, `h`, `l`, `?`, `/`) are shown as
shortcuts.

### Global Shortcuts

| Shortcut | Mode | Action |
|----------|------|--------|
| `Enter` | Insert | Send prompt |
| `Shift+Enter` | Insert | Newline in prompt |
| `Esc`, `Ctrl+C` | Any | Cancel operation |
| `↑/k`, `↓/j` | Normal | Navigate up/down |
| `Tab` | Normal | Switch agent |
| `Shift+Tab` | Normal | Toggle permission mode |
| `Ctrl+L` | Normal | Model selector |
| `Ctrl+K` | Normal | Clear session |
| `Ctrl+N` | Normal | New session |
| `Ctrl+T` | Normal | Toggle sidebar |
| `Ctrl+W` | Normal | Close session |
| `/` | Normal | Focus prompt |
| `?` | Normal | Help |
| `Ctrl+S` | Normal | Stash prompt |
| `Ctrl+R` | Normal | Restore prompt |
| `Ctrl+P` | Normal | Cycle model forward |
| `Ctrl+Shift+P` | Normal | Cycle model backward |
| `Ctrl+Y` | Normal | Toggle TTS |
| `Ctrl+Shift+Y` | Normal | Stop TTS |
| `Ctrl+Shift+F` | Normal | Toggle fullscreen |
| `PgUp/PgDn` | Any | Page scroll |
| `Ctrl+F` | Any | Search |

> **Note:** `help_overlay.rs` exists but is dead code — it is not imported.
> The help dialog is rendered inline from the mode-aware `build_help_lines()` output.

## GlobalEventBus Integration

The TUI uses `GlobalEventBus::subscribe()` to receive events from AgentLoop:

```rust
let mut bus_rx = GlobalEventBus::subscribe();

tokio::select! {
    Some(result) = reader.next() => { /* keyboard/mouse */ }
    Ok(event) = bus_rx.recv() => {
        match event {
            AppEvent::TextDelta { delta, .. } => { /* ... */ }
            AppEvent::ToolCallStarted { tool_name, tool_id, arguments, .. } => { /* ... */ }
            // ... handle other events
        }
    }
}
```

### ClickTarget Enum

Mouse interaction targets for click handling:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ClickTarget {
    Viewport,   // Main message area
    Prompt,     // Input prompt area
    Dialog,     // Active dialog overlay
    Completion, // Completion popup
    Sidebar,    // Sidebar panel
    None,       // No target (background)
}
```

Used by `clickable_area_at()` to determine which UI region was clicked, and `on_click()` to handle the interaction appropriately.

The `App` struct (in `src/tui/app/mod.rs`) includes these fields (among many others):

```rust
pub struct App {
    // ... state domains ...
    pub busy_spinner: SpinnerWidget,  // Animated busy indicator
    pub focus_manager: FocusManager,  // Modal focus stack
    pub notification_manager: Option<NotificationManager>,
    pub undo_session_id: Option<String>,
    pub undo_until: Option<Instant>,
    pub bg_scheduler: Option<Arc<BackgroundScheduler>>,
    pub config_watcher: Option<ConfigWatcher>,
    pub core_client: Option<Arc<dyn CoreClient>>,
    pub active_goal: Option<GoalSnapshot>,  // Active goal for status bar
    // ... other fields ...
}
```

**`busy_spinner: SpinnerWidget`** - Located at `src/tui/components/spinner.rs`. Shows animated busy indicator (frames: `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]`). Starts when `session_status` is `Working`, stops on `Idle` or `Error`. Tick called every render frame (~60fps).

## Remote TUI Protocol (Phase 8)

### Protocol Model

The remote TUI uses an **event/state-driven** protocol. The daemon sends typed state snapshots and event deltas; remote clients render independently. Frame-driven rendering (`RenderFrame`) is explicitly **unsupported** — receiving it returns an `Error` response with code `unsupported_render_frame`.

### Protocol Version

The remote TUI protocol version is defined as `REMOTE_TUI_PROTOCOL_VERSION = 1` in `crates/codegg-protocol/src/tui.rs`. Handshakes should reject incompatible major versions.

### State Snapshots

`RemoteTuiStateSnapshot` is a frontend-neutral DTO containing only render-relevant state: route, model, agent, status, messages (as previews), prompt, dialog, and toasts. Snapshots are produced by `App::remote_snapshot()` which is a pure, nonblocking read of current `App` state.

### Resync

On reconnect or sequence gaps, the server replays events from the EventLog and sends a `ResyncRequired` event. Clients can request a full resync via `RequestSnapshot` — the server responds by replaying all events from sequence 0 followed by `ResyncRequired`. When a client receives a `StateSnapshot`, it applies the snapshot fields (model, status) to local App state. The `ResyncRequired` event is also sent when the broadcast channel lags.

### Unsupported RenderFrame

If a `RenderFrame` payload is received, the handler returns a `TuiMessage::Error` with:
- message: `Frame-driven remote rendering is not supported; request state snapshots instead (unsupported_render_frame)`

The `unsupported_render_frame` identifier is embedded in the message string. This replaces the previous silent log-and-ignore behavior.

### Deferred Items

The following Phase 8 items are defined in the plan but deferred for future work:

- **Hello/Ack handshake** — No protocol version negotiation on connect. Handshakes should reject incompatible major versions or degrade explicitly.
- **Structured RemoteTuiError** — Plan specifies `{ code, message, recoverable }` fields. Current implementation uses `TuiMessage::Error { message: String }` with code embedded in the message string.
- **Server-side input forwarding** — `Input`, `KeyDown`, `MouseClick`, `Resize` handlers in the server WebSocket are logging-only stubs. Remote user input should eventually convert into the same input/command paths as local input.
- **State delta protocol** — `RemoteTuiStateDelta` is not implemented. Snapshot-only is acceptable per the plan.

## Testing

TUI render regression tests live in `tests/tui_render.rs` (95 tests). They use `ratatui::backend::TestBackend` to exercise `App::render()` across multiple terminal sizes without requiring an interactive terminal. Component-level panic injection tests verify fallback behavior for messages, sidebar, dialog, completions, and timeline surfaces.

**Run all render regression tests:**

```bash
cargo test --test tui_render
```

**Test matrix** (terminal sizes):

| Size | Dimensions | Purpose |
|------|-----------|---------|
| tiny | 40x12 | Minimal viable terminal |
| small | 60x20 | Compact terminal |
| normal | 100x32 | Standard terminal |
| wide | 160x40 | Ultra-wide terminal |
| tall | 100x60 | Tall terminal |

**Coverage areas:**

- Empty/home state (sidebar visible/hidden)
- Active session with messages
- Streaming state with active tokens
- Tool calls (pending, completed, error)
- Sidebar with file changes (pending, ready, skipped, error states)
- All 28 dialog variants (help, model, session, agent, tree, theme, mcp, keybind, cost, usage, stats, goto, plan, confirm, review, context, connect, template, share, import, question, permission, diff, research browser, security review, source preview, shell show)
- Completion overlay at various sizes
- Toast notifications
- Pathological content (long lines, wide Unicode, ANSI escapes, malformed JSON, combining marks)
- Component panic injection (messages, sidebar, dialog, completions, timeline fallbacks)
- Component fallback diagnostics tracking
- Error dialog rendering
- Timeline visible state
- Tool-only messages (no user messages)
- Shell cells and reasoning content
- Combined states (sidebar + messages + toasts, dialog + sidebar, streaming + dialog + sidebar + toasts, etc.)

**Key patterns:**

- `render_app_to_buffer(app, w, h)` — renders to `TestBackend`, returns `Buffer`
- `assert_render_ok(app, w, h)` — asserts no panic, returns buffer
- `text_in_buffer(buffer)` — extracts rendered text as string
- `buffer_contains(buffer, needle)` — case-insensitive substring search
- Tests avoid brittle full-screen snapshots; use semantic assertions instead

**Bug fix included:** `PromptWidget::clamp_scroll` and `ensure_cursor_visible` now use `saturating_sub` for `visible_lines - 1` to prevent arithmetic overflow at very small terminal sizes.

## See Also

- [agent.md](agent.md) - AgentLoop that processes TUI commands
- [bus.md](bus.md) - GlobalEventBus and event types
- [session.md](session.md) - Session storage
- `.opencode/skills/tui/SKILL.md` - Detailed TUI development guide
