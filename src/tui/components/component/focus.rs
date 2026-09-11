//! FocusManager - owns live modal components and manages modal focus.
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
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            stack: VecDeque::new(),
        }
    }

    /// Push one live component. Duplicate dialog types are rejected so an
    /// async refresh or rapid reopen cannot create a hidden second owner.
    pub fn push(&mut self, component: Box<dyn Component>) -> bool {
        if self
            .stack
            .iter()
            .any(|existing| existing.dialog_type() == component.dialog_type())
        {
            return false;
        }
        self.stack.push_back(component);
        true
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
            return self.stack.remove(idx);
        }
        None
    }

    /// Mutate the canonical live component for a dialog type. The helper is
    /// intentionally typed at the call site and never exposes the stack.
    pub fn with_dialog_mut<T: Component + 'static, R>(
        &mut self,
        dialog_type: DialogType,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.stack
            .iter_mut()
            .rev()
            .find(|component| component.dialog_type() == dialog_type)
            .and_then(|component| component.as_any_mut().downcast_mut::<T>())
            .map(f)
    }

    pub fn with_component_mut<T: Component + 'static, R>(
        &mut self,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.stack
            .iter_mut()
            .rev()
            .find_map(|component| component.as_any_mut().downcast_mut::<T>())
            .map(f)
    }

    /// Return the canonical live component for typed callers that need to
    /// perform several related mutations. The lifetime is tied to this
    /// manager, so it cannot outlive the live stack entry.
    pub fn dialog_mut<T: Component + 'static>(
        &mut self,
        dialog_type: DialogType,
    ) -> Option<&mut T> {
        self.stack
            .iter_mut()
            .rev()
            .find(|component| component.dialog_type() == dialog_type)
            .and_then(|component| component.as_any_mut().downcast_mut::<T>())
    }

    /// Mutate the one live component of a type, independent of its current
    /// dialog subtype. This is used by generic InfoDialog content updates,
    /// where content kind may change without creating a second instance.
    pub fn dialog_mut_any<T: Component + 'static>(&mut self) -> Option<&mut T> {
        self.stack
            .iter_mut()
            .rev()
            .find_map(|component| component.as_any_mut().downcast_mut::<T>())
    }

    /// Read the canonical live component for a dialog type.
    pub fn with_dialog<T: Component + 'static, R>(
        &self,
        dialog_type: DialogType,
        f: impl FnOnce(&T) -> R,
    ) -> Option<R> {
        self.stack
            .iter()
            .rev()
            .find(|component| component.dialog_type() == dialog_type)
            .and_then(|component| component.as_any().downcast_ref::<T>())
            .map(f)
    }

    pub fn has_dialog(&self, dialog_type: DialogType) -> bool {
        self.stack
            .iter()
            .any(|component| component.dialog_type() == dialog_type)
    }

    pub fn has_component<T: Component + 'static>(&self) -> bool {
        self.stack
            .iter()
            .any(|component| component.as_any().is::<T>())
    }

    pub fn with_component<T: Component + 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.stack
            .iter()
            .rev()
            .find_map(|component| component.as_any().downcast_ref::<T>())
            .map(f)
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
                let current = top.focused_index().min(count - 1);
                if reverse {
                    top.set_focused((current + count - 1) % count);
                } else {
                    top.set_focused((current + 1) % count);
                }
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
        focused: usize,
        focusable: usize,
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
            self.focusable
        }

        fn focused_index(&self) -> usize {
            self.focused
        }

        fn set_focused(&mut self, idx: usize) {
            self.focused = idx;
        }
    }

    fn stub(dialog_type: DialogType) -> Box<dyn Component> {
        stub_with_count(dialog_type, 3)
    }

    fn stub_with_count(dialog_type: DialogType, focusable: usize) -> Box<dyn Component> {
        Box::new(StubComponent {
            dialog_type,
            focused: 0,
            focusable,
        })
    }

    #[test]
    fn pop_dialog_removes_match_and_preserves_valid_component_focus() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.push(stub(DialogType::Theme));
        focus.push(stub(DialogType::Model));
        let removed = focus.pop_dialog(DialogType::Theme);

        assert!(removed.is_some());
        assert_eq!(focus.len(), 2);
        assert_eq!(focus.active_dialog_type(), DialogType::Model);
        assert_eq!(focus.top().unwrap().focused_index(), 0);
    }

    #[test]
    fn pop_dialog_returns_none_when_missing() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));

        assert!(focus.pop_dialog(DialogType::Theme).is_none());
        assert_eq!(focus.len(), 1);
        assert_eq!(focus.top().unwrap().focused_index(), 0);
    }

    #[test]
    fn pop_dialog_preserves_the_remaining_component_focus() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.push(stub(DialogType::Theme));
        focus.push(stub(DialogType::Model));

        focus.pop_dialog(DialogType::Theme);

        assert_eq!(focus.active_dialog_type(), DialogType::Model);
    }

    #[test]
    fn push_ignores_duplicate_dialog_type() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.push(stub(DialogType::Help));

        assert_eq!(focus.len(), 1);
    }

    #[test]
    fn pop_preserves_valid_component_focus() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.push(stub(DialogType::Theme));
        focus.pop();

        assert_eq!(focus.top().unwrap().focused_index(), 0);
    }

    #[test]
    fn tab_wraps_forward_and_reverse_using_component_focus() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));

        for _ in 0..3 {
            focus.handle_key(KeyEvent::from(crossterm::event::KeyCode::Tab));
        }
        assert_eq!(focus.top().unwrap().focused_index(), 0);

        focus.handle_key(KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert_eq!(focus.top().unwrap().focused_index(), 2);
    }

    #[test]
    fn tab_is_safe_for_zero_and_one_focusable_controls() {
        let mut zero = FocusManager::new();
        zero.push(stub_with_count(DialogType::Help, 0));
        zero.handle_key(KeyEvent::from(crossterm::event::KeyCode::Tab));
        zero.handle_key(KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert_eq!(zero.top().unwrap().focused_index(), 0);

        let mut one = FocusManager::new();
        one.push(stub_with_count(DialogType::Theme, 1));
        one.handle_key(KeyEvent::from(crossterm::event::KeyCode::Tab));
        one.handle_key(KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert_eq!(one.top().unwrap().focused_index(), 0);
    }

    #[test]
    fn nested_modal_pop_restores_underlying_component_focus() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        focus.with_dialog_mut::<StubComponent, _>(DialogType::Help, |component| {
            component.focused = 2;
        });
        focus.push(stub(DialogType::Theme));
        assert_eq!(focus.active_dialog_type(), DialogType::Theme);
        focus.pop();
        assert_eq!(focus.active_dialog_type(), DialogType::Help);
        assert_eq!(focus.top().unwrap().focused_index(), 2);
    }

    #[test]
    fn typed_lookup_mutates_the_live_component() {
        let mut focus = FocusManager::new();
        focus.push(stub(DialogType::Help));
        assert!(focus
            .with_dialog_mut::<StubComponent, _>(DialogType::Help, |component| {
                component.focused = 2;
            })
            .is_some());
        assert_eq!(focus.top().unwrap().focused_index(), 2);
    }
}
