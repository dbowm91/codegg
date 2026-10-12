use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::widgets::{StatefulWidget, Widget};
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

#[derive(Debug)]
pub struct HelpDialog {
    help_lines: Vec<String>,
    rows: Vec<HelpRow>,
    query: String,
    theme: Arc<Theme>,
    scroll: usize,
    /// Whether the filter currently has the keyboard.
    ///
    /// **Insert is the default**, so `/help` opens ready to type: the box is
    /// opened to look something up, and a mode switch before every query
    /// would tax the common case. Normal exists for the gestures that need a
    /// non-typable key — above all the directional that docks this box into
    /// the side panel — because in Insert mode `Shift+L` is the letter `L`
    /// and has to stay typeable so it can be searched for.
    mode: InputMode,
}

impl Clone for HelpDialog {
    fn clone(&self) -> Self {
        Self {
            help_lines: self.help_lines.clone(),
            rows: self.rows.clone(),
            query: self.query.clone(),
            theme: Arc::clone(&self.theme),
            scroll: self.scroll,
            mode: self.mode,
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
            mode: InputMode::Insert,
        }
    }

    /// Create a help dialog with mode-aware content.
    ///
    /// `input_mode` decides two things, and both matter: which section is
    /// listed first, and which mode the filter *starts* in. The second one is
    /// the caller's business, not this widget's — a `/help` box opened from
    /// the composer arrives in Insert because the user typed it, while the
    /// docked panel arrives in Normal because a panel is read and scrolled
    /// before it is filtered. Hardcoding Insert here is what made the panel
    /// swallow `j`/`k` as filter text instead of scrolling.
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
            mode: input_mode,
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

    /// First visible row.
    ///
    /// Exposed alongside [`Self::query`] and [`Self::input_mode`] for the
    /// parity tests: "the panel does not scroll" and "the panel types `j` into
    /// the filter" are the same bug seen from two sides, and only this pair
    /// distinguishes them.
    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }

    /// Which mode the filter is in. Exposed for the parity tests, which
    /// cannot see the badge the render draws.
    pub fn input_mode(&self) -> InputMode {
        self.mode
    }

    /// Return to Insert so the user can type again.
    ///
    /// The counterpart of `Esc`, and what a host calls when it adopts this
    /// widget fresh — a docked reference should behave like a freshly typed
    /// one.
    pub fn enter_insert_mode(&mut self) {
        self.mode = InputMode::Insert;
    }

    /// How many searchable rows exist. Zero for the pre-rendered
    /// constructor, and the signal a host uses to know it still has to build
    /// the real row set on first paint.
    pub fn rows_len(&self) -> usize {
        self.rows.len()
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

    /// Inverse-video `INS`/`NOR` badge, matching the chat box's exactly.
    ///
    /// Deliberately identical rather than merely similar: this badge is one
    /// piece of vocabulary shared by every dockable box, and a user who
    /// learned `NOR` on `/chat` must not have to learn it again here to dock
    /// this box.
    fn mode_badge(&self, theme: &Arc<Theme>) -> Vec<Span<'static>> {
        let (label, colour) = match self.mode {
            InputMode::Normal => ("NOR", theme.muted),
            InputMode::Insert => ("INS", theme.primary),
        };
        vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!(" {label} "),
                Style::default()
                    .fg(theme.background)
                    .bg(colour)
                    .add_modifier(Modifier::BOLD),
            ),
        ]
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
    /// Answers the docking gesture itself, under its own mode check — in
    /// Insert, `Shift+L` is the letter `L` and must reach the filter. The
    /// host's central gate must therefore leave this dialog alone.
    fn docks_as_viewer(&self) -> bool {
        false
    }
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::{KeyCode, KeyModifiers};

        // The directional gesture is a Normal-mode verb only. In Insert,
        // `Shift+L` is the letter L and must stay typeable — it is how you
        // search for the docking key itself. Checked ahead of the match so
        // the printable arm below cannot shadow it with a bare `H`/`J`/`K`/`L`.
        if self.mode == InputMode::Normal {
            if let Some(dir) = crate::tui::panels::direction_of(key) {
                return Some(TuiMsg::MoveDirection(dir));
            }
        }

        match key.code {
            // Paging is live in both modes: arrows and page keys are not
            // text, so there is no reason to take them away while typing.
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-PAGE_STEP),
            KeyCode::PageDown => self.scroll_by(PAGE_STEP),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = self.result_count().saturating_sub(1),
            KeyCode::Backspace if self.mode == InputMode::Insert => self.pop_query(),
            KeyCode::Esc => {
                if self.mode == InputMode::Insert {
                    // Leave Insert the way every other box here does, with
                    // the filter intact — that is where the directional and
                    // the `q` exit live.
                    self.mode = InputMode::Normal;
                } else if self.query.is_empty() {
                    return Some(TuiMsg::CloseDialog);
                } else {
                    // Esc clears an active filter first and closes only when
                    // there is nothing to clear, so one Esc never discards a
                    // query the user is midway through typing.
                    self.query.clear();
                    self.scroll = 0;
                }
            }
            KeyCode::Enter => self.mode = InputMode::Insert,
            KeyCode::Char('q') if self.mode == InputMode::Normal => {
                return Some(TuiMsg::CloseDialog);
            }
            KeyCode::Char('i') | KeyCode::Char('a') if self.mode == InputMode::Normal => {
                self.mode = InputMode::Insert;
            }
            // `j`/`k` are navigation *only* in Normal. While Insert is live
            // they have to be typeable so `help` itself can be searched for.
            KeyCode::Char('j') if self.mode == InputMode::Normal => self.scroll_by(1),
            KeyCode::Char('k') if self.mode == InputMode::Normal => self.scroll_by(-1),
            KeyCode::Char(ch)
                if self.mode == InputMode::Insert
                    && (key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT
                        || key.modifiers.contains(KeyModifiers::ALT)) =>
            {
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
        self.render_into(frame.buffer_mut(), area, theme, " Help ");
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Help
    }
}

// ── Buffer-level rendering ──────────────────────────────────────────

impl HelpDialog {
    /// Render into a raw [`Buffer`].
    ///
    /// Exists so the side panel paints the *same* widget rather than a
    /// parallel implementation: the box and the panel must agree on every
    /// row, on the filter, and on which keys are live, or docking the box
    /// would quietly change the interface. [`Component::render`] is a thin
    /// adapter over this.
    pub fn render_into(&mut self, buf: &mut Buffer, area: Rect, theme: &Arc<Theme>, title: &str) {
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
                .title(title.to_string())
                .border_style(Style::default().fg(theme.border)),
        );
        Widget::render(title, chunks[0], buf);

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
        // The badge rides the filter row because that is the surface it
        // governs: without it, a user in Normal who presses `j` and gets
        // nothing has no way to tell why.
        let mut filter_spans = filter_spans;
        filter_spans.extend(self.mode_badge(theme));
        let filter = Paragraph::new(Line::from(filter_spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if filtering {
                    theme.primary
                } else {
                    theme.border
                })),
        );
        Widget::render(filter, chunks[1], buf);

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
        Widget::render(content, chunks[2], buf);

        let mut scrollbar_state = ScrollbarState::new(total_lines).position(self.scroll);
        StatefulWidget::render(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(theme.primary)),
            chunks[2],
            buf,
            &mut scrollbar_state,
        );

        // ── Footer ──────────────────────────────────────────────────────
        // Mode-specific, because the keys that differ are exactly the ones a
        // user reaches for and then finds do nothing.
        let footer_text = match (self.mode, filtering) {
            (InputMode::Insert, true) => {
                "Esc normal  |  ↑/↓ PgUp/PgDn scroll  |  Backspace edit filter"
            }
            (InputMode::Insert, false) => "type to filter  |  Esc normal  |  ↑/↓ PgUp/PgDn scroll",
            (InputMode::Normal, true) => {
                "i insert  |  ↑/↓ PgUp/PgDn scroll  |  Esc clear filter  |  q close"
            }
            (InputMode::Normal, false) => {
                "i insert  |  ↑/↓ PgUp/PgDn scroll  |  Shift+L dock  |  q close"
            }
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
        Widget::render(footer, chunks[3], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::panels::PanelDirection;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// The `/help` box as it actually opens: **Insert**, ready to type.
    ///
    /// Spelled out rather than left implicit because the starting mode is the
    /// caller's decision, and most of these tests exercise the filter.
    fn dialog() -> HelpDialog {
        HelpDialog::new_with_mode(Arc::new(Theme::dark()), false, InputMode::Insert)
    }

    /// The docked panel as it actually opens: **Normal**, ready to scroll.
    fn normal_dialog() -> HelpDialog {
        HelpDialog::new_with_mode(Arc::new(Theme::dark()), false, InputMode::Normal)
    }

    #[test]
    fn the_starting_mode_is_the_callers_choice() {
        // The box and the panel open in different modes on purpose: the box is
        // opened to type a query, the panel to read the list. Hardcoding
        // either one is what made the docked panel swallow `j` as filter text.
        assert_eq!(dialog().input_mode(), InputMode::Insert);
        assert_eq!(normal_dialog().input_mode(), InputMode::Normal);
    }

    #[test]
    fn a_normal_dialog_scrolls_instead_of_filtering() {
        let mut d = normal_dialog();
        d.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(d.query(), "", "Normal must scroll, not type");
        assert_eq!(d.scroll, 1);
        d.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(d.scroll, 0);
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
        for ch in "shift+s".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let hits = d.visible_rows();
        assert!(
            hits.iter().any(|r| r.key == "Shift+S"),
            "the switcher key must be findable by name, got {:?}",
            hits.iter().map(|r| &r.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_switcher_is_reachable_through_search() {
        // The key that opens the panel manager must be discoverable in the
        // help surface, or the manager is undiscoverable.
        let mut d = dialog();
        // A real space, so the query is two ANDed terms ("side" and "panel")
        // rather than one unmatchable literal.
        for ch in "side panel".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(d.query(), "side panel");
        assert!(
            d.visible_rows().iter().any(|r| r.key == "Shift+S"),
            "searching \"side panel\" must surface the panel switcher, got {:?}",
            d.visible_rows().iter().map(|r| &r.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_retired_switcher_chord_is_not_advertised() {
        // `Ctrl+Alt+W` was retired in favour of `Shift+S`. Help must not
        // keep naming a key that no longer does anything, or it teaches a
        // binding that silently fails.
        let rows: Vec<_> = [
            crate::tui::input::HelpMode::Insert,
            crate::tui::input::HelpMode::Normal,
            crate::tui::input::HelpMode::Command,
            crate::tui::input::HelpMode::Dialog,
        ]
        .into_iter()
        .flat_map(crate::tui::input::help_entries_for_mode)
        .collect();
        assert!(
            !rows.iter().any(|r| r.key.contains("Ctrl+Alt+W")),
            "the retired chord is still advertised: {:?}",
            rows.iter()
                .filter(|r| r.key.contains("Ctrl+Alt+W"))
                .map(|r| &r.key)
                .collect::<Vec<_>>()
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
    fn esc_leaves_insert_then_clears_the_filter_then_closes() {
        let mut d = dialog();
        for ch in "panel".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // Esc from Insert leaves the mode, keeping the filter — that is where
        // the directional that docks this box lives.
        assert!(
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .is_none(),
            "Esc must leave Insert, not close the dialog"
        );
        assert_eq!(d.input_mode(), InputMode::Normal);
        assert_eq!(d.query(), "panel", "the filter survives the mode change");

        // A second Esc clears, and must NOT close.
        assert!(
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .is_none(),
            "the next Esc must clear the filter, not close the dialog"
        );
        assert_eq!(d.query(), "");

        // A third Esc closes.
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn esc_on_an_empty_query_closes_only_once_normal() {
        // One Esc from Insert must not close, even with nothing typed: the
        // user still has to be able to reach Normal mode, which is where
        // `q` and the directional live.
        let mut d = dialog();
        assert!(d
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .is_none());
        assert_eq!(d.input_mode(), InputMode::Normal);
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn q_closes_from_normal_mode_only() {
        // `q` is the Normal-mode exit, the same shape every other panel uses.
        // In Insert it must remain a searchable letter.
        let mut d = dialog();
        assert!(d
            .handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .is_none());
        assert_eq!(d.query(), "q", "q must stay typeable while filtering");
        assert!(d
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .is_none());
        d.query.clear();
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn the_directional_docks_only_from_normal_mode() {
        // In Insert, `L` is the letter L and must be searchable; in Normal it
        // is the gesture that moves this box into the side panel.
        let mut d = dialog();
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)),
            None,
            "Insert must type the letter, not dock"
        );
        assert_eq!(d.query(), "L");

        assert!(d
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .is_none());
        d.query.clear();
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)),
            Some(TuiMsg::MoveDirection(PanelDirection::Right))
        );
    }

    #[test]
    fn j_and_k_navigate_only_in_normal_mode() {
        // They have to be typeable while Insert is live so `help` can be
        // searched for.
        let mut d = dialog();
        d.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(d.query(), "j");
        assert!(d
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .is_none());
        d.query.clear();
        let before = d.scroll;
        d.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(d.scroll > before, "j must move the highlight in Normal");
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
