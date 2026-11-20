use std::cell::RefCell;
use std::sync::Arc;

use crate::agent::Agent;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::ListCursor;
use super::super::section::{Section, SectionFrame};
use crate::tui::app::TuiMsg;
use crate::tui::components::hint_line::HintLine;

pub struct AgentDialog {
    pub theme: Arc<Theme>,
    pub agents: Vec<Agent>,
    pub current: String,
    pub cursor: ListCursor,
    pub filter: String,
    visible_height: usize,
    filtered_cache: RefCell<Option<(String, Vec<usize>)>>,
}

impl Clone for AgentDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            agents: self.agents.clone(),
            current: self.current.clone(),
            cursor: self.cursor.clone(),
            filter: self.filter.clone(),
            visible_height: self.visible_height,
            filtered_cache: RefCell::new(self.filtered_cache.borrow().clone()),
        }
    }
}

impl AgentDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            agents: Vec::new(),
            current: String::new(),
            cursor: ListCursor::new(),
            filter: String::new(),
            visible_height: 10,
            filtered_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_agents(&mut self, agents: Vec<&Agent>) {
        self.agents = agents.into_iter().cloned().collect();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_current(&mut self, current: &str) {
        self.current = current.to_string();
    }

    pub fn selected(&self) -> Option<String> {
        let filtered = self.filtered();
        filtered
            .get(self.cursor.index(filtered.len()))
            .map(|a| a.name.clone())
    }

    pub fn select_up(&mut self) {
        let len = self.filtered().len();
        self.cursor.up(len, self.visible_height);
    }

    pub fn select_down(&mut self) {
        let len = self.filtered().len();
        self.cursor.down(len, self.visible_height);
    }

    pub fn set_filter(&mut self, c: char) {
        self.filter.push(c);
        self.cursor.reset();
        self.update_cache();
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.cursor.reset();
        self.update_cache();
    }

    fn update_cache(&mut self) {
        if self.filter.is_empty() {
            self.filtered_cache.borrow_mut().take();
            return;
        }
        let indices: Vec<usize> = self
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name.to_lowercase().contains(&self.filter.to_lowercase()))
            .map(|(i, _)| i)
            .collect();
        self.filtered_cache
            .borrow_mut()
            .replace((self.filter.clone(), indices));
    }

    pub fn initialize_selection(&mut self, current: &str) {
        let len = self.filtered().len();
        let index = self
            .agents
            .iter()
            .position(|a| a.name == current)
            .unwrap_or_else(|| self.cursor.index(len));
        self.cursor.place(index, len, self.visible_height);
    }

    fn filtered(&self) -> Vec<&Agent> {
        if self.filter.is_empty() {
            self.agents.iter().collect()
        } else {
            if let Some((ref cache_filter, ref indices)) = self.filtered_cache.borrow().as_ref() {
                if cache_filter == &self.filter {
                    return indices.iter().map(|&i| &self.agents[i]).collect();
                }
            }
            self.agents
                .iter()
                .filter(|a| a.name.to_lowercase().contains(&self.filter.to_lowercase()))
                .collect()
        }
    }

    /// The section's edge is styled as the focused surface when this dialog
    /// owns it. A modal is what the user is interacting with, so it gets the
    /// primary edge and title; it does **not** get a `●`, because that marker
    /// names the panel column holding the keyboard and belongs to the host
    /// that framed a docked dialog. The hosted arm paints no cell at all, so
    /// its focus flag cannot matter.
    fn render_body(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Arc<Theme>,
        edge: SectionFrame,
    ) {
        self.set_theme(theme);
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Agent ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(theme.muted)),
                Span::raw(&self.filter),
            ]));
            lines.push(Line::from(""));
        }

        let filtered = self.filtered();
        let scroll = self.cursor.offset();
        let selected = self.cursor.index(filtered.len());
        for (i, agent) in filtered.iter().enumerate() {
            if i < scroll {
                continue;
            }
            let is_current = agent.name == self.current;
            let is_selected = i == selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.foreground)
            };
            let marker = if is_current { "✓ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(theme.muted)),
                Span::styled(&agent.name, style),
                Span::styled(
                    format!(" — {}", agent.description),
                    Style::default().fg(theme.muted),
                ),
            ]));
        }

        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "no agents match filter",
                Style::default().fg(theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            HintLine::from_items(&[("Esc", "close"), ("↑/↓", "navigate"), ("Enter", "select")])
                .render(0),
            Style::default().fg(theme.muted),
        )));

        let section = Section::new("Agents", theme)
            .frame(edge)
            .focused(matches!(edge, SectionFrame::Owned));
        let body = section.draw(area, frame.buffer_mut());
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .render(body, frame.buffer_mut());
    }
}

impl Default for AgentDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Component for AgentDialog {
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
            crossterm::event::KeyCode::Up => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down => {
                self.select_down();
                None
            }
            // `j`/`k` move the agent list only while the filter is empty;
            // once the user is typing a query they are ordinary characters.
            crossterm::event::KeyCode::Char('k') if self.filter.is_empty() => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Char('j') if self.filter.is_empty() => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => self
                .selected()
                .map(|agent_name| TuiMsg::SelectAgent { agent_name }),
            crossterm::event::KeyCode::Char(c) => {
                self.set_filter(c);
                None
            }
            crossterm::event::KeyCode::Backspace => {
                self.backspace_filter();
                None
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

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, SectionFrame::Owned);
    }

    /// The body alone, inside the frame the host already drew.
    ///
    /// `SectionFrame::Hosted` is the structural half of this seam: the section
    /// writes no cell at all and answers the full column rect, so the host's
    /// border, its title and the `●` survive by construction rather than by
    /// this method remembering not to paint them. The file previously had no
    /// override at all while `docks_as_viewer` returned `true`, so docking the
    /// editor painted its border and title over the host's three.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, SectionFrame::Hosted);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Agent
    }
}

#[cfg(test)]
mod vim_key_tests {
    use super::*;
    use crate::tui::components::component::Component;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn dialog() -> AgentDialog {
        AgentDialog::new(Arc::new(Theme::default()))
    }

    #[test]
    fn vim_keys_extend_an_active_filter() {
        let mut d = dialog();
        d.set_filter('a');
        d.handle_key(key('j'));
        d.handle_key(key('k'));
        assert_eq!(
            d.filter, "ajk",
            "j/k must be ordinary characters once a filter is being typed"
        );
    }

    #[test]
    fn vim_keys_do_not_type_while_the_filter_is_empty() {
        // With an empty filter the letters stay navigation keys, so they
        // must never leak into the filter.
        let mut d = dialog();
        d.handle_key(key('j'));
        d.handle_key(key('k'));
        assert_eq!(d.filter, "", "j/k must not open the filter");
    }
}

#[cfg(test)]
mod panel_chrome_tests {
    use super::*;
    use crate::tui::panels::render_hosted;
    use ratatui::buffer::Buffer;

    /// A label that is deliberately *not* the dialog's own title, so a
    /// surviving host title is evidence rather than coincidence.
    const HOST_LABEL: &str = "Roster";

    fn theme() -> Arc<Theme> {
        Arc::new(Theme::dark())
    }

    fn hosted_rows(dialog: &mut dyn Component, area: Rect, theme: &Arc<Theme>) -> Vec<String> {
        let mut buf = Buffer::empty(area);
        render_hosted(dialog, &mut buf, area, theme, HOST_LABEL, true);
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| {
                        buf.cell((x, y))
                            .map(|cell| cell.symbol().to_string())
                            .unwrap_or_else(|| " ".to_string())
                    })
                    .collect()
            })
            .collect()
    }

    /// Whether `rows` still carries the host's border, title and `●`.
    ///
    /// Returns the first violation rather than a count: one named defect
    /// localises the regression, a count does not.
    fn host_chrome_intact(rows: &[String]) -> Result<(), String> {
        let height = rows.len();
        let top = &rows[0];
        if !(top.starts_with('┌') && top.ends_with('┐')) {
            return Err(format!("the host top border is gone — row 0 is {top:?}"));
        }
        let bottom = &rows[height - 1];
        if !(bottom.starts_with('└') && bottom.ends_with('┘')) {
            return Err(format!(
                "the host bottom border is gone — row {} is {bottom:?}",
                height - 1
            ));
        }
        for (y, row) in rows.iter().enumerate().skip(1).take(height - 2) {
            let first = row.chars().next().unwrap_or(' ');
            let last = row.chars().last().unwrap_or(' ');
            if first != '│' || last != '│' {
                return Err(format!("the host side border is broken at row {y}"));
            }
        }
        if !top.contains(HOST_LABEL) {
            return Err(format!(
                "the host's own title `{HOST_LABEL}` is not on screen — row 0 is {top:?}"
            ));
        }
        if !top.contains('●') {
            return Err(format!("the `●` focus marker is gone — row 0 is {top:?}"));
        }
        Ok(())
    }

    /// The pre-fix shape: a dockable box that never overrode
    /// `render_in_panel`, so the default delegated to the modal render and its
    /// own border overwrote the host's.
    struct NoPanelSeam(AgentDialog);

    impl Component for NoPanelSeam {
        fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
            self.0.handle_key(key)
        }

        fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
            self.0.update(msg)
        }

        fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
            self.0.render(frame, area, theme);
        }

        fn docks_as_viewer(&self) -> bool {
            true
        }

        fn dialog_type(&self) -> DialogType {
            DialogType::Agent
        }
    }

    #[test]
    fn a_docked_agent_dialog_leaves_the_hosts_border_title_and_focus_dot_alone() {
        let theme = theme();
        let area = Rect::new(0, 0, 44, 16);
        let mut dialog = AgentDialog::new(Arc::clone(&theme));
        let rows = hosted_rows(&mut dialog, area, &theme);

        assert_eq!(host_chrome_intact(&rows), Ok(()), "{rows:#?}");
        let text = rows.concat();
        assert!(
            !text.contains(" Agents "),
            "the dialog's own title is on screen, so it overwrote the host's: {rows:#?}"
        );
        assert!(
            text.contains("Select Agent"),
            "the body must survive the chrome change: {rows:#?}"
        );
    }

    #[test]
    fn the_docked_chrome_assertion_detects_a_box_that_paints_its_own_border() {
        // The assertion's non-vacuity proof, and the exact defect this
        // milestone repairs: a `docks_as_viewer` box with no `render_in_panel`
        // override. If this passed, the test above would prove nothing.
        let theme = theme();
        let area = Rect::new(0, 0, 44, 16);
        let mut unfixed = NoPanelSeam(AgentDialog::new(Arc::clone(&theme)));
        let rows = hosted_rows(&mut unfixed, area, &theme);

        let verdict = host_chrome_intact(&rows);
        assert!(
            verdict.is_err(),
            "a box that paints its own border must fail the hosted-chrome check: {rows:#?}"
        );
        let message = verdict.unwrap_err();
        assert!(
            message.contains("Agents"),
            "the report must name the overwritten title, got: {message}"
        );
    }
}

#[cfg(test)]
mod cursor_policy_tests {
    use super::*;
    use crate::tui::components::scroll::ScrollPolicy;

    fn dialog_with(names: &[&str], visible_height: usize) -> AgentDialog {
        let agents: Vec<Agent> = names
            .iter()
            .map(|name| Agent {
                name: (*name).to_string(),
                ..Default::default()
            })
            .collect();
        let mut dialog = AgentDialog::new(Arc::new(Theme::default()));
        dialog.set_agents(agents.iter().collect());
        dialog.set_visible_height(visible_height);
        dialog
    }

    #[test]
    fn agent_cursor_clamps_at_both_ends_and_never_wraps() {
        let mut dialog = dialog_with(&["alpha", "beta", "gamma"], 2);

        dialog.select_up();
        assert_eq!(
            dialog.cursor.index(3),
            0,
            "ScrollPolicy::Clamp must stop on the first agent, not wrap to the last"
        );

        dialog.select_down();
        dialog.select_down();
        dialog.select_down();
        assert_eq!(
            dialog.cursor.index(3),
            2,
            "ScrollPolicy::Clamp must stop on the last agent, not wrap to the first"
        );
        assert_eq!(dialog.cursor.policy(), ScrollPolicy::Clamp);
    }

    #[test]
    fn agent_window_follows_only_when_the_cursor_leaves_it() {
        let mut dialog = dialog_with(&["alpha", "beta", "gamma"], 2);

        dialog.select_down();
        assert_eq!(
            dialog.cursor.offset(),
            0,
            "a step inside the window is free"
        );
        dialog.select_down();
        assert_eq!(
            dialog.cursor.offset(),
            1,
            "the window must follow once the cursor passes its last visible row"
        );
        dialog.select_up();
        assert_eq!(
            dialog.cursor.offset(),
            1,
            "stepping back inside the window must not scroll again"
        );
        dialog.select_up();
        assert_eq!(dialog.cursor.offset(), 0);
    }

    #[test]
    fn agent_filter_that_matches_nothing_leaves_no_selection() {
        let mut dialog = dialog_with(&["alpha"], 4);
        dialog.set_filter('z');
        assert_eq!(dialog.cursor.index(0), 0);
        assert!(dialog.selected().is_none());
    }
}
