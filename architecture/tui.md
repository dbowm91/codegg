# TUI Module

The `tui` module provides the terminal user interface using Ratatui.

## Overview

**Location**: `src/tui/`

**Key Responsibilities**:
- Terminal UI rendering with Ratatui
- Keyboard input handling
- Application state management
- Layout and component rendering
- Notifications and dialogs

## Directory Structure

```
tui/
├── app/              # Main application state
│   ├── mod.rs        # App struct
│   ├── types.rs      # App types
│   └── commands.rs   # App commands
├── components/       # UI widgets
├── input/            # Keyboard handling
├── layout/          # Layout management
├── theme.rs          # Color themes
└── route.rs          # State machine/routing
```

## Key Components

### app/ - Application State

#### App Struct

```rust
pub struct App {
    pub state: AppState,
    pub route: Route,
    pub session: SessionStore,
    pub config: Config,
    pub bus: GlobalEventBus,
}
```

**State**:
- `Route` - Current view (Chat, Sessions, Settings, etc.)
- `Dialog` - Active modal dialog
- `notifications` - Toast notifications

#### Routes

```rust
pub enum Route {
    Chat,
    Sessions,
    Settings,
    Skills,
    Permissions,
}
```

#### Dialogs

```rust
pub enum Dialog {
    Permission(PermissionRequest),
    Question(QuestionRequest),
    Confirm(ConfirmRequest),
    Error(String),
}
```

### components/ - UI Widgets

| Component | Description |
|-----------|-------------|
| **messages** | Chat message display |
| **prompt** | Input prompt |
| **sidebar** | Session list sidebar |
| **tabs** | Tab navigation |
| **status** | Status bar |
| **notifications** | Toast notifications |

### input/ - Keyboard Handling

```rust
pub enum InputMode {
    Normal,
    Insert,
    Command,
}
```

**Key Bindings**:
- `Normal` mode: Navigation, shortcuts
- `Insert` mode: Text input
- `Command` mode: `/` commands

### layout/ - Layout Management

Handles the terminal layout:
- Sidebar width
- Message area sizing
- Dialog centering

### theme.rs - Theming

```rust
pub struct Theme {
    pub colors: ColorPalette,
    pub fonts: FontSettings,
}

pub struct ColorPalette {
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub error: Color,
    pub success: Color,
}
```

## Event Handling

### TuiCommand

Internal commands from TUI to AgentLoop:

```rust
pub enum TuiCommand {
    Submit(String),           // User submitted message
    SelectSession(String),    // Switch session
    DeleteSession(String),    // Delete session
    ToggleSidebar,            // Show/hide sidebar
    // ...
}
```

### TuiMsg

Responses back to TUI:

```rust
pub enum TuiMsg {
    SessionUpdated(Session),
    Notification(String),
    PermissionRequest(PermissionDetails),
    QuestionRequest(QuestionDetails),
    RouteChanged(Route),
}
```

## Rendering Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                        run_event_loop()                          │
│                                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
│  │ on_key()    │───►│ handle_key()│───►│ update()    │         │
│  │ (keyboard)  │    │ (Component) │    │ (App state) │         │
│  └─────────────┘    └─────────────┘    └──────┬──────┘         │
│                                               │                 │
│                                               ▼                 │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
│  │ render()    │◄───│ App::render │◄───│ State       │         │
│  │ (Terminal)  │    │             │    │ mutations   │         │
│  └─────────────┘    └─────────────┘    └─────────────┘         │
└─────────────────────────────────────────────────────────────────┘
```

## Event Subscriptions

TUI subscribes to `GlobalEventBus` for:

- `Session*` events - Session changes
- `MessageAdded` - New messages
- `ToolPermissionPending` - Permission dialogs
- `Notification` - Toast notifications
- `Indicator` - Status indicators

## Component Trait

All dialogs/components implement `Component` trait:

```rust
pub trait Component {
    fn handle_key(&mut self, key: Key) -> bool;
    fn update(&mut self, msg: TuiMsg);
    fn render(&self, area: Rect, buf: &mut Buffer);
}
```

## Keyboard Shortcuts

| Shortcut | Mode | Action |
|----------|------|--------|
| `Ctrl+C` | Normal | Cancel current operation |
| `Ctrl+Q` | Normal | Quit application |
| `Ctrl+S` | Normal | Force save session |
| `/` | Normal | Open command mode |
| `Esc` | Any | Close dialog/cancel |
| `Tab` | Normal | Cycle sidebar |
| `?` | Normal | Show help |

## See Also

- [agent.md](agent.md) - AgentLoop that processes TUI commands
- [event-bus.md](event-bus.md) - Event subscriptions
- [session.md](session.md) - Session storage
