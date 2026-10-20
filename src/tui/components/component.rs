//! # TUI Component Architecture
//!
//! This module provides the Component trait and FocusManager for decoupled UI architecture.

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
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
    Files,
    Theme,
    Permission,
    PanelManager,
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
    Logs,
    Plugin,
    RunDetail,
    Collaborators,
    ProjectChat,
    Team,
    /// `/control` output. Was folded into the `Team` slot because both are
    /// `InfoDialog`; it needs its own identity now that `App::dock_target`
    /// reads the discriminator to decide which panel a box docks into, and
    /// `/control` and `/team` are different panels.
    Control,
    TeamTokenSecret,
    ProjectPicker,
    TaskSchedule,
    TaskView,
    TriggerSecret,
    WorkspaceDashboard,
    PolicySelector,
    /// M004: the session's waiting prompts (`/queue`).
    Queue,
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
            DialogType::Help => Dialog::Help(
                // A bare dialog *type* carries no route, so it gets the one
                // that cannot copy. The route is carried by the open request
                // (`Dialog::Help(route)`), which is where the decision
                // belongs.
                crate::tui::components::dialogs::help::HelpRoute::Command,
            ),
            DialogType::Tree => Dialog::Tree,
            DialogType::Files => Dialog::Files,
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
            DialogType::Logs => Dialog::Logs,
            DialogType::Plugin => Dialog::Plugin,
            DialogType::RunDetail => Dialog::RunDetail,
            DialogType::ProjectPicker => Dialog::ProjectPicker,
            DialogType::Collaborators => Dialog::Collaborators,
            DialogType::ProjectChat => Dialog::ProjectChat,
            DialogType::Team => Dialog::Team,
            DialogType::Control => Dialog::Control,
            DialogType::TeamTokenSecret => Dialog::TeamTokenSecret,
            DialogType::TaskSchedule => Dialog::TaskSchedule,
            DialogType::TaskView => Dialog::TaskView,
            DialogType::TriggerSecret => Dialog::TriggerSecret,
            DialogType::WorkspaceDashboard => Dialog::WorkspaceDashboard,
            DialogType::PolicySelector => Dialog::PolicySelector,
            DialogType::Queue => Dialog::Queue,
            DialogType::None => Dialog::None,
            DialogType::PanelManager => Dialog::PanelManager,
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
            Dialog::Help(_) => DialogType::Help,
            Dialog::Tree => DialogType::Tree,
            Dialog::Files => DialogType::Files,
            Dialog::Theme => DialogType::Theme,
            Dialog::Question => DialogType::Question,
            Dialog::Permission => DialogType::Permission,
            Dialog::PanelManager => DialogType::PanelManager,
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
            Dialog::Logs => DialogType::Logs,
            Dialog::Plugin => DialogType::Plugin,
            Dialog::RunDetail => DialogType::RunDetail,
            Dialog::ProjectPicker => DialogType::ProjectPicker,
            Dialog::Collaborators => DialogType::Collaborators,
            Dialog::ProjectChat => DialogType::ProjectChat,
            Dialog::Team => DialogType::Team,
            Dialog::Control => DialogType::Control,
            Dialog::TeamTokenSecret => DialogType::TeamTokenSecret,
            Dialog::TaskSchedule => DialogType::TaskSchedule,
            Dialog::TaskView => DialogType::TaskView,
            Dialog::TriggerSecret => DialogType::TriggerSecret,
            Dialog::WorkspaceDashboard => DialogType::WorkspaceDashboard,
            Dialog::PolicySelector => DialogType::PolicySelector,
            Dialog::Queue => DialogType::Queue,
        }
    }
}

pub trait Component: Send + Any + AsAny {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    /// Whether the panel host may resolve the docking gesture for this
    /// dialog on its behalf.
    ///
    /// `App::handle_dialog_key` intercepts `Shift+←/→/↑/↓` before handing a
    /// key to the mounted modal, but **only** when this is `true`. That is
    /// what lets the 24 plain viewer boxes dock into the side column without
    /// each re-implementing the same check.
    ///
    /// The default is `false`, and that default is load-bearing: a dialog
    /// with a composer, a filter, or its own mode tracking must answer the
    /// gesture itself, and a box that already does must not be intercepted
    /// out from under it. Opting a dialog *in* is a claim that its
    /// Shift-qualified keys are free and that nothing here needs them.
    fn docks_as_viewer(&self) -> bool {
        false
    }
    /// The verbs this widget owns, spelled the way this widget spells them.
    ///
    /// The panel host asks for this so the shared hint line above the composer
    /// can describe **this box** rather than a host-side paraphrase of it. The
    /// keymap is written next to the `handle_key` arms that implement it and
    /// read from the same place, which is what makes "the box, its panel, and
    /// the hint line cannot disagree" true rather than aspirational.
    ///
    /// `None` means "the host's generic line is correct for me" — the default,
    /// so a widget with nothing distinctive to say is unchanged.
    fn keymap_hint(&self) -> Option<String> {
        None
    }
    /// Render into a side-panel column.
    ///
    /// Defaults to the modal render, so a widget that draws no chrome of its
    /// own keeps working when the host adopts it. A widget that *does* — a
    /// border, a title, a keymap footer — overrides this instead of `render`,
    /// because the host has already painted a border and a title into the same
    /// rect and a second set of chrome overwrites it, focus dot included.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render(frame, area, theme);
    }
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

/// A centred `Rect` of `percent_x` × `percent_y` inside `area`.
///
/// The one definition of this shape. It was duplicated in
/// `src/tui/app/mod.rs` and `src/tui/components/dialogs/run_detail.rs` — not
/// byte-for-byte identical (one copy qualified `Direction::Vertical`, the other
/// did not, and the third parameter was named `area` in one and `r` in the
/// other), but semantically the same function, so a fix to one would have left
/// the other drawing a differently-sized popup.
///
/// It lives here rather than in either caller because `component.rs` is the one
/// module both `app` and the dialogs already depend on. A dialog reaching into
/// `app::centered_rect` would invert the dependency, which is how the
/// duplication started.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

/// A component that can be drawn either as a modal box or as a docked panel
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
            (
                DialogType::Help,
                Dialog::Help(crate::tui::components::dialogs::help::HelpRoute::Command),
            ),
            (DialogType::Tree, Dialog::Tree),
            (DialogType::Files, Dialog::Files),
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
            (DialogType::Logs, Dialog::Logs),
            (DialogType::Plugin, Dialog::Plugin),
            (DialogType::RunDetail, Dialog::RunDetail),
            (DialogType::Collaborators, Dialog::Collaborators),
            (DialogType::ProjectChat, Dialog::ProjectChat),
            (DialogType::Team, Dialog::Team),
            (DialogType::TeamTokenSecret, Dialog::TeamTokenSecret),
            (DialogType::ProjectPicker, Dialog::ProjectPicker),
            (DialogType::TaskSchedule, Dialog::TaskSchedule),
            (DialogType::TaskView, Dialog::TaskView),
            (DialogType::TriggerSecret, Dialog::TriggerSecret),
            (DialogType::WorkspaceDashboard, Dialog::WorkspaceDashboard),
            (DialogType::PolicySelector, Dialog::PolicySelector),
            (DialogType::Queue, Dialog::Queue),
            (DialogType::None, Dialog::None),
        ];

        for (expected_type, dialog) in cases {
            assert_eq!(DialogType::from(dialog.clone()), expected_type);
            assert_eq!(Dialog::from(expected_type), dialog);
        }
    }
}
