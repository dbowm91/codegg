use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;
use crate::session::Session;
use crate::tui::app::TuiMsg;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortBy {
    #[default]
    Date,
    Title,
    Activity,
}

type SortedCache = Option<(String, bool, SortBy, Vec<usize>)>;

impl SortBy {
    fn next(&self) -> Self {
        match self {
            Self::Date => Self::Title,
            Self::Title => Self::Activity,
            Self::Activity => Self::Date,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Title => "title",
            Self::Activity => "activity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DatePreset {
    #[default]
    All,
    Today,
    Last7Days,
    Last30Days,
    ThisMonth,
}

impl DatePreset {
    fn next(&self) -> Self {
        match self {
            Self::All => Self::Today,
            Self::Today => Self::Last7Days,
            Self::Last7Days => Self::Last30Days,
            Self::Last30Days => Self::ThisMonth,
            Self::ThisMonth => Self::All,
        }
    }

    #[allow(dead_code)]
    fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Today => "today",
            Self::Last7Days => "7d",
            Self::Last30Days => "30d",
            Self::ThisMonth => "month",
        }
    }
}

pub struct SessionDialog {
    pub theme: Arc<Theme>,
    pub filter: String,
    pub selected: usize,
    pub sessions: Vec<Session>,
    pub sort_by: SortBy,
    pub date_preset: DatePreset,
    pub show_archived: bool,
    pub message_counts: HashMap<String, usize>,
    pub search_messages: bool,
    pub message_preview: HashMap<String, String>,
    pub loading: bool,
    pub scroll: CenteredScroll,
    pub bulk_mode: bool,
    pub selected_ids: HashMap<String, bool>,
    visible_height: usize,
    sorted_cache: RefCell<SortedCache>,
}

impl Clone for SessionDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            filter: self.filter.clone(),
            selected: self.selected,
            sessions: self.sessions.clone(),
            sort_by: self.sort_by,
            date_preset: self.date_preset,
            show_archived: self.show_archived,
            message_counts: self.message_counts.clone(),
            search_messages: self.search_messages,
            message_preview: self.message_preview.clone(),
            loading: self.loading,
            scroll: self.scroll.clone(),
            bulk_mode: self.bulk_mode,
            selected_ids: self.selected_ids.clone(),
            visible_height: self.visible_height,
            sorted_cache: RefCell::new(self.sorted_cache.borrow().clone()),
        }
    }
}

impl SessionDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            filter: String::new(),
            selected: 0,
            sessions: Vec::new(),
            sort_by: SortBy::Date,
            date_preset: DatePreset::All,
            show_archived: false,
            message_counts: HashMap::new(),
            search_messages: false,
            message_preview: HashMap::new(),
            loading: false,
            scroll: CenteredScroll::new(),
            bulk_mode: false,
            selected_ids: HashMap::new(),
            visible_height: 10,
            sorted_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn cycle_date_preset(&mut self) {
        self.date_preset = self.date_preset.next();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    pub fn load_sessions(&mut self, sessions: Vec<Session>) {
        self.sessions = sessions;
        self.selected = 0;
        self.filter.clear();
        self.message_counts.clear();
        self.loading = false;
        self.scroll.reset();
        self.sorted_cache = RefCell::new(None);
    }

    pub fn set_message_count(&mut self, session_id: &str, count: usize) {
        self.message_counts.insert(session_id.to_string(), count);
        if self.sort_by == SortBy::Activity {
            self.sorted_cache = RefCell::new(None);
        }
    }

    pub fn search_label(&self) -> &'static str {
        if self.search_messages {
            "messages"
        } else {
            "title"
        }
    }

    pub fn toggle_search_mode(&mut self) {
        self.search_messages = !self.search_messages;
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn set_message_preview(&mut self, session_id: &str, preview: String) {
        self.message_preview.insert(session_id.to_string(), preview);
    }

    pub fn clear_message_preview(&mut self) {
        self.message_preview.clear();
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.sorted_sessions().get(self.selected).copied()
    }

    fn update_cache(&self) {
        let mut sessions: Vec<&Session> = if self.filter.is_empty() {
            self.sessions.iter().collect()
        } else {
            let lower = self.filter.to_lowercase();
            self.sessions
                .iter()
                .filter(|s| s.title.to_lowercase().contains(&lower))
                .collect()
        };

        if !self.show_archived {
            sessions.retain(|s| s.time_archived.is_none());
        }

        match self.sort_by {
            SortBy::Date => sessions.sort_by_key(|b| std::cmp::Reverse(b.time_updated)),
            SortBy::Title => sessions.sort_by_key(|a| a.title.to_lowercase()),
            SortBy::Activity => {
                sessions.sort_by(|a, b| {
                    let a_count = self.message_counts.get(&a.id).unwrap_or(&0);
                    let b_count = self.message_counts.get(&b.id).unwrap_or(&0);
                    b_count
                        .cmp(a_count)
                        .then_with(|| b.time_updated.cmp(&a.time_updated))
                });
            }
        }

        let index_map: std::collections::HashMap<&str, usize> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.as_str(), i))
            .collect();

        let indices: Vec<usize> = sessions
            .iter()
            .filter_map(|s| index_map.get(s.id.as_str()).copied())
            .collect();

        *self.sorted_cache.borrow_mut() = Some((
            self.filter.clone(),
            self.show_archived,
            self.sort_by,
            indices,
        ));
    }

    fn sorted_sessions(&self) -> Vec<&Session> {
        let cache_valid = {
            self.sorted_cache
                .borrow()
                .as_ref()
                .map(|(f, a, s, _)| {
                    f == &self.filter && *a == self.show_archived && *s == self.sort_by
                })
                .unwrap_or(false)
        };

        if !cache_valid {
            self.update_cache();
        }

        if let Some((_, _, _, ref indices)) = *self.sorted_cache.borrow() {
            indices
                .iter()
                .filter_map(|&i| self.sessions.get(i))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        let scroll = self.scroll.get();
        let len = self.sorted_sessions().len();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll.clamp(self.selected, len, self.visible_height);
        }
    }

    pub fn select_down(&mut self) {
        let len = self.sorted_sessions().len();
        if len > 0 && self.selected + 1 < len {
            self.selected += 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll.clamp(self.selected, len, self.visible_height);
        }
    }

    pub fn set_filter(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn cycle_sort(&mut self) {
        self.sort_by = self.sort_by.next();
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn toggle_show_archived(&mut self) {
        self.show_archived = !self.show_archived;
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn sort_label(&self) -> &'static str {
        self.sort_by.label()
    }

    pub fn toggle_bulk_mode(&mut self) {
        self.bulk_mode = !self.bulk_mode;
        if !self.bulk_mode {
            self.selected_ids.clear();
        }
        self.sorted_cache = RefCell::new(None);
    }

    pub fn toggle_selection(&mut self) {
        if let Some(s) = self.sorted_sessions().get(self.selected) {
            let id = s.id.clone();
            let entry = self.selected_ids.entry(id).or_insert(false);
            *entry = !*entry;
        }
    }

    pub fn select_all(&mut self) {
        let ids: Vec<String> = self
            .sorted_sessions()
            .iter()
            .map(|s| s.id.clone())
            .collect();
        for id in ids {
            self.selected_ids.insert(id, true);
        }
    }

    pub fn deselect_all(&mut self) {
        self.selected_ids.clear();
    }

    pub fn selected_count(&self) -> usize {
        self.selected_ids.values().filter(|&&v| v).count()
    }

    pub fn get_selected_ids(&self) -> Vec<String> {
        self.selected_ids
            .iter()
            .filter(|(_, &v)| v)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

fn highlight_match<'a>(text: &'a str, query: &str, theme: &Theme) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::raw(text)];
    }

    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut result = Vec::new();
    let mut last_end = 0;
    let query_byte_len = lower_query.len();

    for match_start in lower_text.match_indices(&lower_query) {
        let start = match_start.0;
        if start > last_end {
            result.push(Span::raw(&text[last_end..start]));
        }
        result.push(Span::styled(
            &text[start..start + query_byte_len],
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
        last_end = start + query_byte_len;
    }

    if last_end < text.len() {
        result.push(Span::raw(&text[last_end..]));
    }

    result
}

impl Default for SessionDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Component for SessionDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.bulk_mode {
                    self.toggle_bulk_mode();
                    None
                } else {
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if self.bulk_mode {
                    self.toggle_bulk_mode();
                    None
                } else {
                    self.selected_session()
                        .cloned()
                        .map(|session| TuiMsg::SelectSession(Box::new(session)))
                }
            }
            crossterm::event::KeyCode::Char('d') if !self.bulk_mode && self.filter.is_empty() => {
                if let Some(session) = self.selected_session() {
                    return Some(TuiMsg::ConfirmDeleteSession {
                        session_id: session.id.clone(),
                    });
                }
                None
            }
            crossterm::event::KeyCode::Char('a') if !self.bulk_mode && self.filter.is_empty() => {
                if let Some(session) = self.selected_session() {
                    return Some(TuiMsg::ConfirmArchiveSession {
                        session_id: session.id.clone(),
                        unarchive: session.time_archived.is_some(),
                    });
                }
                None
            }
            crossterm::event::KeyCode::Char('f') => {
                if let Some(session) = self.selected_session() {
                    return Some(TuiMsg::ForkSession {
                        session_id: session.id.clone(),
                    });
                }
                None
            }
            crossterm::event::KeyCode::Char('s') => {
                self.cycle_sort();
                None
            }
            crossterm::event::KeyCode::Char('h') => {
                self.toggle_show_archived();
                None
            }
            crossterm::event::KeyCode::Char('b') => {
                self.toggle_bulk_mode();
                None
            }
            crossterm::event::KeyCode::Char('m') => {
                self.toggle_search_mode();
                None
            }
            crossterm::event::KeyCode::Char(' ') if self.bulk_mode => {
                self.toggle_selection();
                None
            }
            crossterm::event::KeyCode::Char('A') if self.bulk_mode => {
                self.select_all();
                None
            }
            crossterm::event::KeyCode::Char('D') if self.bulk_mode => {
                self.deselect_all();
                None
            }
            crossterm::event::KeyCode::Char('a') if self.bulk_mode => {
                let session_ids = self.get_selected_ids();
                let count = session_ids.len();
                if count > 0 {
                    Some(TuiMsg::ConfirmBulkArchive {
                        count,
                        unarchive: false,
                        session_ids,
                    })
                } else {
                    None
                }
            }
            crossterm::event::KeyCode::Char('d') if self.bulk_mode => {
                let session_ids = self.get_selected_ids();
                let count = session_ids.len();
                if count > 0 {
                    Some(TuiMsg::ConfirmBulkDelete { count, session_ids })
                } else {
                    None
                }
            }
            crossterm::event::KeyCode::Char(c) if !self.bulk_mode => {
                self.set_filter(c);
                None
            }
            crossterm::event::KeyCode::Backspace if !self.bulk_mode => {
                self.backspace_filter();
                None
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.filter.push_str(&text);
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    #[allow(clippy::incompatible_msrv)]
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.set_theme(theme);
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Sessions ",
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
        }

        let sort_indicator = if self.show_archived {
            " [archived]"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled("sort: ", Style::default().fg(theme.muted)),
            Span::raw(self.sort_label()),
            Span::raw(sort_indicator),
        ]));
        lines.push(Line::from(vec![
            Span::styled("search: ", Style::default().fg(theme.muted)),
            Span::raw(self.search_label()),
        ]));
        lines.push(Line::from(""));

        let sorted = self.sorted_sessions();
        let scroll = self.scroll.get();

        if sorted.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no sessions loaded)",
                Style::default().fg(theme.muted),
            )));
        } else {
            let mut row_idx = 0usize;
            for (i, s) in sorted.iter().enumerate() {
                if i < scroll {
                    continue;
                }
                let is_archived = s.time_archived.is_some();
                let is_forked = s.parent_id.is_some();
                let style = if i == self.selected {
                    theme.selection_style()
                } else {
                    let bg = if row_idx.is_multiple_of(2) {
                        theme.background
                    } else {
                        theme.alternate_bg
                    };
                    if is_archived {
                        Style::default().fg(theme.muted).bg(bg)
                    } else {
                        Style::default().fg(theme.foreground).bg(bg)
                    }
                };
                row_idx += 1;

                let ts = chrono::DateTime::from_timestamp_millis(s.time_updated)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let msg_count = self.message_counts.get(&s.id).unwrap_or(&0);

                let title_suffix = if is_forked {
                    if is_archived {
                        " [archived]"
                    } else {
                        " (fork)"
                    }
                } else if is_archived {
                    " [archived]"
                } else {
                    ""
                };

                let display_title = if title_suffix.is_empty() {
                    s.title.clone()
                } else {
                    format!("{}{}", s.title, title_suffix)
                };

                let checkbox = if self.bulk_mode {
                    if *self.selected_ids.get(&s.id).unwrap_or(&false) {
                        "[✓] "
                    } else {
                        "[ ] "
                    }
                } else {
                    "  "
                };

                lines.push(Line::from(Span::styled(
                    format!("{}{}  {}msgs  {}", checkbox, display_title, msg_count, ts),
                    style,
                )));
            }
        }

        lines.push(Line::from(""));

        if self.bulk_mode {
            let count = self.selected_count();
            lines.push(Line::from(Span::styled(
                format!(
                    "↑/↓ navigate  Space select  Esc exit bulk mode  ({} selected)",
                    count
                ),
                Style::default().fg(theme.muted),
            )));
            lines.push(Line::from(Span::styled(
                "a archive  d delete  e export  A select all  D deselect all",
                Style::default().fg(theme.muted),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "↑/↓ navigate  Enter open  Esc cancel  b bulk mode",
                Style::default().fg(theme.muted),
            )));
            lines.push(Line::from(Span::styled(
                "d delete  a archive  f fork  s sort  h toggle archived  m search messages",
                Style::default().fg(theme.muted),
            )));
        }

        if self.search_messages && !self.filter.is_empty() {
            if let Some(selected_s) = sorted.get(self.selected) {
                if let Some(preview) = self.message_preview.get(&selected_s.id) {
                    let lines_preview: Vec<&str> = preview.lines().take(3).collect();
                    for (i, line) in lines_preview.iter().enumerate() {
                        let highlighted = highlight_match(line, &self.filter, theme);
                        if i == 0 {
                            let mut line_parts =
                                vec![Span::styled("Contains: ", Style::default().fg(theme.muted))];
                            line_parts.extend(highlighted);
                            lines.push(Line::from(line_parts));
                        } else {
                            let mut line_parts = vec![Span::styled(
                                "           ",
                                Style::default().fg(theme.muted),
                            )];
                            line_parts.extend(highlighted);
                            lines.push(Line::from(line_parts));
                        }
                    }
                }
            }
        }

        let block = Block::default()
            .title(" Sessions ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Session
    }
}
