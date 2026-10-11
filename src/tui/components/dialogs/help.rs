use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::input::{build_help_lines, help_entries_for_mode, HelpMode, InputMode};

/// Rows moved by PageUp/PageDown.
const PAGE_STEP: isize = 10;
use crate::tui::theme::Theme;

/// Upper bound on the live filter query. Bounded so a held-down key cannot
/// grow the dialog's state without limit.
const MAX_QUERY_LEN: usize = 64;

/// One searchable help row: the key, what it does, and its gating condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRow {
    pub key: String,
    pub action: String,
    pub condition: Option<String>,
    pub section: String,
}

impl HelpRow {
    /// Case-insensitive substring match against every searchable field.
    ///
    /// All terms must match (AND), so `ctrl panel` narrows rather than widens.
    fn matches(&self, query: &str) -> bool {
        query.split_whitespace().all(|term| {
            let needle = term.to_lowercase();
            self.key.to_lowercase().contains(&needle)
                || self.action.to_lowercase().contains(&needle)
                || self.section.to_lowercase().contains(&needle)
                || self
                    .condition
                    .as_deref()
                    .is_some_and(|c| c.to_lowercase().contains(&needle))
        })
    }

    /// The single rendered line for this row, matching `build_help_lines`
    /// formatting so an unfiltered row looks exactly as it used to.
    fn render_line(&self) -> String {
        let condition_note = match self.condition.as_deref() {
            Some(c) if !c.is_empty() => format!(" ({c})"),
            _ => String::new(),
        };
        format!("  {:<16} {}{}", self.key, self.action, condition_note)
    }
}

pub struct HelpDialog {
    help_lines: Vec<String>,
    rows: Vec<HelpRow>,
    query: String,
    theme: Arc<Theme>,
    scroll: usize,
}

impl Clone for HelpDialog {
    fn clone(&self) -> Self {
        Self {
            help_lines: self.help_lines.clone(),
            rows: self.rows.clone(),
            query: self.query.clone(),
            theme: Arc::clone(&self.theme),
            scroll: self.scroll,
        }
    }
}

impl HelpDialog {
    pub fn new(theme: Arc<Theme>, help_lines: Vec<String>) -> Self {
        // Pre-rendered lines carry no structured fields, so nothing is
        // searchable. Filtering stays inert and the caller can still scroll.
        Self {
            help_lines,
            rows: Vec::new(),
            query: String::new(),
            theme,
            scroll: 0,
        }
    }

    /// Create a help dialog with mode-aware content.
    pub fn new_with_mode(theme: Arc<Theme>, vim_mode: bool, input_mode: InputMode) -> Self {
        let active_mode = match input_mode {
            InputMode::Insert => HelpMode::Insert,
            InputMode::Normal => HelpMode::Normal,
        };
        let help_lines = build_help_lines(vim_mode, active_mode);
        let rows = searchable_help_rows(vim_mode, active_mode);
        Self {
            help_lines,
            rows,
            query: String::new(),
            theme,
            scroll: 0,
        }
    }

    /// Rows matching the current query, or every row when the query is empty.
    pub fn visible_rows(&self) -> Vec<&HelpRow> {
        if self.query.trim().is_empty() {
            return self.rows.iter().collect();
        }
        self.rows
            .iter()
            .filter(|row| row.matches(self.query.trim()))
            .collect()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    /// Append printable input to the filter, returning to the top of the
    /// results so a newly-narrowed list is not left scrolled past its end.
    fn push_query(&mut self, ch: char) {
        if self.query.chars().count() >= MAX_QUERY_LEN {
            return;
        }
        self.query.push(ch);
        self.scroll = 0;
    }

    /// Backspace one character from the filter.
    fn pop_query(&mut self) {
        self.query.pop();
        self.scroll = 0;
    }

    /// Number of rows the filter currently admits.
    fn result_count(&self) -> usize {
        if self.rows.is_empty() {
            self.help_lines.len()
        } else {
            self.visible_rows().len()
        }
    }

    /// Scroll by `delta`, clamped to the result set.
    fn scroll_by(&mut self, delta: isize) {
        let total = self.result_count() as isize;
        if total == 0 {
            self.scroll = 0;
            return;
        }
        let next = (self.scroll as isize + delta).clamp(0, total - 1);
        self.scroll = next as usize;
    }
}

/// Rows the vim-gating rule keeps visible, as owned strings.
///
/// Mirrors the filter `build_help_lines` applies: an entry whose condition
/// mentions vim mode is hidden when vim mode is off. Filtering must not surface
/// an entry that the unfiltered view hides.
fn help_rows_for_mode(vim_mode: bool, mode: HelpMode) -> Vec<HelpRow> {
    help_entries_for_mode(mode)
        .into_iter()
        .filter(|e| {
            if !vim_mode {
                if let Some(cond) = e.condition {
                    return !cond.contains("vim mode");
                }
            }
            true
        })
        .map(|e| HelpRow {
            key: e.key.to_string(),
            action: e.action.to_string(),
            condition: e.condition.map(str::to_string),
            section: String::new(),
        })
        .collect()
}

/// Flatten the help entries into searchable rows.
///
/// The section name is folded into each row's `action` rather than kept as a
/// separate row type, so a filter can never orphan a header from the entries it
/// labels. The active mode is listed first: that ordering is what
/// `build_help_lines` already does, and preserving it keeps the unscrolled view
/// byte-identical to the pre-filter dialog.
fn searchable_help_rows(vim_mode: bool, active_mode: HelpMode) -> Vec<HelpRow> {
    let mut order = Vec::with_capacity(4);
    order.push(active_mode);
    for mode in [
        HelpMode::Insert,
        HelpMode::Normal,
        HelpMode::Command,
        HelpMode::Dialog,
    ] {
        if mode != active_mode {
            order.push(mode);
        }
    }

    let mut out = Vec::new();
    for mode in order {
        let section = match mode {
            HelpMode::Insert => "Insert Mode",
            HelpMode::Normal => "Normal Mode",
            HelpMode::Command => "Command Mode",
            HelpMode::Dialog => "Dialog Mode",
        };
        for entry in help_rows_for_mode(vim_mode, mode) {
            out.push(HelpRow {
                key: entry.key,
                action: entry.action,
                condition: entry.condition,
                section: section.to_string(),
            });
        }
    }
    out
}

impl Component for HelpDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::{KeyCode, KeyModifiers};
        match key.code {
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-PAGE_STEP),
            KeyCode::PageDown => self.scroll_by(PAGE_STEP),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = self.result_count().saturating_sub(1),
            KeyCode::Backspace => self.pop_query(),
            KeyCode::Esc => {
                // Esc clears an active filter first and closes only when there
                // is nothing to clear, so one Esc never discards a query the
                // user is midway through typing.
                if self.query.is_empty() {
                    return Some(TuiMsg::CloseDialog);
                }
                self.query.clear();
                self.scroll = 0;
            }
            KeyCode::Char(ch)
                if key.modifiers == KeyModifiers::NONE
                    || key.modifiers == KeyModifiers::SHIFT
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                // Printable keys filter. `j`/`k` are deliberately *not*
                // navigation here: they have to be typeable to search for
                // "jump", and the arrows plus PageUp/PageDown still scroll.
                self.push_query(ch);
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(3), // title
            Constraint::Length(3), // filter
            Constraint::Min(1),    // content
            Constraint::Length(3), // footer
        ])
        .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "Help",
            Style::default().fg(theme.primary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(title, chunks[0]);

        // ── Filter row ──────────────────────────────────────────────────
        // Always visible, so it is discoverable without pressing anything
        // first. When the query is empty it shows the affordance rather than
        // an empty box the user could mistake for a dead input.
        let filtering = !self.query.trim().is_empty();
        let matches = self.visible_rows().len();
        let total = self.rows.len();
        let filter_spans = if filtering {
            vec![
                Span::styled("search ", Style::default().fg(theme.muted)),
                Span::styled(
                    format!("{}_", self.query),
                    Style::default().fg(theme.primary),
                ),
                Span::styled(
                    format!("  {matches}/{total}"),
                    Style::default().fg(theme.muted),
                ),
            ]
        } else {
            vec![Span::styled(
                "type to filter keybindings…",
                Style::default().fg(theme.muted),
            )]
        };
        let filter = Paragraph::new(Line::from(filter_spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if filtering {
                    theme.primary
                } else {
                    theme.border
                })),
        );
        frame.render_widget(filter, chunks[1]);

        // ── Content ─────────────────────────────────────────────────────
        let content_height = chunks[2].height as usize;
        let visible_height = content_height.saturating_sub(2);
        let lines: Vec<String> = if self.rows.is_empty() {
            // Pre-rendered caller (tests, non-modal surfaces): no structured
            // rows to filter, so show exactly what was supplied.
            self.help_lines.clone()
        } else if filtering {
            self.visible_rows()
                .iter()
                .map(|row| row.render_line())
                .collect()
        } else {
            // Unfiltered structured view reuses the canonical builder, so the
            // idle dialog is byte-identical to the pre-filter rendering.
            self.help_lines.clone()
        };

        let total_lines = lines.len();
        // Key handling does not know the dialog's eventual size. Clamp here
        // so repeated Down presses cannot scroll into a blank content pane.
        let max_scroll = total_lines.saturating_sub(visible_height);
        self.scroll = self.scroll.min(max_scroll);
        let visible_lines: Vec<Line> = lines
            .iter()
            .skip(self.scroll)
            .take(visible_height)
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.foreground))))
            .collect();

        let content = Paragraph::new(visible_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(content, chunks[2]);

        let mut scrollbar_state = ScrollbarState::new(total_lines).position(self.scroll);
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(theme.primary)),
            chunks[2],
            &mut scrollbar_state,
        );

        // ── Footer ──────────────────────────────────────────────────────
        let footer_text = if filtering {
            "↑/↓ PgUp/PgDn scroll  |  Esc clear filter  |  Esc again close"
        } else {
            "type to filter  |  ↑/↓ PgUp/PgDn scroll  |  Esc close"
        };
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(theme.secondary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(footer, chunks[3]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Help
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn dialog() -> HelpDialog {
        HelpDialog::new_with_mode(Arc::new(Theme::dark()), false, InputMode::Normal)
    }

    fn key(code: KeyCode) -> Option<TuiMsg> {
        dialog().handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn typing_filters_the_rows() {
        let mut d = dialog();
        let total = d.rows.len();
        assert!(total > 0, "help must ship rows to search");
        for ch in "panel".chars() {
            assert!(d
                .handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .is_none());
        }
        assert_eq!(d.query(), "panel");
        let hits = d.visible_rows();
        assert!(!hits.is_empty(), "\"panel\" must match something");
        assert!(
            hits.len() < total,
            "a filter must narrow, got {}/{}",
            hits.len(),
            total
        );
    }

    #[test]
    fn filtering_is_case_insensitive() {
        let mut d = dialog();
        for ch in "PANEL".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert!(!d.visible_rows().is_empty());
    }

    #[test]
    fn multiple_terms_narrow_rather_than_widen() {
        let mut d = dialog();
        for ch in "ctrl alt".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(d.query(), "ctrl alt");
        let both = d.visible_rows().len();
        for ch in "panel".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert!(
            d.visible_rows().len() <= both,
            "adding a term must not widen the result set"
        );
    }

    #[test]
    fn filter_matches_the_key_field() {
        let mut d = dialog();
        for ch in "ctrl+alt+w".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let hits = d.visible_rows();
        assert!(
            hits.iter().any(|r| r.key == "Ctrl+Alt+W"),
            "the switcher chord must be findable by name, got {:?}",
            hits.iter().map(|r| &r.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_switcher_is_reachable_through_search() {
        // The chord that used to be undecodable must be discoverable in the
        // help surface, or the fix is undiscoverable.
        let mut d = dialog();
        // A real space, so the query is two ANDed terms ("side" and "panel")
        // rather than one unmatchable literal.
        for ch in "side panel".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(d.query(), "side panel");
        assert!(
            d.visible_rows().iter().any(|r| r.key == "Ctrl+Alt+W"),
            "searching \"side panel\" must surface the panel switcher, got {:?}",
            d.visible_rows().iter().map(|r| &r.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn backspace_walks_the_query_back() {
        let mut d = dialog();
        for ch in "pane".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        d.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(d.query(), "pan");
        for _ in 0..5 {
            d.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        assert_eq!(d.query(), "");
    }

    #[test]
    fn an_empty_query_restores_every_row() {
        let mut d = dialog();
        let total = d.rows.len();
        for ch in "panel".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        for _ in 0..10 {
            d.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        assert_eq!(d.visible_rows().len(), total);
    }

    #[test]
    fn a_query_with_no_match_yields_nothing() {
        let mut d = dialog();
        for ch in "zzzznotakey".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert!(d.visible_rows().is_empty());
    }

    #[test]
    fn the_query_is_bounded() {
        let mut d = dialog();
        for _ in 0..MAX_QUERY_LEN + 40 {
            d.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        }
        assert_eq!(d.query().chars().count(), MAX_QUERY_LEN);
    }

    #[test]
    fn esc_clears_the_filter_before_closing() {
        let mut d = dialog();
        for ch in "panel".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // First Esc clears, and must NOT close.
        assert!(
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .is_none(),
            "the first Esc must clear the filter, not close the dialog"
        );
        assert_eq!(d.query(), "");
        // Second Esc closes.
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn esc_on_an_empty_query_closes_immediately() {
        assert_eq!(key(KeyCode::Esc), Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn vim_only_entries_are_hidden_when_vim_mode_is_off() {
        // Filtering must not surface an entry the unfiltered view hides.
        let d = dialog();
        assert!(
            d.rows.iter().all(|r| r
                .condition
                .as_deref()
                .is_none_or(|c| !c.contains("vim mode"))),
            "vim-gated entries leaked into the searchable rows"
        );
    }

    #[test]
    fn vim_entries_appear_when_vim_mode_is_on() {
        let d = HelpDialog::new_with_mode(Arc::new(Theme::dark()), true, InputMode::Normal);
        assert!(
            d.rows.iter().any(|r| r
                .condition
                .as_deref()
                .is_some_and(|c| c.contains("vim mode"))),
            "vim mode must surface the vim-gated entries"
        );
    }

    #[test]
    fn printable_letters_filter_instead_of_scrolling() {
        // `j`/`k` must be typeable to search for "jump"; arrows still scroll.
        let mut d = dialog();
        assert!(d.scroll == 0);
        d.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(d.query(), "jk");
        assert_eq!(d.scroll, 0, "typing must not move the viewport");
    }

    #[test]
    fn arrows_scroll_the_filtered_results() {
        let mut d = dialog();
        d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(d.scroll, 1);
        d.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(d.scroll, 0);
    }

    #[test]
    fn scrolling_clamps_to_the_result_set() {
        let mut d = dialog();
        for _ in 0..500 {
            d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(d.scroll, d.result_count().saturating_sub(1));
        for _ in 0..500 {
            d.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        }
        assert_eq!(d.scroll, 0);
    }

    #[test]
    fn a_new_query_resets_the_scroll_to_the_top() {
        let mut d = dialog();
        for _ in 0..20 {
            d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert!(d.scroll > 0);
        d.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(
            d.scroll, 0,
            "a narrowing query must not stay scrolled past its end"
        );
    }

    #[test]
    fn control_modified_keys_do_not_pollute_the_query() {
        let mut d = dialog();
        d.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(d.query(), "", "Ctrl+D must not be typed into the filter");
    }

    #[test]
    fn pre_rendered_lines_have_no_rows_and_stay_unfiltered() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
        let d = HelpDialog::new(Arc::new(Theme::dark()), lines);
        assert!(d.rows.is_empty());
        assert_eq!(d.result_count(), 20);
    }
}
