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
    ConnectionSelection,
    Keybind,
    Context,
    Cost,
    Usage,
    Stats,
    Goto,
    Plan,
    Review,
    Confirm,
    ResearchBrowser,
    SecurityReview,
    SourcePreview,
    ShellShow,
    Terminal,
    TaskList,
    WorktreeList,
    GoalShow,
    MemoryResults,
    DoctorReport,
    Plugin,
    RunDetail,
    Collaborators,
    ProjectChat,
    ProjectPicker,
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
            DialogType::ConnectionSelection => Dialog::ConnectionSelection,
            DialogType::Keybind => Dialog::Keybind,
            DialogType::Context => Dialog::Context,
            DialogType::Cost => Dialog::Cost,
            DialogType::Usage => Dialog::Usage,
            DialogType::Stats => Dialog::Stats,
            DialogType::Goto => Dialog::Goto,
            DialogType::Plan => Dialog::Plan,
            DialogType::Review => Dialog::Review,
            DialogType::Confirm => Dialog::Confirm,
            DialogType::ResearchBrowser => Dialog::ResearchBrowser,
            DialogType::SecurityReview => Dialog::SecurityReview,
            DialogType::SourcePreview => Dialog::SourcePreview,
            DialogType::ShellShow => Dialog::ShellShow,
            DialogType::Terminal => Dialog::Terminal,
            DialogType::TaskList => Dialog::TaskList,
            DialogType::WorktreeList => Dialog::WorktreeList,
            DialogType::GoalShow => Dialog::GoalShow,
            DialogType::MemoryResults => Dialog::MemoryResults,
            DialogType::DoctorReport => Dialog::DoctorReport,
            DialogType::Plugin => Dialog::Plugin,
            DialogType::RunDetail => Dialog::RunDetail,
            DialogType::ProjectPicker => Dialog::ProjectPicker,
            DialogType::Collaborators => Dialog::Collaborators,
            DialogType::ProjectChat => Dialog::ProjectChat,
            DialogType::None => Dialog::None,
        }
    }
}

impl From<Dialog> for DialogType {
    fn from(dialog: Dialog) -> Self {
        match dialog {
            Dialog::None => DialogType::None,
            Dialog::Model => DialogType::Model,
            Dialog::Agent => DialogType::Agent,
            Dialog::Session => DialogType::Session,
            Dialog::Help => DialogType::Help,
            Dialog::Tree => DialogType::Tree,
            Dialog::Theme => DialogType::Theme,
            Dialog::Question => DialogType::Question,
            Dialog::Permission => DialogType::Permission,
            Dialog::Mcp => DialogType::Mcp,
            Dialog::Keybind => DialogType::Keybind,
            Dialog::Share => DialogType::Share,
            Dialog::Import => DialogType::Import,
            Dialog::Template => DialogType::Template,
            Dialog::Connect => DialogType::Connect,
            Dialog::ConnectionSelection => DialogType::ConnectionSelection,
            Dialog::Context => DialogType::Context,
            Dialog::Cost => DialogType::Cost,
            Dialog::Usage => DialogType::Usage,
            Dialog::Stats => DialogType::Stats,
            Dialog::Goto => DialogType::Goto,
            Dialog::Plan => DialogType::Plan,
            Dialog::Diff => DialogType::Diff,
            Dialog::Confirm => DialogType::Confirm,
            Dialog::Review => DialogType::Review,
            Dialog::ResearchBrowser => DialogType::ResearchBrowser,
            Dialog::SecurityReview => DialogType::SecurityReview,
            Dialog::SourcePreview => DialogType::SourcePreview,
            Dialog::ShellShow => DialogType::ShellShow,
            Dialog::Terminal => DialogType::Terminal,
            Dialog::TaskList => DialogType::TaskList,
            Dialog::WorktreeList => DialogType::WorktreeList,
            Dialog::GoalShow => DialogType::GoalShow,
            Dialog::MemoryResults => DialogType::MemoryResults,
            Dialog::DoctorReport => DialogType::DoctorReport,
            Dialog::Plugin => DialogType::Plugin,
            Dialog::RunDetail => DialogType::RunDetail,
            Dialog::ProjectPicker => DialogType::ProjectPicker,
            Dialog::Collaborators => DialogType::Collaborators,
            Dialog::ProjectChat => DialogType::ProjectChat,
        }
    }
}

pub trait Component: Send + Any + AsAny {
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

/// Object-safe type erasure used by the narrow typed modal accessors.
pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{Dialog, DialogType};

    #[test]
    fn dialog_discriminators_round_trip_exhaustively() {
        let cases = [
            (DialogType::Share, Dialog::Share),
            (DialogType::Model, Dialog::Model),
            (DialogType::Agent, Dialog::Agent),
            (DialogType::Session, Dialog::Session),
            (DialogType::Help, Dialog::Help),
            (DialogType::Tree, Dialog::Tree),
            (DialogType::Theme, Dialog::Theme),
            (DialogType::Permission, Dialog::Permission),
            (DialogType::Mcp, Dialog::Mcp),
            (DialogType::Question, Dialog::Question),
            (DialogType::Diff, Dialog::Diff),
            (DialogType::Import, Dialog::Import),
            (DialogType::Template, Dialog::Template),
            (DialogType::Connect, Dialog::Connect),
            (DialogType::ConnectionSelection, Dialog::ConnectionSelection),
            (DialogType::Keybind, Dialog::Keybind),
            (DialogType::Context, Dialog::Context),
            (DialogType::Cost, Dialog::Cost),
            (DialogType::Usage, Dialog::Usage),
            (DialogType::Stats, Dialog::Stats),
            (DialogType::Goto, Dialog::Goto),
            (DialogType::Plan, Dialog::Plan),
            (DialogType::Review, Dialog::Review),
            (DialogType::Confirm, Dialog::Confirm),
            (DialogType::ResearchBrowser, Dialog::ResearchBrowser),
            (DialogType::SecurityReview, Dialog::SecurityReview),
            (DialogType::SourcePreview, Dialog::SourcePreview),
            (DialogType::ShellShow, Dialog::ShellShow),
            (DialogType::Terminal, Dialog::Terminal),
            (DialogType::TaskList, Dialog::TaskList),
            (DialogType::WorktreeList, Dialog::WorktreeList),
            (DialogType::GoalShow, Dialog::GoalShow),
            (DialogType::MemoryResults, Dialog::MemoryResults),
            (DialogType::DoctorReport, Dialog::DoctorReport),
            (DialogType::Plugin, Dialog::Plugin),
            (DialogType::RunDetail, Dialog::RunDetail),
            (DialogType::Collaborators, Dialog::Collaborators),
            (DialogType::ProjectChat, Dialog::ProjectChat),
            (DialogType::ProjectPicker, Dialog::ProjectPicker),
            (DialogType::None, Dialog::None),
        ];

        for (expected_type, dialog) in cases {
            assert_eq!(DialogType::from(dialog.clone()), expected_type);
            assert_eq!(Dialog::from(expected_type), dialog);
        }
    }
}
