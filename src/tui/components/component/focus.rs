//! FocusManager - manages modal focus stack
//!
//! The FocusManager maintains a stack of Components, with the top component
//! receiving key events first. If unhandled, events bubble to underlying components.

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::Frame;
use std::collections::VecDeque;
use std::sync::Arc;

pub struct FocusManager {
    stack: VecDeque<Box<dyn Component>>,
    focus_index: usize,
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            stack: VecDeque::new(),
            focus_index: 0,
        }
    }

    pub fn push(&mut self, component: Box<dyn Component>) {
        self.stack.push_back(component);
    }

    pub fn pop(&mut self) -> Option<Box<dyn Component>> {
        self.stack.pop_back()
    }

    pub fn pop_dialog(&mut self, dialog_type: DialogType) -> Option<Box<dyn Component>> {
        let pos = self
            .stack
            .iter()
            .position(|c| c.dialog_type() == dialog_type);
        if let Some(idx) = pos {
            if idx <= self.focus_index && self.focus_index > 0 {
                self.focus_index -= 1;
            }
            return self.stack.remove(idx);
        }
        None
    }

    /// Replace the topmost stack entry whose `dialog_type()` matches the
    /// given type with a new component. Used when the caller mutates a
    /// dialog stored separately (e.g. `dialog_state.info_dialog`) and
    /// needs the rendered component in the focus stack to reflect the
    /// new content. Returns `true` when a replacement was made.
    pub fn replace_top_dialog(
        &mut self,
        dialog_type: DialogType,
        component: Box<dyn Component>,
    ) -> bool {
        let pos = self
            .stack
            .iter()
            .rposition(|c| c.dialog_type() == dialog_type);
        if let Some(idx) = pos {
            self.stack[idx] = component;
            return true;
        }
        false
    }

    pub fn top(&self) -> Option<&dyn Component> {
        self.stack.back().map(|v| &**v)
    }

    pub fn top_mut(&mut self) -> Option<&mut Box<dyn Component>> {
        self.stack.back_mut()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        if key.code == crossterm::event::KeyCode::Tab {
            return self.handle_tab(
                key.modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT),
            );
        }
        if let Some(top) = self.stack.back_mut() {
            if let Some(msg) = top.handle_key(key) {
                return Some(msg);
            }
        }
        None
    }

    fn handle_tab(&mut self, reverse: bool) -> Option<TuiMsg> {
        if let Some(top) = self.stack.back_mut() {
            let count = top.focusable_count();
            if count > 0 {
                if reverse {
                    self.focus_index = self.focus_index.saturating_sub(1);
                } else {
                    self.focus_index = (self.focus_index + 1) % count;
                }
                top.set_focused(self.focus_index);
            }
        }
        None
    }

    pub fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        if let Some(top) = self.stack.back_mut() {
            if let Some(msg) = top.handle_paste(text) {
                return Some(msg);
            }
        }
        None
    }

    pub fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        if let Some(top) = self.stack.back_mut() {
            if let Some(response) = top.update(msg) {
                return Some(response);
            }
        }
        None
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        if let Some(top) = self.stack.back_mut() {
            top.render(frame, area, theme);
        }
    }

    pub fn active_dialog_type(&self) -> DialogType {
        self.stack
            .back()
            .map(|c| c.dialog_type())
            .unwrap_or(DialogType::None)
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    struct StubComponent {
        dialog_type: DialogType,
    }

    impl Component for StubComponent {
        fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
            None
        }

        fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
            None
        }

        fn render(&mut self, _frame: &mut Frame, _area: Rect, _theme: &Arc<Theme>) {}

        fn dialog_type(&self) -> DialogType {
            self.dialog_type.clone()
        }

        fn focusable_count(&self) -> usize {
            3
        }
    }

    fn stub(dialog_type: DialogType) -> Box<dyn Component> {
        Box::new(StubComponent { dialog_type })
    }

    #[test]
    fn pop_dialog_removes_match_and_preserves_valid_focus_index() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.push(stub(DialogType::Theme));
        focus.push(stub(DialogType::Model));
        focus.focus_index = 2;

        let removed = focus.pop_dialog(DialogType::Theme);

        assert!(removed.is_some());
        assert_eq!(focus.len(), 2);
        assert_eq!(focus.active_dialog_type(), DialogType::Model);
        assert_eq!(focus.focus_index, 1);
    }

    #[test]
    fn pop_dialog_returns_none_when_missing() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));

        assert!(focus.pop_dialog(DialogType::Theme).is_none());
        assert_eq!(focus.len(), 1);
        assert_eq!(focus.focus_index, 0);
    }

    #[test]
    fn replace_top_dialog_swaps_matching_top_component() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.push(stub(DialogType::Stats));
        focus.focus_index = 1;

        let replaced = focus.replace_top_dialog(DialogType::Stats, stub(DialogType::Stats));

        assert!(replaced);
        assert_eq!(focus.len(), 2);
        // Top component is still the Stats entry; its identity changed
        // but the dialog type is preserved so the focus stack stays
        // consistent for future lookups.
        assert_eq!(focus.active_dialog_type(), DialogType::Stats);
        assert_eq!(focus.focus_index, 1);
    }

    #[test]
    fn replace_top_dialog_returns_false_when_missing() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));

        let replaced = focus.replace_top_dialog(DialogType::Stats, stub(DialogType::Stats));

        assert!(!replaced);
        assert_eq!(focus.len(), 1);
        assert_eq!(focus.active_dialog_type(), DialogType::Help);
    }
}
