//! # TUI Component Architecture
//!
//! This module provides the Component trait and FocusManager for decoupled UI architecture.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;
use std::any::Any;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::theme::Theme;
use crate::tui::Dialog;

pub mod context;
pub mod focus;

pub use context::AppContext;
pub use focus::FocusManager;

#[derive(Debug, Clone, PartialEq)]
pub enum DialogType {
    Share,
    Model,
    Agent,
    Session,
    Help,
    Tree,
    Theme,
    Permission,
    Mcp,
    Question,
    Diff,
    Import,
    Template,
    Connect,
    Keybind,
    Context,
    Cost,
    Usage,
    Stats,
    Goto,
    Plan,
    Review,
    Confirm,
    None,
}

impl DialogType {
    pub fn is_modal(&self) -> bool {
        !matches!(self, DialogType::None)
    }
}

impl From<DialogType> for Dialog {
    fn from(dialog_type: DialogType) -> Self {
        match dialog_type {
            DialogType::Share => Dialog::Share,
            DialogType::Model => Dialog::Model,
            DialogType::Agent => Dialog::Agent,
            DialogType::Session => Dialog::Session,
            DialogType::Help => Dialog::Help,
            DialogType::Tree => Dialog::Tree,
            DialogType::Theme => Dialog::Theme,
            DialogType::Permission => Dialog::Permission,
            DialogType::Mcp => Dialog::Mcp,
            DialogType::Question => Dialog::Question,
            DialogType::Diff => Dialog::Diff,
            DialogType::Import => Dialog::Import,
            DialogType::Template => Dialog::Template,
            DialogType::Connect => Dialog::Connect,
            DialogType::Keybind => Dialog::Keybind,
            DialogType::Context => Dialog::Context,
            DialogType::Cost => Dialog::Cost,
            DialogType::Usage => Dialog::Usage,
            DialogType::Stats => Dialog::Stats,
            DialogType::Goto => Dialog::Goto,
            DialogType::Plan => Dialog::Plan,
            DialogType::Review => Dialog::Review,
            DialogType::Confirm => Dialog::Confirm,
            DialogType::None => Dialog::None,
        }
    }
}

pub trait Component: Send + Any {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    fn handle_paste(&mut self, _text: String) -> Option<TuiMsg> {
        None
    }
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool {
        self.dialog_type().is_modal()
    }
    /// Hit test a mouse click at the given row relative to the dialog's Rect (including borders).
    /// `rel_y` = 0 corresponds to the top border of the dialog.
    /// Returns the item index if the row corresponds to a selectable item, or None.
    /// Default implementation disables mouse selection for complex dialogs.
    fn hit_test(&self, _rel_y: usize) -> Option<usize> {
        None
    }
    /// Set the selected item index. Used to sync state from mouse clicks.
    /// Default implementation does nothing.
    fn set_selected(&mut self, _idx: usize) {}
    fn focus_next(&mut self) {}
    fn focus_prev(&mut self) {}
    fn focusable_count(&self) -> usize {
        1
    }
    fn focused_index(&self) -> usize {
        0
    }
    fn set_focused(&mut self, _idx: usize) {}
}
