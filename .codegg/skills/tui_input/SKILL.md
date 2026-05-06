# Skill: TUI Input Handling

## Overview

This skill covers TUI input handling in opencode-rs, including:
- Shifted printable character handling
- Paste routing through FocusManager
- Component trait's `handle_paste()` method
- Prompt widget input and completions

## Key Concepts

### 1. Shifted Printable Characters

In `src/tui/input.rs`, the `handle_key_with_bindings()` function now properly handles shifted printable characters:

```rust
fn is_printable_char(c: char) -> bool {
    !c.is_control()
}

fn is_text_modifier(modifiers: KeyModifiers) -> bool {
    modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT
}

fn handle_key_with_bindings(...) -> Option<InputAction> {
    match mode {
        InputMode::Insert => {
            // Check for exact binding matches first
            if let Some(action) = map.get(&key_tuple) {
                return Some(action.clone());
            }
            // No binding found - check if printable char with NONE or SHIFT
            if let KeyCode::Char(c) = key.code {
                if is_printable_char(c) && is_text_modifier(key.modifiers) {
                    return Some(InputAction::Char(c));
                }
            }
            None
        }
        // ...
    }
}
```

**Key Points**:
- Exact bindings are checked FIRST (so Ctrl+Shift+P still works)
- If no binding, shifted printable chars (Shift+A, Shift+1, Shift+!) insert the character
- Custom bindings for shift+char take precedence over character insertion

### 2. Paste Routing Through FocusManager

Paste events (`Event::Paste`) are now routed through the FocusManager:

**Event Loop** (`src/tui/mod.rs`):
```rust
if let Event::Paste(text) = &event {
    if let Some(msg) = app.focus_manager.handle_paste(text.clone()) {
        app.process_msg(msg);
    } else {
        app.on_paste(text.clone());
    }
    continue;
}
```

**FocusManager** (`src/tui/components/component/focus.rs`):
```rust
pub fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
    if let Some(top) = self.stack.back_mut() {
        if let Some(msg) = top.handle_paste(text) {
            return Some(msg);
        }
    }
    None
}
```

### 3. Component Trait's `handle_paste()` Method

The `Component` trait (`src/tui/components/component.rs`) now includes:

```rust
pub trait Component: Send {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    
    fn handle_paste(&mut self, _text: String) -> Option<TuiMsg> {
        None  // Default: not handled
    }
    
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool {
        self.dialog_type().is_modal()
    }
}
```

**Implementing `handle_paste()` for Dialogs**:

```rust
// Example: SessionDialog
impl Component for SessionDialog {
    // ... other methods ...
    
    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.filter.push_str(&text);
        None  // No TuiMsg to send
    }
}
```

### 4. Dialogs with `handle_paste()` Implementation

| Dialog | Field Updated | Notes |
|-------|---------------|-------|
| `SessionDialog` | `filter` | Appends to filter for session search |
| `ConnectDialog` | `api_key_input` | Only in `EnterApiKey` step |
| `ImportDialog` | `input` | Import source URL/path |
| `GotoDialog` | `input` | Goto line number |
| `ModelDialog` | `filter` | Also calls `update_cache()` |
| `TemplateDialog` | `filter` | Also calls `update_cache()` |

### 5. App::on_paste() Method

Located in `src/tui/app/mod.rs`:

```rust
pub fn on_paste(&mut self, text: String) {
    if self.ui_state.command_mode {
        self.prompt_state.prompt.paste(text);
        let query = self.prompt_state.prompt.get_text();
        self.dialog_state.command_palette.set_query(&query);
    } else if !self.ui_state.dialog.is_open() {
        self.paste_into_prompt(text);
    }
    // If dialog is open but FocusManager didn't handle paste,
    // don't paste into prompt behind the dialog
}

/// Paste text into prompt and update completions
fn paste_into_prompt(&mut self, text: String) {
    self.prompt_state.prompt.paste(text);
    self.update_completions();
}
```

**Key Behavior**:
- **Command mode**: Pastes into command palette query
- **Dialog open**: FocusManager handles it (if dialog implements `handle_paste()`)
- **No dialog open**: Pastes into prompt and updates completions
- **Dialog open but not handled**: Text is NOT pasted into hidden prompt

### 6. Prompt Widget Input

Located in `src/tui/components/prompt.rs`:

```rust
pub struct PromptWidget {
    pub text: String,
    pub cursor: usize,
    // ... other fields
}

impl PromptWidget {
    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }
    
    pub fn paste(&mut self, text: String) {
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
    
    pub fn backspace(&mut self) { /* ... */ }
    pub fn delete(&mut self) { /* ... */ }
    pub fn cursor_left(&mut self) { /* ... */ }
    pub fn cursor_right(&mut self) { /* ... */ }
}
```

### 7. Completions Update After Paste

When text is pasted into the prompt (not in command mode), completions are updated:

```rust
fn update_completions(&mut self) {
    let text = self.prompt_state.prompt.get_text();
    let cursor = self.prompt_state.prompt.cursor_pos();
    let before_cursor = &text[..cursor];
    
    // Check for '/' trigger
    if let Some(pos) = before_cursor.rfind('/') {
        if pos == 0 || before_cursor.chars().nth(pos.saturating_sub(1)) == Some(' ') {
            self.prompt_state.completion_filter = before_cursor[pos..].to_string();
            self.prompt_state.completion_type = CompletionType::Slash;
            self.prompt_state.show_completions = true;
            return;
        }
    }
    
    // Check for '@' trigger
    if let Some(pos) = before_cursor.rfind('@') {
        // ... similar logic for file/agent completions
    }
    
    self.prompt_state.show_completions = false;
}
```

## Testing TUI Input

### Unit Tests in `src/tui/input.rs`

```rust
#[test]
fn test_shift_char_insert() {
    let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
    let result = handle_key_with_bindings(key, None, InputMode::Insert);
    assert_eq!(result, Some(InputAction::Char('A')));
}
```

### App-Level Tests in `tests/tui.rs`

```rust
#[test]
fn test_app_shift_char_inserts_text() {
    let mut app = App::new_for_testing("/tmp/test".to_string());
    let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
    app.on_key(key);
    assert_eq!(app.prompt_state.prompt.get_text(), "A");
}

#[test]
fn test_app_paste_updates_completions_slash() {
    let mut app = App::new_for_testing("/tmp/test".to_string());
    app.on_paste("/model".to_string());
    assert!(app.prompt_state.show_completions);
    assert_eq!(app.prompt_state.completion_type, CompletionType::Slash);
}
```

### Dialog handle_paste() Tests

```rust
#[test]
fn test_session_dialog_handle_paste() {
    use opencode_rs::tui::components::component::Component;
    let theme = Arc::new(Theme::dark());
    let mut dialog = SessionDialog::new(theme);
    
    let msg = dialog.handle_paste("test_filter".to_string());
    assert!(msg.is_none());
    assert_eq!(dialog.filter, "test_filter");
}
```

## Adding handle_paste() to New Dialogs

When creating a new dialog that accepts text input:

1. **Add field for text input** (e.g., `pub input: String`)

2. **Implement Component trait** and include `handle_paste()`:

```rust
impl Component for MyDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // ... handle key events
    }
    
    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.input.push_str(&text);
        None  // Return None = handled, no TuiMsg to send
    }
    
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        // ... handle messages
    }
    
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        // ... render dialog
    }
    
    fn dialog_type(&self) -> DialogType {
        DialogType::MyDialog
    }
}
```

3. **Update cache if needed** (for dialogs with filtered lists):

```rust
fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
    self.filter.push_str(&text);
    self.update_cache();  // Invalidate cache for filtered dialogs
    None
}
```

## Common Patterns

### Check if Dialog is in FocusManager Stack

```rust
// In App::on_key()
if !self.focus_manager.is_empty() {
    if let Some(msg) = self.focus_manager.handle_key(key) {
        self.process_msg(msg);
    }
    return;
}
```

### Paste Only When Dialog is in Correct State

```rust
impl Component for ConnectDialog {
    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        if self.step == ConnectStep::EnterApiKey {
            self.api_key_input.push_str(&text);
            self.cursor_pos = self.api_key_input.len();
        }
        None
    }
}
```

## Troubleshooting

### Shift+Char Not Inserting

1. Check `handle_key_with_bindings()` in `src/tui/input.rs`
2. Verify `is_text_modifier()` allows SHIFT modifier
3. Ensure no custom binding conflicts with shift+char

### Paste Not Working in Dialog

1. Verify dialog implements `handle_paste()`
2. Check if dialog is in FocusManager stack (add with `focus_manager.push(Box::new(dialog))`)
3. Debug by logging in `FocusManager::handle_paste()`

### Completions Not Updating After Paste

1. Check `paste_into_prompt()` calls `update_completions()`
2. Verify `update_completions()` detects the correct trigger (`/`, `@`)
3. For file completions, ensure `update_file_completions()` has data in `indexed_files`
