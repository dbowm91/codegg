# Skill: TUI Dialog Maintenance

## Purpose
Guide for maintaining TUI dialogs in opencode-rs, including key handling, state synchronization, and FocusManager integration.

## Key Concepts

### Dialog State Synchronization
- Always sync `dialog_state.<dialog>.current` with `agent_state` when state changes (e.g., `set_models()`, `cycle_model_forward()`)
- Use `ModelDialog::set_current()` and `ModelDialog::set_models()` to update dialog state
- FocusManager cloned dialogs must carry payloads in `TuiMsg` to avoid stale `dialog_state` reads

### Core-Backed Dialogs
- Session tree, share, import, export, archive/delete, and history-related dialogs should prefer `CoreClient` requests instead of direct `SessionStore` or `MessageStore` access
- `TreeDialog::load_nodes()` is the preferred way to load a prebuilt tree from core responses
- If a dialog action already has a matching `CoreRequest`, wire the dialog to that request first and keep the UI layer thin

### Key Handling
- `Component::handle_key()` should handle `KeyCode::Tab` directly (not as `Char('\t')`)
- Separate key handling per dialog tab (e.g., SelectModel vs Configure tab in ModelDialog)
- Add mode-specific help text in footer (e.g., `Tab/Enter next field` when adding custom model)

### Custom Model Input
- Use `field_index` to track active input field (0=name, 1=provider, 2=api_key, 3=base_url)
- `handle_add_model_input(c)` appends to active field based on `field_index`
- `next_add_model_field()` cycles through fields with Tab/Enter

### FocusManager Integration
- `close_dialog()` pops focused component, updates `ui_state.dialog` to new top dialog type
- Use `DialogType` to `Dialog` conversion (`From<DialogType> for Dialog`) for proper state sync
- Nested dialogs close correctly without leaving stale dialog state

### Connect Dialog
- All entry points (`open_dialog(Dialog::Connect)`, `/connect` slash command) must use shared provider list
- Padding expression: `40usize.saturating_sub(display_key.len())` to prevent underflow
- Provider list: openai, anthropic, google, ollama, openrouter

## Testing
- Add unit tests for `ModelDialog::handle_key()` (Tab, Enter, Esc, filter, paste)
- Test `close_dialog()` nested dialog behavior
- Test paste behavior per dialog tab (SelectModel filter, Configure add-model fields)
- Run `cargo test --lib -- model::tests` and `cargo test --test tui` for verification
