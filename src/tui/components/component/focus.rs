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
            if idx < self.stack.len() {
                let removed = self.stack.remove(idx).unwrap();
                return Some(removed);
            }
        }
        None
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
