# Model Dialog Skill

Guide for working with the Model Dialog in opencode-rs TUI.

## Overview

The Model Dialog allows users to select, configure, and add custom models. It has two tabs:
- **Select Model**: Browse/Filter available models, select a model to use
- **Configure**: View provider status, add custom models, delete custom models

## Key Files

- `src/tui/components/dialogs/model.rs`: Main dialog implementation
- `src/tui/components/component.rs`: Component trait with `hit_test()` contract
- `.opencode/docs/tui/AGENTS.override.md`: TUI keyboard shortcuts and mouse handling docs

## Configure Tab Behavior

### Adding Custom Models
1. Press `a` in Configure tab to start add mode
2. Input fields for Name, Provider, API Key, Base URL appear with active field highlighted
3. Use `Tab` to move forward, `Shift+Tab` to move backward between fields
4. Type to enter text, `Backspace` to delete, paste works in active field
5. Press `Enter` on the last field (Base URL) to save the custom model
6. Custom model is added to the Select Model list automatically

### Deleting Custom Models
- `d` deletes the selected custom model (only shown when custom models exist)
- Custom models are listed below provider configs in Configure tab

## Mouse Handling

- `Component::hit_test()` uses **dialog-relative coordinates** (including borders)
- `rel_y=0` corresponds to the top border of the dialog
- `ModelDialog::hit_test()` subtracts 1 (top border) before delegating to `hit_test_model_row()`
- Real mouse clicks use the same coordinate path via `App::select_dialog_item()`

## Dialog Height Budget

- `ModelDialog::model_row_budget()` computes available rows for model entries
- Subtracts non-model rows: tab line, blank spacer, optional filter lines, footer spacer, footer line
- `count_visible_models()` and scroll clamping use this budget to prevent footer clipping

## Testing

- Run `cargo test --lib model_dialog` for unit tests
- Run `cargo test --test tui` for integration tests
- Key test cases:
  - `test_configure_tab_a_starts_add_mode`: Verify 'a' starts add mode
  - `test_enter_on_last_field_saves_custom_model`: Verify Enter saves custom model
  - `test_hit_test_uses_dialog_relative_coordinates`: Verify hit_test uses dialog-relative coords
  - `test_small_height_shows_footer`: Verify footer not clipped in small terminals
