---
name: tui
description: Guide for working with Terminal UI in opencode-rs
version: 3.0.0
tags:
  - tui
  - ratatui
  - ui
  - dialogs
  - scroll
  - component
---

# TUI Development Guide

This skill covers working with the Terminal UI (TUI) in opencode-rs, built using **Ratatui**. All dialogs now implement the `Component` trait (Wave 7.2+) with FocusManager integration.

## Project Structure

```
src/tui/
├── app/
│   ├── mod.rs          # Main App struct and event loop
│   ├── state/
│   │   ├── mod.rs      # Exports all state modules
│   │   ├── agent.rs    # AgentState (models, agents, selection)
│   │   ├── dialog.rs   # DialogState (dialog instances)
│   │   ├── messages.rs # MessagesState (message history, toasts)
│   │   ├── prompt.rs   # PromptState (prompt, completions)
│   │   ├── session.rs  # SessionState (session, history)
│   │   └── ui.rs       # UiState (theme, layout, routes)
│   └── types.rs        # Dialog, TuiMsg, TuiCommand, SessionStatus, etc.
├── components/
│   ├── dialogs/
│   │   ├── agent.rs       # AgentDialog
│   │   ├── command.rs     # CommandPalette
│   │   ├── import.rs      # ImportDialog (import sessions)
│   │   ├── keybind.rs     # KeybindDialog
│   │   ├── mcp.rs         # McpDialog (MCP server management)
│   │   ├── model.rs       # ModelDialog (model selection)
│   │   ├── permission.rs  # PermissionDialog
│   │   ├── question.rs    # QuestionDialog
│   │   ├── session.rs     # SessionDialog
│   │   ├── share.rs       # ShareDialog (share sessions)
│   │   ├── theme.rs       # ThemePickerDialog
│   │   ├── tree.rs        # TreeDialog (session hierarchy)
│   │   └── mod.rs
│   ├── messages.rs     # MessagesWidget (message display)
│   ├── prompt.rs       # PromptWidget (input prompt)
│   ├── sidebar.rs      # SidebarWidget (side panel)
│   ├── spinner.rs      # SpinnerWidget (loading indicator)
│   ├── toast.rs        # ToastManager (notifications)
│   ├── scroll.rs       # CenteredScroll (reusable scrolling)
│   ├── diff.rs         # DiffViewer (diff visualization)
│   ├── image.rs        # ImageViewer (image rendering)
│   ├── notification.rs  # NotificationManager (desktop notifications)
│   └── mod.rs
├── input/              # Key event handling, keybindings
├── layout.rs           # Layout calculations
├── theme.rs            # Theme definitions
└── mod.rs              # TUI entry point, event loop
```

## Key Concepts

### State Management

The TUI uses a single `App` struct that holds all state. Key sub-states:

- `agent_state.models: Vec<String>` - List of model IDs in `provider/model` format
- `agent_state.current_model: String` - Currently selected model
- `agent_state.model_idx: usize` - Current selection index
- `agent_state.plan_mode: bool` - Whether in plan/build mode
- `agent_state.plan_topic: Option<String>` - Topic for plan mode
- `dialog_state.model_dialog: ModelDialog` - Model selection dialog
- `dialog_state.theme_picker: Option<ThemePickerDialog>` - Theme picker dialog

**Plan/Build Mode**: Toggle via `enter_plan_mode()` / `exit_plan_mode()` methods, or Shift+Tab keybinding. When enabled, `agent_state.plan_mode` is true.

### Dialog Rendering with Component Trait

Dialogs are rendered in `app/mod.rs` `render()` method using the `Component` trait. The FocusManager handles the active dialog:

```rust
// In render loop
if let Some(active) = self.focus_manager.top_mut() {
    active.render(frame, popup_area, &self.ui_state.theme);
}
```

**Critical**: `set_visible_height()` must be called before every render, as popup size can change.

## Component Trait Architecture

All dialogs implement the `Component` trait from `src/tui/components/component.rs`:

```rust
pub trait Component: Send {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool {
        self.dialog_type().is_modal()
    }

    // Optional methods for mouse support
    fn hit_test(&self, _rel_y: usize) -> Option<usize> { None }
    fn set_selected(&mut self, _idx: usize) {}
}
```

### DialogType Enum

Each dialog returns its type via `dialog_type()`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum DialogType {
    Share, Model, Agent, Session, Help, Tree, Theme, Permission,
    Mcp, Question, Diff, Import, Template, Connect, Keybind,
    Context, Cost, Usage, Goto, Plan, Confirm, None,
}
```

### Migrating a Dialog to Component

1. **Add Clone**: `#[derive(Clone)]` or manual impl
2. **Add imports**:
   ```rust
   use crossterm::event::KeyEvent;
   use crate::tui::components::component::{Component, DialogType};
   use crate::tui::app::TuiMsg;
   ```
3. **Implement handle_key**:
   ```rust
   fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
       match key.code {
           crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
           crossterm::event::KeyCode::Up => { self.select_up(); None }
           crossterm::event::KeyCode::Down => { self.select_down(); None }
           _ => None,
       }
   }
   ```
4. **Implement update**: Process TuiMsg payloads, no stale dialog_state reads
   ```rust
   fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
       match msg {
           TuiMsg::SelectModel { model } => {
               self.selected_model = model;
               Some(TuiMsg::CloseDialog)
           }
           _ => None,
       }
   }
   ```
5. **Implement render**: Use `frame.buffer_mut()` directly (not Widget trait)
6. **Implement dialog_type**: Return matching `DialogType` variant

### Key Differences from Widget Trait

| Aspect | Widget Trait | Component Trait |
|--------|-------------|-----------------|
| Theme | `self.theme.field` | `theme` parameter |
| Buffer | `buf.render_widget(...)` | `frame.buffer_mut()` |
| Clone | Not required | Required for FocusManager |
| Key handling | Direct in App | Via `handle_key()` |
| State sync | Stale dialog_state reads | TuiMsg carries payload |

## FocusManager Integration

Dialogs are pushed to `FocusManager` for modal focus handling:

```rust
// In App - opening a dialog
let dialog = Box::new(self.dialog_state.model_dialog.clone());
self.focus_manager.push(dialog);
```

### Mouse Handling

Mouse dialog item selection delegates hit testing to the active `Component` via `FocusManager`:

```rust
pub trait Component: Send {
    // ... other methods ...

    /// Hit test a mouse click at the given row (relative to dialog area).
    /// Returns the item index if the row corresponds to a selectable item, or None.
    fn hit_test(&self, _rel_y: usize) -> Option<usize> { None }

    /// Set the selected item index. Used to sync state from mouse clicks.
    fn set_selected(&mut self, _idx: usize) {}
}
```

For `ModelDialog`, the `hit_test_model_row()` method maps rendered rows to model indices accounting for:
- Tab line (row 0)
- Blank line (row 1)
- Optional filter lines (filter + blank line)
- Provider header lines
- Scroll offset
- Current visible height

Mouse click flow:
1. User clicks on dialog
2. `handle_mouse_click()` maps terminal coordinates to dialog-relative `(rel_x, rel_y)`
3. `focus_manager.top().hit_test(rel_y)` maps row to item index
4. `focus_manager.top_mut().set_selected(idx)` syncs selection
5. Dialog processes the selection via TuiMsg

## Dialog Lifecycle Management

### Open Dialog

Dialogs must be created in `open_dialog()` and initialized **once**:

```rust
fn open_dialog(&mut self, dialog: Dialog) {
    match dialog {
        Dialog::Model => {
            self.dialog_state.model_dialog.initialize_selection();
            let dialog = Box::new(self.dialog_state.model_dialog.clone());
            self.focus_manager.push(dialog);
        }
        // ... other dialogs
    }
}
```

**Critical**: `initialize_selection()` should only be called once when dialog opens, NOT during render.

### Close Dialog

No cleanup needed for simple dialogs. For dialogs with temporary state (like `message_preview` HashMaps), clean up in Cancel handler:

```rust
Some(TuiMsg::CloseDialog) => {
    if matches!(self.ui_state.dialog, Dialog::Session) {
        self.dialog_state.session_dialog.clear_message_preview();
    }
    self.focus_manager.pop();
    self.ui_state.dialog = Dialog::None;
}
```

### Render Time Updates

If dialog state needs to change during render (like updating theme), use separate reference update methods:

```rust
impl ModelDialog {
    // GOOD: Reference update, can be called during render
    pub fn set_current(&mut self, current: &str) {
        self.current = current.to_string();
    }

    // BAD: Full initialization, mutates scroll
    pub fn initialize_selection(&mut self) {
        if let Some(idx) = self.models.iter().position(|m| m == &self.current) {
            self.selected = idx;
            self.scroll.clamp(...); // Recalculates scroll
        }
    }
}
```

## Navigation Pattern

Navigation flows through these layers:

1. **User Input** → KeyEvent in `app/mod.rs`
2. **Key Mapping** → `InputAction` enum (NavigateUp, NavigateDown, etc.)
3. **FocusManager** → `handle_key()` dispatches to active Component
4. **Component Methods** → `select_up()`, `select_down()` update selection and scroll
5. **TuiMsg** → Component returns TuiMsg which is processed by App

```rust
// In app/mod.rs event loop
if let Some(active) = self.focus_manager.top_mut() {
    if let Some(msg) = active.handle_key(key) {
        self.handle_tui_msg(msg);
    }
}
```

## Mouse Handling

Mouse dialog item selection now delegates hit testing to the active `Component` via `FocusManager`. See FocusManager Integration section above.

## Common Issues and Solutions

### Scroll "Pulling" Issue

**Problem**: Selection gets "pulled" away from its current position when navigating, causing it to disappear off-screen.

**Cause**: Always calling `scroll.clamp()` tries to recenter selection even when it's already in a good position.

**Solution**: Only adjust scroll when selection would be outside visible range:

```rust
pub fn select_up(&mut self) {
    if self.selected > 0 {
        self.selected -= 1;
    }
    let scroll = self.scroll.get();
    let visible_items = self.count_visible_items(scroll);
    if self.selected < scroll || self.selected >= scroll + visible_items {
        self.scroll.clamp(self.selected, self.items.len(), visible_items);
    }
}
```

### Selection Not Visible on Dialog Open

**Problem**: When dialog opens, selection starts at index 0 (top), requiring multiple arrow key presses to reach middle of list.

**Solution**: Center selection when dialog opens:

```rust
impl ModelDialog {
    pub fn initialize_selection(&mut self) {
        let flat = self.flat_filtered();
        if !flat.is_empty() {
            if !self.current.is_empty() {
                if let Some(idx) = flat
                    .iter()
                    .position(|(p, n)| format!("{}/{}", p, n) == self.current)
                {
                    self.selected = idx;
                    let visible_models = self.count_visible_models(0);
                    self.scroll.clamp(self.selected, flat.len(), visible_models);
                }
            } else if !flat.is_empty() {
                let visible_models = self.count_visible_models(0);
                self.selected = visible_models / 2;
                self.scroll.clamp(self.selected, flat.len(), visible_models);
            }
        }
    }
}
```

### Multi-line Item Clipping

**Problem**: Items that span multiple lines (themes with swatches, models with provider headers) get clipped at bottom.

**Solution**: Calculate actual number of items that fit in visible area:

```rust
impl ThemePickerDialog {
    fn count_visible_themes(&self, start_idx: usize) -> usize {
        let mut lines_used = 0;
        let mut themes_shown = 0;

        for (i, _theme) in self.themes.iter().enumerate().skip(start_idx) {
            if i < start_idx { continue; }

            let theme_lines = 2 + if i == self.selected { 1 } else { 0 };
            if lines_used + theme_lines > self.visible_height {
                break;
            }

            lines_used += theme_lines;
            themes_shown += 1;
        }

        themes_shown
    }
}
```

### Dialog Initialization During Render

**Problem**: Dialog initialization (like `set_theme()`) happens during render loop (~60fps), causing scroll to recalculate incorrectly.

**Solution**: Dialogs should only update reference values during render, not initialize state:

```rust
// GOOD - Only mutates reference, initialization happens in open_dialog
fn open_dialog(&mut self, dialog: Dialog) {
    match dialog {
        Dialog::Theme => {
            if self.dialog_state.theme_picker.is_none() {
                self.dialog_state.theme_picker = Some(ThemePickerDialog::new());
            }
            // Initialization happens here, not in render
            self.dialog_state.theme_picker.initialize_selection();
            let dialog = Box::new(self.dialog_state.theme_picker.clone());
            self.focus_manager.push(dialog);
        }
    }
}
```

## Performance Optimization

### Filtered List Caching

For dialogs with filtering (agents, sessions, models), cache filtered indices instead of recomputing on every render:

```rust
pub struct ModelDialog {
    filtered_cache: Option<(String, Vec<usize>)>,
}

impl ModelDialog {
    fn update_cache(&mut self) {
        if self.filter.is_empty() {
            self.filtered_cache = None;
            return;
        }

        let indices: Vec<usize> = self
            .models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.to_lowercase().contains(&self.filter.to_lowercase()))
            .map(|(i, _)| i)
            .collect();

        self.filtered_cache = Some((self.filter.clone(), indices));
    }

    fn filtered(&self) -> Vec<String> {
        if let Some((ref cache_filter, ref indices)) = self.filtered_cache {
            if cache_filter == &self.filter {
                return indices.iter().map(|&i| self.models[i].clone()).collect();
            }
        }
        // Fallback: recompute if cache miss or stale
        self.update_cache();
        // ... return filtered items
    }

    fn invalidate_cache(&mut self) {
        self.filtered_cache = None;
    }
}
```

**Key Points**:
- Cache invalidated when filter changes (`set_filter()`, `backspace_filter()`)
- Cache is `Option<(String, Vec<usize>)>` - stores filter string and indices
- Avoids cloning entire filtered vector (~60fps)
- Only cache when filter is non-empty to avoid unnecessary clones

### Invalidating Cache on Model Changes

When model list changes (new provider added, models refreshed), invalidate the cache:

```rust
// In app/mod.rs, when models are loaded
self.agent_state.models = new_models;
self.dialog_state.model_dialog.set_models(new_models);
self.dialog_state.model_dialog.invalidate_cache();
```

## Dialog Best Practices

### DO: ✅

1. **Initialize once** - Call initialization in `open_dialog()`, never in render
2. **Use Component trait** - Implement Component with FocusManager integration
3. **TuiMsg carries payload** - Don't read stale dialog_state, use TuiMsg payload
4. **Visible item counting** - Account for multi-line items when calculating scroll
5. **Prevent scroll pulling** - Only adjust scroll when selection would leave visible range
6. **Cache filtered indices** - Cache index vectors, not full filtered vectors
7. **Set visible height** - Always call `set_visible_height()` before rendering
8. **Use CenteredScroll** - Provides smooth, centered scrolling behavior
9. **Clean up state** - Clear temporary state (HashMaps, preview data) on close
10. **Focus trapping** - Block Tab key in modal dialogs to prevent focus escaping
11. **Empty state feedback** - Show "No results" instead of blank screen
12. **Alternating rows** - Use alternating backgrounds for list items

### DO NOT: ❌

1. **Never initialize in render** - This causes scroll to recalculate at 60fps
2. **Never mutate scroll in render** - Only reference updates during render
3. **Never read stale dialog_state** - Use TuiMsg payload for state synchronization
4. **Don't clone full filtered vectors** - Cache indices instead
5. **Don't use line count for scroll** - Use actual visible item count
6. **Don't render all items** - Stop after visible_items count reached
7. **Don't use filter every render** - Cache filtered indices
8. **Don't assume 1 line per item** - Multi-line items exist

## CenteredScroll Component

Located in `src/tui/components/scroll.rs`. Provides opencode-style scrolling:

```rust
pub struct CenteredScroll {
    scroll: usize,
}

impl CenteredScroll {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }

    pub fn reset(&mut self) {
        self.scroll = 0;
    }

    pub fn get(&self) -> usize {
        self.scroll
    }

    pub fn clamp(&mut self, cursor: usize, total: usize, visible: usize) {
        if total == 0 || visible == 0 {
            self.scroll = 0;
            return;
        }

        let max_scroll = total.saturating_sub(visible);
        let middle = visible / 2;

        let new_scroll = if cursor >= max_scroll {
            max_scroll
        } else if cursor < middle {
            0
        } else {
            cursor.saturating_sub(middle)
        };

        self.scroll = new_scroll.min(max_scroll);
    }
}
```

**Behavior**:
- Selection moves freely until it reaches **middle** of visible area
- Once past middle, scrolling begins
- At list boundaries, scroll stops but selection can continue to end
- Keeps selected item centered or near top of visible area

### Adding Scrolling to a Dialog

1. **Include CenteredScroll**:
   ```rust
   use super::super::scroll::CenteredScroll;
   ```

2. **Add to Dialog Struct**:
   ```rust
   pub struct MyDialog {
       pub scroll: CenteredScroll,
       pub visible_height: usize,
       // ... other fields
   }
   ```

3. **Initialize in Constructor**:
   ```rust
   pub fn new() -> Self {
       Self {
           scroll: CenteredScroll::new(),
           visible_height: 10, // default, updated at render time
           // ...
       }
   }
   ```

4. **Add Visible Height Setter**:
   ```rust
   pub fn set_visible_height(&mut self, height: usize) {
       self.visible_height = height;
   }
   ```

5. **Update Selection Methods**:
   Call `scroll.clamp()` in `select_up()` and `select_down()`:
   ```rust
   pub fn select_up(&mut self) {
       if self.selected > 0 {
           self.selected -= 1;
       }
       let scroll = self.scroll.get();
       let visible_items = self.count_visible_items(scroll);
       if self.selected < scroll || self.selected >= scroll + visible_items {
           self.scroll.clamp(self.selected, self.items.len(), visible_items);
       }
   }
   ```

6. **Update Render**:
   Skip items before `scroll` offset, stop after rendering `visible_items`:
   ```rust
   fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
       let scroll = self.scroll.get();
       let visible_items = self.count_visible_items(scroll);

       for (i, item) in self.items.iter().enumerate() {
           if i < scroll { continue; }
           if render_idx >= visible_items { break; }
           // ... render item ...
           render_idx += 1;
       }
   }
   ```

## Dialog Sizing Pattern

Dialogs should use contextually appropriate sizes based on content:

```rust
fn dialog_size(dialog: &Dialog) -> (u16, u16) {
    match dialog {
        Dialog::Model => (60, 70),      // Tall list of models with provider headers
        Dialog::Agent => (40, 40),       // List of agents (compact)
        Dialog::Session => (60, 70),     // List of sessions with metadata
        Dialog::Theme => (50, 50),       // Theme picker with preview
        Dialog::Tree => (50, 60),        // Tree visualization
        Dialog::Help => (80, 50),        // Help text (wide)
        Dialog::Keybind => (60, 40),     // Keybinding display
        Dialog::Mcp => (60, 50),         // MCP server list
        Dialog::Question => (40, 40),     // Question with answers
        Dialog::Permission => (50, 30),   // Permission prompt
        Dialog::Share => (50, 40),        // Share dialog
        Dialog::Import => (50, 60),       // Import preview
        Dialog::Command => (50, 60),      // Command palette
        Dialog::Template => (60, 40),     // Template picker
    }
}
```

Use with `centered_rect()` helper to center dialogs in the available space.

## Async Operations with TuiCommand

The TUI uses a command channel to handle async operations from synchronous event handlers. This avoids blocking the UI thread.

### TuiCommand Enum (src/tui/app/types.rs)

```rust
#[derive(Debug, Clone)]
pub enum TuiCommand {
    DeleteSession { session_id: String },
    ArchiveSession { session_id: String, unarchive: bool },
    ForkSession { session_id: String },
    ShareSession { session_id: String },
    BulkDelete { session_ids: Vec<String> },
    BulkArchive { session_ids: Vec<String>, unarchive: bool },
    BulkExport { session_ids: Vec<String> },
    ReloadSessions,
    OpenTreeDialog,
    PreviewImport { source: ImportSource },
    ConfirmImport { source: ImportSource },
    ListTasks,
    DeleteTask { id: String },
    CompactSession,
    OpenDiffDialog { old_content: String, new_content: String, title: String },
    SendNotification { notification_type: NotificationType, body: String },
}
```

### Sending Commands from Sync Handlers

```rust
// BAD - blocks the event handler thread
fn handle_fork_session(&mut self) {
    tokio::runtime::Handle::current().block_on(async move {
        store.fork(&session_id).await
    });
}

// GOOD - send command to async handler, return immediately
fn handle_fork_session(&mut self) {
    if let Some(ref tx) = self.tui_cmd_tx {
        let _ = tx.try_send(TuiCommand::ForkSession { session_id });
    }
}
```

### Handling Commands in Event Loop

In `src/tui/mod.rs` `run_event_loop`, async operations are performed:

```rust
Some(cmd) = cmd_rx.recv() => {
    match cmd {
        TuiCommand::ForkSession { session_id } => {
            if let Some(ref store) = app.session_store {
                if let Err(e) = store.fork(&session_id).await {
                    tracing::warn!("failed to fork session: {}", e);
                }
            }
            app.messages_state.toasts.info("Session forked");
            reload_sessions(app).await;
        }
        // ... other commands
    }
}
```

### Key Design Points

1. **Never use `block_on` in event handlers** - it blocks the UI thread
2. **Send commands and return immediately** - let the async handler do the work
3. **Commands carry all needed data** - clone data needed for async operation
4. **Handler updates UI state directly** - modifies app state after async operation completes
5. **Use `Arc::clone(store)`** - store is cloned before the async block to avoid borrow issues

## TuiMsg Enum (src/tui/app/types.rs)

The `TuiMsg` enum provides a centralized message type for UI intentions, enabling decoupled event handling. All dialogs emit explicit TuiMsg for user-visible effects:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TuiMsg {
    // Navigation & Submission
    SubmitPrompt,
    NavigateUp,
    NavigateDown,
    // Dialog Open/Close
    OpenModelDialog,
    OpenAgentDialog,
    OpenSessionDialog,
    OpenHelpDialog,
    OpenTreeDialog,
    OpenThemeDialog,
    OpenShareDialog,
    OpenImportDialog,
    OpenDiffDialog { old_content: String, new_content: String, title: String },
    CloseDialog,
    // Dialog-Specific Results (payload carries state)
    SelectModel { model: String },
    SelectAgent { agent_name: String },
    SelectSession { session_id: String },
    ConnectConfigured { provider_name: String, env_var: Option<String>, api_key: Option<String> },
    SelectTheme { theme_name: String },
    SubmitPermission { choice_index: usize },
    SubmitQuestionAnswers { answers_json: String },
    SelectTreeSession { session_id: String },
    ForkTreeSession { session_id: String },
    SubmitImportPreview,
    ConfirmImport,
    SelectTemplate { key: String },
    GotoMessage { index: usize },
    CopyShareUrl,
    McpAction { server_name: String, action: String },
    KeybindChanged { action: String, binding: String },
    // Input
    CharInput(char),
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    // ... and more
}
```

**Important**: TuiMsg carries payload for state synchronization. Dialogs should NOT read stale `dialog_state` - instead, the TuiMsg contains all necessary data.

## Debug Patterns

### File Logging

Since TUI captures terminal output, use file logging for debugging:

```rust
// In main.rs or dialog file
let _ = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open("debug.log")
    .and_then(|mut file| {
        writeln!(file, "[DEBUG] Selected item: {}", selected_index)
    });
```

### Conditional Debug Logging

Use compile-time feature flag:

```rust
#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("opencode_debug.log")
            .and_then(|mut file| {
                writeln!(file, format!($($arg)*))
            });
    };
}

#[cfg(not(feature = "debug-logging"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}
```

Enable with: `cargo run --features debug-logging`

## Multi-line Items (Themes, Models with Providers)

For dialogs where each item takes multiple lines (themes with color swatches, models with provider headers), the **scroll.clamp()** call must use the **number of visible items**, not lines.

### Example: Model Dialog with Provider Headers

For dialogs with collapsible provider headers (like `/models`), need to account for headers in visible count:

```rust
impl ModelDialog {
    fn count_visible_models(&self, start_idx: usize) -> usize {
        let mut lines_used = 0;
        let mut models_shown = 0;
        let flat = self.flat_filtered();

        for (i, _) in flat.iter().enumerate().skip(start_idx) {
            if i < start_idx { continue; }

            let last_provider = if i > 0 {
                let (prev_provider, _) = &flat[i - 1];
                Some(prev_provider.clone())
            } else { None };

            let (provider, _) = &flat[i];
            let is_new_provider = last_provider.as_ref() != Some(provider);
            let model_lines = 1 + if is_new_provider { 1 } else { 0 };

            if i == self.selected {
                if lines_used + model_lines > self.visible_height { break; }
                lines_used += model_lines + 1;
            } else {
                if lines_used + model_lines > self.visible_height { break; }
                lines_used += model_lines;
            }

            models_shown += 1;
        }

        models_shown
    }
}
```

**Key Points**:
- Provider headers take 1 line, models take 1 line
- `rendered_lines` tracks what each rendered line is (header vs model)
- Only count model lines (where `is_model = true`) for scroll calculation
- Ensure selected item is always within visible range

## Working with Provider-Grouped Models

For dialogs like `/models` that group items by provider with collapsible headers:

```rust
impl ModelDialog {
    pub struct ModelDialog {
        expanded_providers: HashSet<String>,
    }

    pub fn toggle_provider(&mut self, provider: &str) {
        let provider_str = provider.to_string();
        if self.expanded_providers.contains(&provider_str) {
            self.expanded_providers.remove(&provider_str);
        } else {
            self.expanded_providers.insert(provider_str);
        }
    }

    fn flat_filtered(&self) -> Vec<(String, String)> {
        let groups = self.get_grouped_models();
        let mut result = Vec::new();
        for (provider, models) in groups {
            for model in models {
                if !self.expanded_providers.contains(&provider) {
                    continue;
                }
                let name = model.split('/').next_back().unwrap_or(model).to_string();
                result.push((provider.clone(), name));
            }
        }
        result
    }
}
```

**Consider**: For simple provider-grouped lists, avoid collapsible feature unless user explicitly wants it. It adds complexity for marginal benefit.

## GlobalEventBus Integration

The TUI subscribes to `GlobalEventBus` for receiving events from AgentLoop and other components.

### Event Types (src/bus/events.rs)

Key events handled in TUI:

```rust
// Streaming events
TextDelta { session_id: Arc<str>, delta: Arc<str> },
ReasoningDelta { session_id: Arc<str>, delta: String },

// Tool events
ToolCallStarted { session_id: String, tool_name: String, tool_id: String, arguments: String },
ToolResult { tool_id: String, tool_name: String, session_id: String, output: String, success: bool },

// Agent lifecycle
AgentFinished { session_id: String, stop_reason: String },

// Permission/Question events
PermissionPending { session_id: String, perm_id: String, tool: String, path: Option<String>, args: Option<serde_json::Value> },
QuestionPending { session_id: String, questions: String },
```

### TUI Event Loop Pattern

```rust
pub async fn run_event_loop(app: &mut App) -> Result<(), AppError> {
    let mut bus_rx = GlobalEventBus::subscribe();

    tokio::select! {
        biased;
        Some(result) = reader.next() => { /* keyboard/mouse */ }
        Ok(event) = bus_rx.recv() => {
            match event {
                AppEvent::TextDelta { delta, .. } => {
                    app.messages_state.messages.add_assistant_text(delta);
                }
                AppEvent::ToolCallStarted { tool_name, tool_id, arguments, .. } => {
                    app.messages_state.messages.add_tool_call(tool_id, tool_name, arguments);
                }
                AppEvent::AgentFinished { .. } => {
                    app.session_state.session_status = SessionStatus::Idle;
                }
                AppEvent::PermissionPending { perm_id, tool, path, args } => {
                    app.show_permission_dialog(perm_id, PermissionRequest { tool, path, args });
                }
                AppEvent::QuestionPending { session_id, questions } => {
                    let questions: Vec<QuestionSpec> = serde_json::from_str(&questions).unwrap();
                    app.show_question_dialog(questions, session_id);
                }
                // ... handle other events
            }
        }
    }
}
```

## SpinnerWidget (Busy Spinner)

The `SpinnerWidget` in `src/tui/components/spinner.rs` provides an animated busy spinner for the TUI.

### Features
- Uses `Cell` for interior mutability (can be used in `&self` contexts)
- Animated frames: `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]`
- Configurable speed (default 80ms per frame)
- Start/stop functionality
- Optional text label

### Usage in App

The `App` struct has `busy_spinner: SpinnerWidget` field (initialized in `new()` and `new_for_testing()`).

**In event loop** (`src/tui/mod.rs`):
```rust
// Tick the spinner (call before render)
self.busy_spinner.tick();

// Render with frame
let spinner_text = self.busy_spinner.frame();
// Use spinner_text in UI (e.g., in footer or status line)
```

**Controlling the spinner**:
```rust
// Start when session becomes active
self.session_state.session_status = SessionStatus::Working;
self.busy_spinner.start(None);  // Optional: set label

// Stop when session completes or errors
self.busy_spinner.stop();
```

### Public API

```rust
impl SpinnerWidget {
    pub fn new() -> Self;
    pub fn with_speed(self, speed_ms: u64) -> Self;
    pub fn with_color(self, color: Color) -> Self;
    pub fn with_label(self, label: String) -> Self;
    pub fn start(&mut self, label: Option<String>);
    pub fn stop(&mut self);
    pub fn tick(&self);      // Advances to next frame (call in render loop)
    pub fn frame(&self) -> String;  // Gets current frame as styled String
    pub fn is_active(&self) -> bool;
}
```

### Integration Points

- **Session Status**: Spinner starts when `SessionStatus::Working`, stops on `Idle` or `Error`
- **Footer Display**: Shown in footer area when session is active
- **Event Loop**: `tick()` called every render frame (~60fps)

## Related Skills

- See `.opencode/skills/event-bus/SKILL.md` for GlobalEventBus and AppEvent documentation
- See `.opencode/skills/agent-loop/SKILL.md` for AgentLoop event publishing
