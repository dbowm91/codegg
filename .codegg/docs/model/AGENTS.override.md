# Model Dialog - Agent Guidance

## Overview

The `ModelDialog` in `src/tui/components/dialogs/model.rs` provides the UI for selecting and configuring AI models.

## Key Concepts

### hit_test() Coordinate Contract

**Important**: The `Component::hit_test()` method uses **dialog-relative coordinates**:
- `rel_y = 0` corresponds to the **top border** of the dialog
- Content (tab line, model rows, footer) starts at `rel_y = 1`

When implementing or testing `hit_test()`:
1. Dialog-relative: `rel_y = 0` → top border
2. Content-relative: `rel_y = 0` → tab line ("[ Select Model ] | [ Configure ]")
3. `ModelDialog::hit_test()` subtracts 1 (top border) before calling `hit_test_model_row()`

### hit_test_model_row() Expectations

- Expects **content-relative** coordinates (excluding borders)
- Row 0 = tab line
- Row 1 = blank line
- Optional filter lines follow (if filter is not empty)
- Then model rows with provider headers

### count_visible_models() Behavior

The `count_visible_models()` function:
1. Computes budget using `model_row_budget()` (subtracts non-model rows)
2. Handles small heights by showing model without header when there's not enough room
3. Uses `CenteredScroll` for scroll management

### Model Row Budget

`model_row_budget()` computes available rows for model entries:
- Starts with `visible_height` (area.height - 2 for borders)
- Subtracts: tab line + blank spacer (2 rows)
- Subtracts: filter line + spacer if filter is not empty (2 rows)
- Subtracts: spacer before footer + footer line (2 rows)

## Testing

### App-Level Mouse Click Tests

When testing mouse clicks, use the **full coordinate path**:
```rust
#[test]
fn test_app_mouse_click() {
    use crossterm::event::{MouseEvent, MouseEventKind, MouseButton};
    use opencode_rs::tui::app::App;
    use opencode_rs::tui::Dialog;
    use ratatui::layout::Rect;

    let mut app = App::new_for_testing("/tmp/test".to_string());
    app.set_models(models.clone());
    app.open_dialog(Dialog::Model);
    app.dialog_area = Some(Rect::new(0, 0, 60, 20));

    // Click at dialog-relative row (matches real mouse events)
    let mouse_event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 4,  // dialog-relative
        modifiers: crossterm::event::KeyModifiers::empty(),
    };
    app.on_mouse(mouse_event);
    assert_eq!(app.dialog_state.model_dialog.selected, 0);
}
```

### Small Height Tests

When testing small popup heights:
1. Set `visible_height` (not `area.height`)
2. Call `update_cache()` after setting models
3. Verify budget with `model_row_budget()`
4. Check `count_visible_models()` returns correct values

```rust
#[test]
fn test_small_height() {
    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    dialog.models = vec!["openai/gpt4".to_string()];
    dialog.set_visible_height(5);  // area.height = 7
    dialog.update_cache();

    // budget = 5 - 2(tab+blank) - 2(spacer+footer) = 1
    assert_eq!(dialog.model_row_budget(), 1);

    // With budget=1, can show 1 model (without header)
    let visible = dialog.count_visible_models(0);
    assert!(visible >= 1);
}
```

## Public Methods for Testing

These methods are `pub` for testing purposes:
- `model_row_budget()` - compute available rows for models
- `count_visible_models(start_idx)` - count visible models
- `update_cache()` - update the filtered model cache
- `flat_filtered()` - get the filtered flat list of (provider, model) pairs
