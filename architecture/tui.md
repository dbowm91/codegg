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

## Directory Structure

```
tui/
├── app/                    # Main application state
│   ├── mod.rs              # App struct (5978 lines), event loop, key handling
│   ├── types.rs            # Dialog, TuiMsg, TuiCommand, SessionStatus, etc.
│   └── state/              # State domains
│       ├── agent.rs        # AgentState (models, agents, selection)
│       ├── dialog.rs       # DialogState (dialog instances, dialog visibility)
│       ├── messages.rs     # MessagesState (message history, toasts, spinner)
│       ├── prompt.rs       # PromptState (prompt, completions)
│       ├── session.rs      # SessionState (session, history, git info)
│       └── ui.rs           # UiState (theme, layout, routes, keybindings)
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
│   │   ├── question.rs      # QuestionDialog
│   │   ├── session.rs      # SessionDialog
│   │   ├── share.rs        # ShareDialog (share sessions)
│   │   ├── template.rs     # TemplateDialog
│   │   ├── theme.rs         # ThemePickerDialog
│   │   └── tree.rs         # TreeDialog (session hierarchy)
│   ├── completion_overlay.rs # Slash/file/agent completion popups
│   ├── diff.rs             # DiffViewer (diff visualization)
│   ├── footer.rs           # FooterWidget (status bar)
│   ├── help_overlay.rs     # HelpOverlay (keyboard shortcuts overlay)
│   ├── image.rs            # ImageViewer (image rendering via ANSI)
│   ├── messages.rs         # MessagesWidget (message display, streaming)
│   ├── notification.rs     # NotificationManager (desktop notifications)
│   ├── prompt.rs           # PromptWidget (input prompt)
│   ├── scroll.rs           # CenteredScroll (reusable scrolling)
│   ├── sidebar.rs          # SidebarWidget (side panel, git info)
│   ├── spinner.rs          # SpinnerWidget (busy indicator)
│   ├── toast.rs            # ToastManager (notifications)
│   └── tool_output.rs      # ToolOutput (tool execution output display)
├── input.rs                # Key event handling, keybindings, InputMode
├── layout.rs               # Layout calculations, TuiLayout
├── route.rs                # Route/RouteManager (Home, Session routes)
├── theme.rs                # Theme definitions (33 themes)
├── command.rs              # Slash command registry
└── mod.rs                  # TUI entry point, event loop, GlobalEventBus
```

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
    pub help_lines: Vec<String>,        // Help content
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
- `model_dialog`, `agent_dialog`, `session_dialog`, `tree_dialog`, `command_palette` - always instantiated
- `theme_picker`, `question_dialog`, `permission_dialog`, `keybind_dialog`, `mcp_dialog` - created on demand
- `share_dialog`, `import_dialog`, `template_dialog`, `connect_dialog`, `goto_dialog`, `plan_dialog`, `diff_dialog`, `help_dialog`, `info_dialog` - created on demand

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
    Context, Cost, Usage, Goto, Plan, Diff, Confirm,
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
    UpdateModels(Vec<String>),
}
```

## Component Trait

All dialogs implement the `Component` trait from `src/tui/components/component.rs`:

```rust
pub trait Component: Send {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> { None }
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool { self.dialog_type().is_modal() }
    fn hit_test(&self, rel_y: usize) -> Option<usize> { None }
    fn set_selected(&mut self, idx: usize) {}
}
```

### DialogType

```rust
pub enum DialogType {
    Share, Model, Agent, Session, Help, Tree, Theme, Permission,
    Mcp, Question, Diff, Import, Template, Connect, Keybind,
    Context, Cost, Usage, Goto, Plan, Confirm, None,
}
```

## FocusManager

Modal focus handling via stack in `components/component/focus.rs`:

```rust
pub struct FocusManager {
    stack: VecDeque<Box<dyn Component>>,
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

1. **Header**: Agent name, model, session info, active indicators
2. **Timeline**: Optional timeline panel (when `timeline_visible` is true)
3. **Viewport**: Messages (Home or Session view)
4. **Prompt**: Input area with status indicator, mode indicator
5. **Footer**: Token counts, session status, keybinds, TTS indicator
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
| `FileChanged` | Track changed files |
| `SubagentStarted/Progress/Completed/Failed` | Show toasts |
| `CompactionTriggered` | Show toast |

## Keyboard Shortcuts

| Shortcut | Mode | Action |
|----------|------|--------|
| `Enter` | Insert | Send prompt |
| `Shift+Enter` | Insert | Newline in prompt |
| `Esc`, `Ctrl+C` | Any | Cancel operation |
| `↑/j`, `↓/k` | Normal | Navigate up/down |
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
    // ... other fields ...
}
```

**`busy_spinner: SpinnerWidget`** - Located at `src/tui/components/spinner.rs`. Shows animated busy indicator (frames: `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]`). Starts when `session_status` is `Working`, stops on `Idle` or `Error`. Tick called every render frame (~60fps).

- [agent.md](agent.md) - AgentLoop that processes TUI commands
- [bus.md](bus.md) - GlobalEventBus and event types
- [session.md](session.md) - Session storage
- `.skills/tui/SKILL.md` - Detailed TUI development guide
