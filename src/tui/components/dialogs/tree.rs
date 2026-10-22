use std::collections::HashSet;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;

use crate::tui::app::TuiMsg;

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub session_id: String,
    pub label: String,
    pub time_updated: i64,
    pub message_count: Option<usize>,
    pub is_current: bool,
    pub is_archived: bool,
    pub children: Vec<TreeNode>,
    pub depth: usize,
}

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

/// The verbs this box implements, for the panel host to reuse verbatim.
///
/// `handle_key` below is the implementation and this is the advertisement.
/// `↑/↓` are deliberately absent even though `handle_key` answers
/// them: a focused panel never sees them, because `SidePanelHost::handle_key`
/// answers the arrow keys as panel directions before consulting the widget.
/// `Esc` is absent for a different reason — it is the mode switch, so it means
/// "close" in the modal box and "leave the widget's mode" in a column.
/// Repeating the footer's old
/// `"↑/↓ navigate … | Esc close"` would teach a docked panel two keys that
/// cannot fire.
const HINT: &str = "j/k move · e expand · f fork · enter select";

#[derive(Clone)]
pub struct TreeDialog {
    pub theme: Arc<Theme>,
    pub nodes: Vec<TreeNode>,
    pub selected: usize,
    pub flat: Vec<(TreeNode, usize)>,
    pub expanded: HashSet<String>,
    pub scroll: CenteredScroll,
    visible_height: usize,
    current_session_id: Option<String>,
}

impl TreeDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            nodes: Vec::new(),
            selected: 0,
            flat: Vec::new(),
            expanded: HashSet::new(),
            scroll: CenteredScroll::new(),
            visible_height: 10,
            current_session_id: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn load_nodes(&mut self, nodes: Vec<TreeNode>, current_session_id: Option<String>) {
        self.nodes = nodes;
        self.current_session_id = current_session_id;
        if self.nodes.is_empty() {
            self.nodes.push(TreeNode {
                id: "root".to_string(),
                session_id: "root".to_string(),
                label: "no session".to_string(),
                time_updated: 0,
                message_count: None,
                is_current: false,
                is_archived: false,
                children: Vec::new(),
                depth: 0,
            });
        }
        self.expanded.clear();
        if let Some(first) = self.nodes.first() {
            self.expanded.insert(first.session_id.clone());
        }
        self.flatten();
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn toggle_expand(&mut self) {
        if let Some((node, _)) = self.flat.get(self.selected) {
            if self.expanded.contains(&node.session_id) {
                self.expanded.remove(&node.session_id);
            } else {
                self.expanded.insert(node.session_id.clone());
            }
            self.flatten();
        }
    }

    pub fn fork_selected(&mut self) -> Option<String> {
        if let Some((node, _)) = self.flat.get(self.selected) {
            return Some(node.session_id.clone());
        }
        None
    }

    fn flatten(&mut self) {
        self.flat.clear();
        let nodes: &[TreeNode] = &self.nodes;
        Self::flatten_nodes_to_flat(nodes, &self.expanded, &mut self.flat);
    }

    fn flatten_nodes_to_flat(
        nodes: &[TreeNode],
        expanded: &std::collections::HashSet<String>,
        flat: &mut Vec<(TreeNode, usize)>,
    ) {
        for node in nodes {
            let indent = 0;
            flat.push((node.clone(), indent));

            if expanded.contains(&node.session_id) {
                Self::flatten_nodes_to_flat(&node.children, expanded, flat);
            }
        }
    }

    #[allow(dead_code)]
    fn flatten_nodes(&mut self, nodes: &[TreeNode], depth: usize) {
        for node in nodes {
            let indent = depth;
            self.flat.push((node.clone(), indent));

            if self.expanded.contains(&node.session_id) {
                self.flatten_nodes(&node.children, depth + 1);
            }
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, self.flat.len(), self.visible_height);
        }
    }

    pub fn select_down(&mut self) {
        let max = self.flat.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, self.flat.len(), self.visible_height);
        }
    }

    /// One listing routine for both render paths.
    ///
    /// The scroll window below is driven by [`Self::visible_height`], and the
    /// same field arms `select_up`/`select_down`, so the window the render pass
    /// paints and the window the key arms clamp against cannot disagree. That
    /// is why this is one routine and not two: a hosted path computing its own
    /// viewport from a different rect is how a docked box ends up with an
    /// unreachable tail and no error anywhere.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        let mut lines: Vec<Line> = Vec::new();

        if chrome == Chrome::Owned {
            lines.push(Line::from(Span::styled(
                " Session Tree ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        if self.flat.is_empty() {
            lines.push(Line::from(Span::styled(
                "no forks",
                Style::default().fg(theme.muted),
            )));
        } else {
            let scroll = self.scroll.get();
            let mut rendered = 0;
            for (i, (node, indent)) in self.flat.iter().enumerate() {
                if i < scroll {
                    continue;
                }
                if rendered >= self.visible_height {
                    break;
                }
                let is_selected = i == self.selected;
                let prefix = "  ".repeat(*indent);
                let connector = if *indent > 0 { "├─ " } else { "" };

                let current_marker = if node.is_current { "● " } else { "  " };
                let has_children = !node.children.is_empty();
                let expand_indicator = if has_children {
                    if self.expanded.contains(&node.session_id) {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    "  "
                };
                let fork_suffix = if node.depth > 0 { " (fork)" } else { "" };
                let archived_suffix = if node.is_archived { " [archived]" } else { "" };

                let bg = if i.is_multiple_of(2) {
                    theme.background
                } else {
                    theme.alternate_bg
                };
                let style = if is_selected {
                    theme.selection_style()
                } else if node.is_archived {
                    Style::default().fg(theme.muted).bg(bg)
                } else if node.is_current {
                    Style::default()
                        .fg(theme.primary)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground).bg(bg)
                };

                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{}{}{}{}{}{}",
                        prefix,
                        connector,
                        current_marker,
                        expand_indicator,
                        node.label,
                        fork_suffix,
                        archived_suffix
                    ),
                    style,
                )));
                rendered += 1;
            }
        }

        if chrome == Chrome::Owned {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "↑/↓ navigate  |  Enter select  |  e expand  |  f fork  |  Esc close",
                Style::default().fg(theme.muted),
            )));
        }

        match chrome {
            Chrome::Owned => {
                let block = Block::default()
                    .title(" Tree ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .style(Style::default().bg(theme.background));
                let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
                paragraph.render(area, frame.buffer_mut());
            }
            Chrome::Hosted => {
                let paragraph = Paragraph::new(lines)
                    .style(Style::default().bg(theme.background))
                    .wrap(Wrap { trim: true });
                paragraph.render(area, frame.buffer_mut());
            }
        }
    }
}

impl Default for TreeDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &TreeDialog {
    #[allow(clippy::incompatible_msrv)]
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Session Tree ",
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if self.flat.is_empty() {
            lines.push(Line::from(Span::styled(
                "no forks",
                Style::default().fg(self.theme.muted),
            )));
        } else {
            let scroll = self.scroll.get();
            let mut rendered = 0;
            for (i, (node, indent)) in self.flat.iter().enumerate() {
                if i < scroll {
                    continue;
                }
                if rendered >= self.visible_height {
                    break;
                }
                let is_selected = i == self.selected;
                let prefix = "  ".repeat(*indent);
                let connector = if *indent > 0 { "├─ " } else { "" };

                let current_marker = if node.is_current { "● " } else { "  " };
                let has_children = !node.children.is_empty();
                let expand_indicator = if has_children {
                    if self.expanded.contains(&node.session_id) {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    "  "
                };
                let fork_suffix = if node.depth > 0 { " (fork)" } else { "" };
                let archived_suffix = if node.is_archived { " [archived]" } else { "" };

                let bg = if i.is_multiple_of(2) {
                    self.theme.background
                } else {
                    self.theme.alternate_bg
                };
                let style = if is_selected {
                    self.theme.selection_style()
                } else if node.is_archived {
                    Style::default().fg(self.theme.muted).bg(bg)
                } else if node.is_current {
                    Style::default()
                        .fg(self.theme.primary)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.foreground).bg(bg)
                };

                let _msg_count = node
                    .message_count
                    .map(|c| format!(" [{} msgs]", c))
                    .unwrap_or_default();

                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{}{}{}{}{}{}",
                        prefix,
                        connector,
                        current_marker,
                        expand_indicator,
                        node.label,
                        fork_suffix,
                        archived_suffix
                    ),
                    style,
                )));
                rendered += 1;
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ navigate  |  Enter select  |  e expand  |  f fork  |  Esc close",
            Style::default().fg(self.theme.muted),
        )));

        let block = Block::default()
            .title(" Tree ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for TreeDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if let Some((node, _)) = self.flat.get(self.selected) {
                    Some(TuiMsg::SelectTreeSession {
                        session_id: node.session_id.clone(),
                    })
                } else {
                    None
                }
            }
            crossterm::event::KeyCode::Char('e') => {
                self.toggle_expand();
                None
            }
            crossterm::event::KeyCode::Char('f') => {
                if let Some((node, _)) = self.flat.get(self.selected) {
                    Some(TuiMsg::ForkTreeSession {
                        session_id: node.session_id.clone(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    /// The verbs this box owns, spelled the way this box spells them.
    fn keymap_hint(&self) -> Option<String> {
        Some(HINT.to_string())
    }

    /// The listing alone: no border, no title, no footer.
    ///
    /// The host has already painted a border, a ` ● Tree ` title and the `●`
    /// focus marker into this same rect, and `render_in_panel` is handed the
    /// full rect rather than the interior, so a second border does not stack —
    /// it overwrites all three, and a focused column renders exactly like an
    /// unfocused one. The heading goes with it: the host's title already names
    /// the panel. `area` is the host's full rect, so inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    #[allow(clippy::incompatible_msrv)]
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn nodes(count: usize) -> Vec<TreeNode> {
        (0..count)
            .map(|i| TreeNode {
                id: format!("node-{i}"),
                session_id: format!("node-{i}"),
                label: format!("session-{i:02}"),
                time_updated: 0,
                message_count: None,
                is_current: false,
                is_archived: false,
                children: Vec::new(),
                depth: 0,
            })
            .collect()
    }

    fn loaded(count: usize) -> TreeDialog {
        let mut dialog = TreeDialog::new(Arc::new(Theme::dark()));
        dialog.load_nodes(nodes(count), None);
        dialog
    }

    /// Render through the panel host, exactly as `render_hosted` does: host
    /// chrome first, then `render_in_panel` over the *same* rect.
    fn hosted_rows(dialog: &mut TreeDialog, width: u16, height: u16) -> Vec<String> {
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Tree ● ")
                        .border_style(Style::default().fg(theme.border)),
                    frame.area(),
                );
                dialog.render_in_panel(frame, frame.area(), &theme);
            })
            .expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect::<Vec<_>>()
            .chunks(width as usize)
            .map(|row| row.concat())
            .collect()
    }

    /// The host's border and the `●` that marks which panel has the keyboard.
    /// A widget's own `Block` over this rect does not layer — it replaces all
    /// three, so a focused column renders exactly like an unfocused one.
    #[test]
    fn the_hosted_path_draws_no_border_of_its_own() {
        let mut dialog = loaded(3);
        let rows = hosted_rows(&mut dialog, 30, 10);
        assert!(
            rows[0].contains("Tree ●"),
            "the panel title and its focus marker are the host's — row 0 is {:?}",
            rows[0]
        );
        assert!(
            rows[0].starts_with('┌') && rows[0].ends_with('┐'),
            "the host's top border was overwritten — row 0 is {:?}",
            rows[0]
        );
        for (y, row) in rows.iter().enumerate().take(rows.len() - 1).skip(1) {
            assert!(
                row.starts_with('│') && row.ends_with('│'),
                "the host's side border was overwritten at row {y} — {row:?}"
            );
        }
    }

    /// The heading and the keymap footer belong to the box, not the column.
    /// The host's title already names the panel, and the same verbs reach a
    /// docked box through the status line above the composer — where they are
    /// legible, which in a 40-column column they were not.
    #[test]
    fn the_hosted_path_keeps_the_boxes_own_heading_and_footer() {
        let mut dialog = loaded(3);
        let rows = hosted_rows(&mut dialog, 40, 12);
        let text = rows.join("\n");
        assert!(
            !text.contains("Session Tree"),
            "the heading duplicates the host's title — {text}"
        );
        assert!(
            !text.contains("Esc close") && !text.contains("navigate"),
            "the footer advertises Esc, which the host answers before this \
             widget is consulted — {text}"
        );
    }

    /// The window the render pass paints and the window the key arms clamp
    /// against both come from `visible_height`, so a hosted box cannot scroll
    /// its tail out of reach the way a second viewport computation would.
    #[test]
    fn both_paths_reach_the_same_last_row() {
        let mut modal = loaded(12);
        let mut hosted = loaded(12);
        // Walk to the end the way the panel host would deliver `j`: repeatedly,
        // because the host consumes every key a focused panel sees.
        for _ in 0..40 {
            let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
            modal.handle_key(j);
            hosted.handle_key(j);
        }
        assert_eq!(
            modal.selected, hosted.selected,
            "the same keys must leave both boxes on the same node"
        );
        assert_eq!(
            modal.scroll.get(),
            hosted.scroll.get(),
            "the same keys must leave both boxes at the same scroll offset"
        );

        // Only the hosted side is asserted to *show* the tail. `visible_height`
        // is an externally-owned field that no production code sets, so it stays
        // at 10 however tall the box is; a 12-row modal has six rows of chrome
        // and therefore clips the tail it was offered. That mismatch predates
        // this milestone and is deliberately not fixed here — what matters is
        // that the two paths land on the same node at the same offset, which the
        // assertions above already pin.
        assert_eq!(modal.visible_height, hosted.visible_height);
        assert!(
            hosted_rows(&mut hosted, 40, 12)
                .join("")
                .contains("session-11"),
            "the hosted view must reach the last node, since it spends none of \
             its rows on a heading or a footer"
        );
    }

    /// `handle_key` answers `Esc`, `↑` and `↓`, and the hint names none of them:
    /// the arrows never reach a focused panel, because `SidePanelHost::handle_key`
    /// answers them as panel directions before consulting the widget, and `Esc`
    /// is the mode switch — it means "close" in the modal box and something
    /// else in a column, so a hosted hint cannot promise it.
    #[test]
    fn keymap_hint_names_no_key_the_host_answers_first() {
        let dialog = loaded(1);
        let hint = dialog.keymap_hint().expect("the tree owns real verbs");
        for dead in ["esc", "Esc", "↑", "↓", "←", "→", "q"] {
            assert!(
                !hint.contains(dead),
                "the hint teaches {dead:?}, which a focused panel cannot deliver: {hint:?}"
            );
        }
        assert_eq!(dialog.keymap_hint().as_deref(), Some(HINT));
    }

    /// A modal keeps its own chrome, which is what makes the hosted override
    /// necessary rather than merely tidy.
    #[test]
    fn the_modal_still_draws_its_own_box() {
        let mut dialog = loaded(3);
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(40u16, 12u16)).expect("test terminal");
        terminal
            .draw(|frame| Component::render(&mut dialog, frame, frame.area(), &theme))
            .expect("draw");
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect::<Vec<_>>()
            .chunks(40)
            .map(|r| r.concat())
            .collect();
        assert!(
            rows[0].contains("Tree"),
            "the modal keeps its titled border — row 0 is {:?}",
            rows[0]
        );
        assert!(
            rows.join("\n").contains("navigate"),
            "the modal keeps its own keymap footer"
        );
    }

    /// A column too small to hold an interior renders nothing rather than
    /// panicking on a zero-sized rect. The host's chrome is already painted, so
    /// the panel shows an empty column.
    #[test]
    fn a_column_with_no_interior_paints_nothing_and_does_not_panic() {
        let mut dialog = loaded(3);
        let rows = hosted_rows(&mut dialog, 2, 2);
        // At width 2 the host's border has no room for a rule, so these rows
        // are the corners themselves. That is the whole claim: the widget
        // returned before painting, so the column is the host's and nothing else.
        assert_eq!(
            rows,
            vec!["┌┐".to_string(), "└┘".to_string()],
            "a 2x2 column has no interior, so only the host's border should remain"
        );
    }
}
