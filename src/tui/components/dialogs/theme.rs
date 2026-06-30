use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;
use crate::tui::app::TuiMsg;

/// State machine for the live-preview flow. The dialog starts `Inactive`
/// and the previewed theme equals the last committed theme. The first
/// navigation key flips it to `Previewing` and remembers the original so
/// we can revert on Esc or dialog close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewState {
    Inactive,
    Previewing { original_id: String },
}

#[derive(Clone)]
pub struct ThemePickerDialog {
    pub theme: Arc<Theme>,
    pub themes: Vec<Theme>,
    pub selected: usize,
    pub preview_theme: Theme,
    pub scroll: CenteredScroll,
    visible_height: usize,
    pub preview_state: PreviewState,
}

impl ThemePickerDialog {
    /// Construct a picker. The `themes` list is the snapshot the picker
    /// operates on; if `None`, the picker falls back to the registry's
    /// current built-ins.
    pub fn new(theme: Arc<Theme>) -> Self {
        let themes = super::super::super::theme::all_themes();
        let default_idx = themes
            .iter()
            .position(|t| t.name == crate::theme::registry::DEFAULT_THEME_ID)
            .unwrap_or(0);
        Self {
            theme,
            themes,
            selected: default_idx,
            preview_theme: Theme::dark(),
            scroll: CenteredScroll::new(),
            visible_height: 10,
            preview_state: PreviewState::Inactive,
        }
    }

    /// Construct a picker from a caller-supplied list of themes. The
    /// currently-active theme is captured as the preview state, so we
    /// can revert on Esc or dialog close.
    pub fn with_themes(theme: Arc<Theme>, themes: Vec<Theme>) -> Self {
        let default_idx = themes
            .iter()
            .position(|t| t.name == theme.name)
            .or_else(|| {
                themes
                    .iter()
                    .position(|t| t.name == crate::theme::registry::DEFAULT_THEME_ID)
            })
            .unwrap_or(0);
        let preview = themes.get(default_idx).cloned().unwrap_or_else(Theme::dark);
        Self {
            theme: Arc::clone(&theme),
            themes,
            selected: default_idx,
            preview_theme: preview,
            scroll: CenteredScroll::new(),
            visible_height: 10,
            preview_state: PreviewState::Inactive,
        }
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn initialize_selection(&mut self) {
        if let Some(idx) = self.themes.iter().position(|t| t.name == self.theme.name) {
            self.selected = idx;
            self.preview_theme = self.themes[idx].clone();
            let visible_themes = self.count_visible_themes(0);
            self.scroll
                .clamp(self.selected, self.themes.len(), visible_themes);
        } else if !self.themes.is_empty() {
            let visible_themes = self.count_visible_themes(0);
            if visible_themes > 0 {
                self.selected = visible_themes / 2;
                self.scroll
                    .clamp(self.selected, self.themes.len(), visible_themes);
                self.preview_theme = self.themes[self.selected].clone();
            }
        }
    }

    fn count_visible_themes(&self, start_idx: usize) -> usize {
        self.count_visible_themes_in(start_idx, self.visible_height)
    }

    /// Same as [`count_visible_themes`](Self::count_visible_themes) but
    /// accepts an explicit line budget. The render methods use the
    /// actual rendered area's inner height so the picker fills the box
    /// instead of being capped at the default [`visible_height`](Self::visible_height).
    fn count_visible_themes_in(&self, start_idx: usize, max_lines: usize) -> usize {
        let mut lines_used = 0;
        let mut themes_shown = 0;

        for (i, _theme) in self.themes.iter().enumerate().skip(start_idx) {
            let theme_lines = 2 + if i == self.selected { 1 } else { 0 };
            if lines_used + theme_lines > max_lines {
                break;
            }

            lines_used += theme_lines;
            themes_shown += 1;
        }

        themes_shown
    }

    pub fn select_up(&mut self) -> Option<String> {
        if self.selected > 0 {
            self.selected -= 1;
            self.preview_theme = self.themes[self.selected].clone();
        }
        let scroll = self.scroll.get();
        let visible_themes = self.count_visible_themes(scroll);
        if self.selected < scroll || self.selected >= scroll + visible_themes {
            self.scroll
                .clamp(self.selected, self.themes.len(), visible_themes);
        }
        self.transition_to_previewing()
    }

    pub fn select_down(&mut self) -> Option<String> {
        if self.selected + 1 < self.themes.len() {
            self.selected += 1;
            self.preview_theme = self.themes[self.selected].clone();
        }
        let scroll = self.scroll.get();
        let visible_themes = self.count_visible_themes(scroll);
        if self.selected < scroll || self.selected >= scroll + visible_themes {
            self.scroll
                .clamp(self.selected, self.themes.len(), visible_themes);
        }
        self.transition_to_previewing()
    }

    /// Called by App immediately after `select_up`/`select_down`. Returns
    /// the id that should be live-previewed. The first navigation also
    /// flips the preview state from `Inactive` to `Previewing` and
    /// captures the original theme id so we can revert.
    pub fn transition_to_previewing(&mut self) -> Option<String> {
        if self.themes.is_empty() {
            return None;
        }
        let new_id = self.themes[self.selected].name.clone();
        if matches!(self.preview_state, PreviewState::Inactive) {
            self.preview_state = PreviewState::Previewing {
                original_id: self.theme.name.clone(),
            };
        }
        Some(new_id)
    }

    /// The id of the theme that was active when the dialog opened, or
    /// `None` if no preview is in progress.
    pub fn preview_original_id(&self) -> Option<String> {
        match &self.preview_state {
            PreviewState::Inactive => None,
            PreviewState::Previewing { original_id } => Some(original_id.clone()),
        }
    }

    /// True iff the user has navigated since the dialog opened.
    pub fn is_previewing(&self) -> bool {
        matches!(self.preview_state, PreviewState::Previewing { .. })
    }

    pub fn selected_theme(&self) -> Option<&Theme> {
        self.themes.get(self.selected)
    }
}

impl Default for ThemePickerDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

fn color_to_rgb_string(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("{:02x}{:02x}{:02x}", r, g, b),
        Color::Reset => "reset".to_string(),
        Color::Black => "000000".to_string(),
        Color::White => "ffffff".to_string(),
        Color::Red => "ff0000".to_string(),
        Color::Green => "00ff00".to_string(),
        Color::Yellow => "ffff00".to_string(),
        Color::Blue => "0000ff".to_string(),
        Color::Magenta => "ff00ff".to_string(),
        Color::Cyan => "00ffff".to_string(),
        Color::Gray => "808080".to_string(),
        Color::DarkGray => "404040".to_string(),
        Color::LightRed => "ff8080".to_string(),
        Color::LightGreen => "80ff80".to_string(),
        Color::LightYellow => "ffff80".to_string(),
        Color::LightBlue => "8080ff".to_string(),
        Color::LightMagenta => "ff80ff".to_string(),
        Color::LightCyan => "80ffff".to_string(),
        _ => "808080".to_string(),
    }
}

fn render_color_swatch(color: Color, theme: &Theme) -> Span<'static> {
    let hex = color_to_rgb_string(color);
    Span::styled(
        format!(" {} ", hex),
        Style::default()
            .fg(color)
            .bg(theme.background)
            .add_modifier(Modifier::BOLD),
    )
}

fn push_theme_row<'a>(lines: &mut Vec<Line<'a>>, theme: &'a Theme, chrome: &Theme, selected: bool) {
    let style = if selected {
        Style::default()
            .fg(chrome.primary)
            .bg(chrome.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(chrome.foreground)
    };
    let marker = if selected { "▸ " } else { "  " };

    lines.push(Line::from(vec![
        Span::styled(marker.to_string(), Style::default().fg(chrome.muted)),
        Span::styled(theme.name.as_str(), style),
        Span::raw("  "),
    ]));
    lines.push(Line::from(vec![
        Span::raw("     "),
        Span::raw(" "),
        render_color_swatch(theme.primary, chrome),
        render_color_swatch(theme.secondary, chrome),
        render_color_swatch(theme.success, chrome),
        render_color_swatch(theme.warning, chrome),
        render_color_swatch(theme.error, chrome),
    ]));
}

impl Widget for &ThemePickerDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Theme ",
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Reserve 2 lines for the top + bottom of the surrounding
        // `Block` border and 4 lines for the dialog's own header (title
        // + spacer) and footer (spacer + hint). The remaining height
        // drives how many theme entries the picker shows so it fills
        // the box regardless of the default `visible_height` value.
        let max_lines = (area.height as usize).saturating_sub(6);
        let scroll = self.scroll.get();
        let visible_themes = self.count_visible_themes_in(scroll, max_lines);
        for (i, theme) in self
            .themes
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_themes)
        {
            push_theme_row(&mut lines, theme, &self.theme, i == self.selected);
        }

        lines.push(Line::from(""));
        let footer = if self.is_previewing() {
            "j/k/↑/↓ preview  |  Enter commit  |  Esc revert"
        } else {
            "j/k/↑/↓ navigate  |  Enter apply  |  Esc cancel"
        };
        lines.push(Line::from(Span::styled(
            footer,
            Style::default().fg(self.theme.muted),
        )));

        let block = Block::default()
            .title(" Themes ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for ThemePickerDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                // Revert any live preview, then close.
                if self.is_previewing() {
                    Some(TuiMsg::ThemeRevert)
                } else {
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                let new_id = self.select_up();
                new_id.map(|id| TuiMsg::ThemePreviewChanged { theme_id: id })
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                let new_id = self.select_down();
                new_id.map(|id| TuiMsg::ThemePreviewChanged { theme_id: id })
            }
            crossterm::event::KeyCode::Enter => {
                if let Some(theme) = self.selected_theme() {
                    Some(TuiMsg::ThemeCommit {
                        theme_id: theme.name.clone(),
                    })
                } else {
                    Some(TuiMsg::CloseDialog)
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

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Theme ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // See Widget::render for the line budget rationale. Using the
        // actual area height here makes the picker fill the dialog box
        // even after the inline preview block was removed.
        let max_lines = (area.height as usize).saturating_sub(6);
        let scroll = self.scroll.get();
        let visible_themes = self.count_visible_themes_in(scroll, max_lines);
        for (i, thm) in self
            .themes
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_themes)
        {
            push_theme_row(&mut lines, thm, theme, i == self.selected);
        }

        lines.push(Line::from(""));
        let footer = if self.is_previewing() {
            "j/k/↑/↓ preview  |  Enter commit  |  Esc revert"
        } else {
            "j/k/↑/↓ navigate  |  Enter apply  |  Esc cancel"
        };
        lines.push(Line::from(Span::styled(
            footer,
            Style::default().fg(theme.muted),
        )));

        let block = Block::default()
            .title(" Themes ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_themes(names: &[&str]) -> Vec<Theme> {
        names
            .iter()
            .map(|n| Theme {
                name: (*n).to_string(),
                ..Theme::dark()
            })
            .collect()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn starts_in_inactive_preview_state() {
        let picker =
            ThemePickerDialog::with_themes(Arc::new(Theme::dark()), make_themes(&["a", "b", "c"]));
        assert!(!picker.is_previewing());
        assert_eq!(picker.preview_original_id(), None);
    }

    #[test]
    fn first_navigation_captures_original_and_moves_to_previewing() {
        let mut picker = ThemePickerDialog::with_themes(
            Arc::new(Theme::dark()), // "dark" is the active theme
            make_themes(&["dark", "b", "c"]),
        );
        // The picker opens with the active theme highlighted (idx 0).
        // Move down — should preview `b` and remember `dark` as original.
        let new_id = picker.select_down();
        assert_eq!(new_id.as_deref(), Some("b"));
        assert!(picker.is_previewing());
        assert_eq!(picker.preview_original_id().as_deref(), Some("dark"));
    }

    #[test]
    fn subsequent_navigation_keeps_original() {
        let mut picker = ThemePickerDialog::with_themes(
            Arc::new(Theme::dark()),
            make_themes(&["dark", "b", "c", "d"]),
        );
        let _ = picker.select_down();
        let _ = picker.select_down();
        let _ = picker.select_up();
        // The original id stays the same across many navigations.
        assert!(picker.is_previewing());
        assert_eq!(picker.preview_original_id().as_deref(), Some("dark"));
    }

    #[test]
    fn esc_after_navigation_emits_revert() {
        let mut picker = ThemePickerDialog::with_themes(
            Arc::new(Theme::dark()),
            make_themes(&["dark", "b", "c"]),
        );
        let _ = picker.select_down();
        let msg = picker.handle_key(key(KeyCode::Esc));
        assert!(matches!(msg, Some(TuiMsg::ThemeRevert)));
    }

    #[test]
    fn esc_without_navigation_emits_close_dialog() {
        let picker = ThemePickerDialog::with_themes(
            Arc::new(Theme::dark()),
            make_themes(&["dark", "b", "c"]),
        );
        // No navigation yet — Esc closes the dialog normally.
        let mut picker = picker;
        let msg = picker.handle_key(key(KeyCode::Esc));
        assert!(matches!(msg, Some(TuiMsg::CloseDialog)));
    }

    #[test]
    fn enter_emits_commit() {
        let mut picker = ThemePickerDialog::with_themes(
            Arc::new(Theme::dark()),
            make_themes(&["dark", "b", "c"]),
        );
        let _ = picker.select_down(); // highlight "b"
        let msg = picker.handle_key(key(KeyCode::Enter));
        assert!(matches!(msg, Some(TuiMsg::ThemeCommit { ref theme_id }) if theme_id == "b"));
    }

    #[test]
    fn up_and_down_navigation_emit_preview_changed() {
        let mut picker = ThemePickerDialog::with_themes(
            Arc::new(Theme::dark()),
            make_themes(&["dark", "b", "c"]),
        );
        let msg = picker.handle_key(key(KeyCode::Down));
        assert!(
            matches!(msg, Some(TuiMsg::ThemePreviewChanged { ref theme_id }) if theme_id == "b")
        );
        let msg = picker.handle_key(key(KeyCode::Up));
        assert!(
            matches!(msg, Some(TuiMsg::ThemePreviewChanged { ref theme_id }) if theme_id == "dark")
        );
    }

    #[test]
    fn default_highlight_is_active_theme() {
        // The picker should open with the active theme highlighted, not
        // the default theme id. This way the first Down-key previews
        // the *next* theme rather than jumping to "Cyber Red".
        let picker = ThemePickerDialog::with_themes(
            Arc::new(Theme {
                name: "dracula".into(),
                ..Theme::dark()
            }),
            make_themes(&["dark", "dracula", "midnight", "cyber-red"]),
        );
        let names: Vec<&str> = picker.themes.iter().map(|t| t.name.as_str()).collect();
        let highlighted = &names[picker.selected];
        assert_eq!(*highlighted, "dracula");
    }
}
