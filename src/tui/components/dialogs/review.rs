use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own border, heading and keymap footer; [`Chrome::Hosted`] is a
/// column in a side panel, where the host has already painted all three into
/// the same rect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs `ReviewMode::FileList` delivers, for the panel host to reuse.
///
/// `handle_key` also answers `↑/↓` and `Esc`; neither survives docking, so
/// neither is named. See [`HINTS_ARE_MODE_SCOPED`](ReviewMode) below for why
/// `j/k` appear here but not in [`DIFF_VIEW_HINT`].
const FILE_LIST_HINT: &str = "j/k move · enter diff";

/// The verbs `ReviewMode::DiffView` delivers, for the panel host to reuse.
///
/// Only `⌫`. The arm above it is `Esc`, which `SidePanelHost::handle_key`
/// claims as a universal panel exit before this widget is consulted, so a
/// docked diff box has exactly one live verb — and advertising both would be
/// teaching `Esc` to close something the host has already turned into "return
/// the keyboard to the composer".
const DIFF_VIEW_HINT: &str = "⌫ back to list";

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewMode {
    FileList,
    DiffView,
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub path: String,
    pub kind: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone)]
pub struct ReviewDialog {
    pub items: Vec<ReviewItem>,
    pub selected: usize,
    pub scroll: usize,
    pub mode: ReviewMode,
    pub diff_content: Option<(String, String)>,
    pub diff_scroll: usize,
}

impl ReviewDialog {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            scroll: 0,
            mode: ReviewMode::FileList,
            diff_content: None,
            diff_scroll: 0,
        }
    }

    pub fn set_items(&mut self, items: Vec<ReviewItem>) {
        self.items = items;
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn set_diff(&mut self, old: String, new: String) {
        self.diff_content = Some((old, new));
        self.mode = ReviewMode::DiffView;
        self.diff_scroll = 0;
    }

    pub fn selected_item(&self) -> Option<&ReviewItem> {
        self.items.get(self.selected)
    }

    fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll {
                self.scroll = self.selected;
            }
        }
    }

    fn select_down(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
            self.scroll = self.scroll.saturating_add(1);
        }
    }
}

impl Component for ReviewDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match self.mode {
            ReviewMode::FileList => match key.code {
                crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                    if key.modifiers.is_empty() =>
                {
                    self.select_up();
                    None
                }
                crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                    if key.modifiers.is_empty() =>
                {
                    self.select_down();
                    None
                }
                crossterm::event::KeyCode::Enter => {
                    if let Some(item) = self.items.get(self.selected) {
                        let path = item.path.clone();
                        Some(TuiMsg::ReviewOpenDiff { path })
                    } else {
                        None
                    }
                }
                crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
                _ => None,
            },
            ReviewMode::DiffView => match key.code {
                crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                    if key.modifiers.is_empty() =>
                {
                    self.diff_scroll = self.diff_scroll.saturating_add(1);
                    None
                }
                crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                    if key.modifiers.is_empty() =>
                {
                    self.diff_scroll = self.diff_scroll.saturating_add(1);
                    None
                }
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Backspace => {
                    self.mode = ReviewMode::FileList;
                    self.diff_content = None;
                    self.diff_scroll = 0;
                    None
                }
                _ => None,
            },
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    /// The verbs this box owns **in the mode it is currently in**, spelled the
    /// way this box spells them.
    ///
    /// `handle_key` is mode-dispatched, so one hint cannot describe both
    /// modes: `enter` opens a diff in the file list and means nothing in the
    /// diff view, and `⌫` — the only way back that survives the host — means
    /// nothing in the file list. A single string would have to advertise one
    /// of them as dead.
    ///
    /// `DiffView` deliberately does not name `j/k`, even though it dispatches
    /// them. That scroll is inert today — both the `Up|k` and `Down|j` arms do
    /// `diff_scroll.saturating_add(1)`, and the diff renderer never reads the
    /// field — so the honest hint is the one verb that does move. The
    /// direction bug and the unread field are reported, not fixed here: this
    /// milestone changes chrome and keymaps, and a hint must describe what the
    /// box delivers rather than what it intends to.
    fn keymap_hint(&self) -> Option<String> {
        Some(match self.mode {
            ReviewMode::FileList => FILE_LIST_HINT.to_string(),
            ReviewMode::DiffView => DIFF_VIEW_HINT.to_string(),
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    /// The listing alone: no border, no title, no footer.
    ///
    /// The host has already painted a border, a ` Changed Files (n) ` title
    /// and the `●` focus marker into this same rect, and `render_in_panel` is
    /// handed the full rect rather than the interior, so a second border does
    /// not stack — it overwrites all three. `area` is the host's full rect, so
    /// inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Review
    }
}

impl ReviewDialog {
    /// One body routine for both render paths, and both modes.
    ///
    /// The borders and the two footers below were the only difference between
    /// drawing this box as a modal and drawing it in a side column, and both
    /// are gone from the hosted path: the host has painted a border, a title
    /// and the `●` focus marker into the same rect, so a second copy
    /// overwrites all three instead of layering. The footers were worse than
    /// cosmetic — a docked column destroyed them, and the verbs they carried
    /// (`↑/↓ navigate`, `Esc close`) cannot fire from a column in the first
    /// place, because `SidePanelHost::handle_key` claims the arrows as panel
    /// directions and `Esc` as a universal exit before this widget is
    /// consulted. `keymap_hint` is where a docked box now says what it does.
    ///
    /// Keeping the two modes in one routine is the point rather than tidiness:
    /// `FileList` and `DiffView` used to be two render functions with two
    /// footers between them, which is exactly the shape where one mode gets
    /// converted and the other does not.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        if area.height < 5 {
            return;
        }

        match self.mode {
            ReviewMode::FileList => {
                let block = match chrome {
                    Chrome::Owned => Some(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.border))
                            .title(format!(" Changed Files ({}) ", self.items.len())),
                    ),
                    Chrome::Hosted => None,
                };
                let inner = match &block {
                    Some(block) => block.inner(area),
                    None => area,
                };
                if let Some(block) = &block {
                    block.render(area, frame.buffer_mut());
                }

                if self.items.is_empty() {
                    let empty = Line::from(Span::styled(
                        "No changed files",
                        Style::default().fg(theme.muted),
                    ));
                    let empty_area = Rect::new(inner.x, inner.y, inner.width, 1);
                    frame.render_widget(empty, empty_area);
                    return;
                }

                let items: Vec<ListItem> = self
                    .items
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .map(|(i, item)| {
                        let is_selected = i == self.selected;
                        let style = if is_selected {
                            Style::default()
                                .bg(theme.selection)
                                .fg(theme.primary)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.foreground)
                        };

                        let kind_char = match item.kind.as_str() {
                            "Created" | "Added" => "A",
                            "Deleted" => "D",
                            "Renamed" => "R",
                            _ => "M",
                        };

                        let kind_style = match item.kind.as_str() {
                            "Created" | "Added" => Style::default().fg(theme.success),
                            "Deleted" => Style::default().fg(theme.error),
                            "Renamed" => Style::default().fg(theme.warning),
                            _ => Style::default().fg(theme.foreground),
                        };

                        let stats = if item.additions > 0 || item.deletions > 0 {
                            format!(" +{}/-{}", item.additions, item.deletions)
                        } else {
                            String::new()
                        };

                        let line = Line::from(vec![
                            Span::styled(format!("  {} ", kind_char), kind_style),
                            Span::styled(&item.path, style),
                            Span::styled(stats, Style::default().fg(theme.muted)),
                        ]);
                        ListItem::new(line)
                    })
                    .collect();
                let list = List::new(items);

                match chrome {
                    Chrome::Owned => {
                        let footer = Line::from(Span::styled(
                            "↑/↓ navigate  |  Enter diff  |  Esc close",
                            Style::default().fg(theme.muted),
                        ));

                        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                            .split(area);

                        frame.render_widget(list, chunks[0]);
                        frame.render_widget(footer, chunks[1]);
                    }
                    Chrome::Hosted => {
                        frame.render_widget(list, inner);
                    }
                }
            }
            ReviewMode::DiffView => {
                let block = match chrome {
                    Chrome::Owned => Some(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.border))
                            .title(" Diff "),
                    ),
                    Chrome::Hosted => None,
                };
                let inner = match &block {
                    Some(block) => block.inner(area),
                    None => area,
                };
                if let Some(block) = &block {
                    block.render(area, frame.buffer_mut());
                }

                let Some((old, new)) = &self.diff_content else {
                    let msg = Line::from(Span::styled(
                        "No diff content",
                        Style::default().fg(theme.muted),
                    ));
                    let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
                    frame.render_widget(msg, msg_area);
                    return;
                };

                let diff = similar::TextDiff::from_lines(old, new);
                let visible_lines = inner.height as usize;

                // `diff_scroll` is intentionally not consulted: both the `Up|k`
                // and `Down|j` arms in `handle_key` add to it and nothing ever
                // read the field, so reading it here would be a behaviour
                // change in a milestone that changes chrome. Reported, not fixed.
                for (y, change) in diff.iter_all_changes().enumerate() {
                    if y >= visible_lines {
                        break;
                    }

                    let (prefix, color) = match change.tag() {
                        similar::ChangeTag::Delete => ("-", theme.error),
                        similar::ChangeTag::Insert => ("+", theme.success),
                        similar::ChangeTag::Equal => (" ", theme.foreground),
                    };

                    let text = change.value();
                    let line_text = format!("{}{}", prefix, text);
                    let display_line = if line_text.ends_with('\n') {
                        &line_text[..line_text.len() - 1]
                    } else {
                        &line_text
                    };

                    let line = Line::from(Span::styled(
                        display_line.to_string(),
                        Style::default().fg(color),
                    ));

                    let line_area = Rect::new(inner.x, inner.y + y as u16, inner.width, 1);
                    frame.render_widget(line, line_area);
                }

                if chrome == Chrome::Owned {
                    let footer = Line::from(Span::styled(
                        "Esc/Backspace: back to file list",
                        Style::default().fg(theme.muted),
                    ));
                    let footer_area = Rect::new(
                        inner.x,
                        inner.y + inner.height.saturating_sub(1),
                        inner.width,
                        1,
                    );
                    frame.render_widget(footer, footer_area);
                }
            }
        }
    }
}

/// These constructors lost their theme parameter when the render body stopped
/// caching one, which leaves `new()` shaped like `Default` — so it has one.
impl Default for ReviewDialog {
    fn default() -> Self {
        Self::new()
    }
}
