# TUI Dialog Testing Skill

## When to Use

Use this skill when:
- Adding tests for TUI dialogs (ModelDialog, SessionDialog, ConnectDialog, etc.)
- Testing Component trait implementations
- Verifying FocusManager dialog handling
- Testing mouse click behavior (hit_test)

## Key Patterns

### 1. Unit Tests for Dialog Components

Location: `src/tui/components/dialogs/*.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::components::component::Component;
    use std::sync::Arc;
    use crate::tui::theme::Theme;

    #[test]
    fn test_dialog_behavior() {
        let theme = Arc::new(Theme::dark());
        let mut dialog = ModelDialog::new(theme);
        
        // Test initialization
        assert_eq!(dialog.tab, ModelDialogTab::SelectModel);
        
        // Test key handling
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE
        );
        dialog.handle_key(key);
        assert_eq!(dialog.tab, ModelDialogTab::Configure);
    }
}
```

### 2. Integration Tests for App-level Dialog Handling

Location: `tests/tui.rs`

```rust
#[test]
fn test_app_dialog_flow() {
    use opencode_rs::tui::app::App;
    use opencode_rs::tui::app::TuiMsg;
    
    let mut app = App::new_for_testing("/tmp/test".to_string());
    
    // Test dialog state sync
    let models = vec!["model1".to_string(), "model2".to_string()];
    app.set_models(models.clone());
    
    assert_eq!(app.dialog_state.model_dialog.models, models);
}
```

### 3. Mouse Click Tests

```rust
#[test]
fn test_mouse_click_selects_item() {
    use opencode_rs::tui::components::dialogs::model::ModelDialog;
    use opencode_rs::tui::components::component::Component;
    use opencode_rs::tui::components::dialogs::model::ModelDialogTab;
    
    let theme = Arc::new(opencode_rs::tui::theme::Theme::dark());
    let mut dialog = ModelDialog::new(theme);
    dialog.tab = ModelDialogTab::SelectModel;
    dialog.models = vec!["openai/gpt4".to_string()];
    dialog.set_visible_height(20);
    
    // Simulate click on model row
    let result = dialog.hit_test(3);  // Usually row 3 = first model
    assert!(result.is_some());
    
    if let Some(idx) = result {
        dialog.set_selected(idx);
        assert_eq!(dialog.selected, idx);
    }
}
```

## Testing Checklist

For each dialog component:

- [ ] Unit tests for `handle_key()` with all key types
- [ ] Unit tests for `handle_paste()` with various inputs
- [ ] Unit tests for `hit_test()` with valid/invalid rows
- [ ] Integration tests for App-level state synchronization
- [ ] Tests for `visible_height` calculation
- [ ] Tests for footer text matching implemented keys
- [ ] Tests for empty state messages ("no models" vs "no matches")

## Common Pitfalls

1. **Private methods**: Use public methods in tests (`set_visible_height()` not `visible_height =`)
2. **Widget import**: Don't remove `Widget` trait import - `Paragraph::render()` needs it
3. **FocusManager**: Remember dialogs are cloned when pushed - test both `dialog_state` and focused clone
4. **Mouse coordinates**: `hit_test()` uses relative Y (0 = top of dialog area, including borders)

## References

- `src/tui/components/component.rs` - Component trait definition
- `src/tui/components/dialogs/model.rs` - ModelDialog implementation
- `src/tui/app/mod.rs` - App-level dialog handling, `close_dialog()`
- `tests/tui.rs` - Integration tests
