# TUI Module Override

This file contains TUI-specific guidance and overrides root AGENTS.md.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Enter` | Send prompt |
| `Shift+Enter` | New line in prompt |
| `Tab` | Switch agent |
| `Shift+Tab` | Toggle Permission Mode (Plan/Build) |
| `Ctrl+K` | Clear session |
| `Ctrl+N` | New session |
| `Ctrl+T` | Toggle sidebar |
| `Ctrl+W` | Close session |
| `Ctrl+L` | Select model |
| `Ctrl+Y` | Toggle TTS |
| `Ctrl+S` | Stash prompt |
| `Ctrl+R` | Restore prompt |
| `Ctrl+P` | Cycle model forward |
| `Ctrl+Shift+P` | Cycle model backward |
| `Ctrl+F` | Search / Find |
| `Ctrl+E` | Open in external editor |
| `Ctrl+Q` | Quit |
| `Esc` | Close dialog / Cancel |
| `?` | Help overlay |
| `d` | Open diff dialog |
| `i` | Toggle image visibility (when image support enabled) |

## Input Handling (Updated 2026-05-02)

### Shifted Printable Characters

Shifted printable characters (Shift+A, Shift+1, Shift+!, etc.) are now properly handled in insert mode:

- `handle_key_with_bindings()` checks for exact binding matches first
- If no binding exists and the key is a printable `KeyCode::Char(c)` with `NONE` or `SHIFT` modifiers, it returns `InputAction::Char(c)`
- Custom bindings for shift+char combinations take precedence over character insertion
- Helper functions: `is_printable_char()`, `is_text_modifier()`

### Model Dialog Key Handling (Updated 2026-05-02)

- `KeyCode::Tab` switches between SelectModel/Configure tabs
- `Shift+Tab` moves backward between add-model fields (when adding model)
- `a` adds custom model (Configure tab): starts visible input form for name, provider, API key, base URL
- `d` deletes selected custom model (Configure tab, only shown when custom models exist)
- `Esc` cancels add-model mode before closing dialog
- Enter saves custom model when on last input field (base URL), switches to SelectModel tab
- Multi-char input for custom model fields (name, provider, api_key, base_url) with Tab/Shift+Tab to navigate fields
- `j`/`k` vim-style navigation now shown in footer
- `Backspace` for filter editing now shown in footer
- Configure tab footer shows 'a add custom model' and 'd delete' only when custom models exist

### FocusManager Dialog Sync (Updated 2026-05-02)

- `close_dialog()` now pops focused component, updates `ui_state.dialog` to new top dialog type
- `DialogType` has `From<Dialog>` conversion for proper dialog state sync
- Nested dialogs close correctly without leaving stale `ui_state.dialog`
- `TuiMsg::SelectSession { session }` now carries full `Session` object (not just ID)
- `TuiMsg::SelectTemplate { key, template }` now carries full `SessionTemplate`
- `process_msg()` uses payload directly instead of reading stale `dialog_state`

### Connect Dialog (Updated 2026-05-02)

- `open_dialog(Dialog::Connect)` now calls `open_connect_dialog()` for shared provider list
- Duplicate provider list removed (was 5 providers in fallback, now uses 14 providers from `open_connect_dialog()`)
- Padding expression fixed to `40usize.saturating_sub(display_key.len())` to prevent underflow
- Provider list builder centralized in `open_connect_dialog()`

### Paste Routing

1. Event loop in `src/tui/mod.rs` routes `Event::Paste` to `focus_manager.handle_paste()` first
2. `Component` trait has a new `handle_paste(&mut self, text: String) -> Option<TuiMsg>` method
3. Dialogs that accept text input implement `handle_paste()`:
   - `SessionDialog` - pastes into `filter` field
   - `ConnectDialog` - pastes into `api_key_input` when in `EnterApiKey` step
   - `ModelDialog` - pastes into filter (SelectModel tab) or active add-model field (Configure tab)
   - `ImportDialog` - pastes into `input` field
   - `GotoDialog` - pastes into `input` field
   - `TemplateDialog` - pastes into `filter` field, updates cache
4. `App::on_paste()` handles remaining cases:
   - Command mode: pastes into prompt and updates command palette query
   - No dialog open: pastes into prompt and updates completions via `paste_into_prompt()`
   - Dialog open but not handled by FocusManager: ignored (prevents pasting into hidden prompt)

### Prompt Completions

Paste now updates prompt completions:

- `paste_into_prompt(&mut self, text: String)` helper calls `prompt.paste()` then `update_completions()`
- Slash completions: paste `/model` triggers completion display
- File completions: paste `@src/tui` triggers file/agent completion
- Completion state (`completion_filter`, `completion_type`, `completion_sel`) updates correctly

## Mouse Handling (Updated 2026-05-02)

### Dialog Mouse Click Selection

Mouse dialog item selection now delegates hit testing to the active `Component` via `FocusManager`:

- `Component` trait has `hit_test(&self, rel_y: usize) -> Option<usize>` method
- `ModelDialog` implements `hit_test_model_row()` that accounts for:
  - Tab line (row 0)
  - Blank line (row 1)
  - Optional filter lines (filter + blank line)
  - Provider header lines
  - Scroll offset
  - Current visible height
- `select_dialog_item()` in `app/mod.rs` now:
  1. Gets the index from `focus_manager.top().hit_test(rel_y)`
  2. Updates `dialog_state.*` for legacy code paths
  3. Syncs selection to focused clone via `focus_manager.top_mut().set_selected(idx)`

### Component Trait Additions

- `hit_test(&self, rel_y: usize) -> Option<usize>`: Returns item index for mouse click at given row (dialog-relative, including borders; rel_y=0 is top border)
- `set_selected(&mut self, idx: usize)`: Sets selected item index (for state sync)
- `ModelDialog::hit_test()` subtracts 1 from rel_y to account for top border, delegates to `hit_test_model_row()` with content-relative coordinates

### Mouse Click Flow

1. User clicks on dialog
2. `handle_mouse_click()` maps terminal coordinates to dialog-relative `(rel_x, rel_y)`
3. `select_dialog_item(rel_x, rel_y, area)` is called
4. `focus_manager.top().hit_test(rel_y)` maps row to item index
5. State is synced to both `dialog_state` and focused clone
6. Dialog processes the selection (e.g., `TuiMsg::SelectModel`)

## Testing

Test files:
- `src/tui/input.rs` - Input mapping tests (shifted chars, modifiers)
- `tests/tui.rs` - App-level tests, dialog tests, widget tests
- `App::new_for_testing()` - Creates minimal App without background tasks
- `tests/e2e.rs` - E2E test infrastructure (ratatui-testlib, currently non-functional)
- `tests/minimal_e2e.rs` - Minimal E2E test examples

Key test cases:
- Shift+A inserts 'A' in prompt
- Shift+punctuation inserts correct characters
- Paste updates completions correctly
- Dialog `handle_paste()` implementations work correctly

### E2E Testing Notes
- E2E tests for TUI require PTY infrastructure (ratatui-testlib v0.1.0 added as dev-dependency)
- `ratatui-testlib` PTY harness currently non-functional on this system (tests hang on spawn)
- Non-interactive mode via `--run` flag or `exec` subcommand is preferred for CI/CD testing
- Example non-interactive test: `opencode-rs --run "prompt" --output-format text`

## SpinnerWidget (Busy Spinner) - Added 2026-05-02

The `SpinnerWidget` in `src/tui/components/spinner.rs` provides an animated busy spinner for the TUI:

### Features
- Uses `Cell` for interior mutability (can be used in `&self` contexts)
- Animated frames: `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]`
- Configurable speed (default 80ms per frame)
- Start/stop functionality

### Usage in App
- `App` has `busy_spinner: SpinnerWidget` field
- Updated in event loop: `self.busy_spinner.tick()` and `self.busy_spinner.frame()`
- Started when session status becomes "Working"
- Stopped when session completes or errors

### Public API
- `new()` - Create new spinner with default frames
- `with_speed(ms)` - Set animation speed
- `with_color(color)` - Set spinner color
- `with_label(text)` - Set text label shown next to spinner
- `start(label)` - Start animation (optionally set label)
- `stop()` - Stop animation
- `tick()` - Advance to next frame (call in render loop)
- `frame()` - Get current frame as styled `String`
- `is_active()` - Check if spinner is running
