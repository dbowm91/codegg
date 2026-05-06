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

#[derive(Clone)]
pub struct ThemePickerDialog {
    pub theme: Arc<Theme>,
    pub themes: Vec<Theme>,
    pub selected: usize,
    pub preview_theme: Theme,
    pub scroll: CenteredScroll,
    visible_height: usize,
}

impl ThemePickerDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        let themes = super::super::super::theme::all_themes();
        let default_idx = themes.iter().position(|t| t.name == "dark").unwrap_or(0);
        Self {
            theme,
            themes,
            selected: default_idx,
            preview_theme: Theme::dark(),
            scroll: CenteredScroll::new(),
            visible_height: 10,
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
        let mut lines_used = 0;
        let mut themes_shown = 0;

        for (i, _theme) in self.themes.iter().enumerate().skip(start_idx) {
            if i < start_idx {
                continue;
            }

            let theme_lines = 2 + if i == self.selected { 1 } else { 0 };
            if lines_used + theme_lines > self.visible_height {
                break;
            }

            lines_used += theme_lines;
            themes_shown += 1;
        }

        themes_shown
    }

    pub fn select_up(&mut self) {
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
    }

    pub fn select_down(&mut self) {
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

        let scroll = self.scroll.get();
        let visible_themes = self.count_visible_themes(scroll);
        for (i, theme) in self.themes.iter().enumerate() {
            if i < scroll {
                continue;
            }
            if i >= scroll + visible_themes {
                break;
            }

            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(self.theme.primary)
                    .bg(self.theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.foreground)
            };

            let swatches = vec![
                render_color_swatch(theme.primary, &self.theme),
                render_color_swatch(theme.secondary, &self.theme),
                render_color_swatch(theme.success, &self.theme),
                render_color_swatch(theme.warning, &self.theme),
                render_color_swatch(theme.error, &self.theme),
            ];

            let marker = if is_selected { "▸ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(self.theme.muted)),
                Span::styled(&theme.name, style),
                Span::raw("  "),
            ]));
            lines.push(Line::from(vec![Span::raw("     "), Span::raw(" ")]));
            for swatch in &swatches {
                lines.last_mut().unwrap().spans.push(swatch.clone());
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Preview ",
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        let preview = &self.preview_theme;
        let preview_lines = vec![
            Line::from(vec![
                Span::styled("┌─", Style::default().fg(preview.border)),
                Span::styled("▌", Style::default().fg(preview.primary)),
                Span::styled(
                    " build ",
                    Style::default().fg(preview.foreground).bg(preview.primary),
                ),
                Span::raw("─────────────────────────────"),
                Span::styled("┐", Style::default().fg(preview.border)),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    " > ",
                    Style::default().fg(preview.muted).bg(preview.background),
                ),
                Span::styled(
                    "How do I exit vim?",
                    Style::default()
                        .fg(preview.foreground)
                        .bg(preview.background),
                ),
                Span::raw("                   │"),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    " To exit vim:  ",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
                Span::raw("                      │"),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    "   ",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
                Span::styled(":q", Style::default().fg(preview.success)),
                Span::styled(
                    " to quit                     │",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    "   ",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
                Span::styled(":wq", Style::default().fg(preview.success)),
                Span::styled(
                    " to save and quit            │",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::raw("                              │"),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(" ⟳ ", Style::default().fg(preview.primary)),
                Span::styled(
                    " Bash running ",
                    Style::default().fg(preview.foreground).bg(preview.primary),
                ),
                Span::raw("                  │"),
            ]),
            Line::from(vec![
                Span::styled("└", Style::default().fg(preview.border)),
                Span::raw("─────────────────────────────"),
                Span::styled("┘", Style::default().fg(preview.border)),
            ]),
        ];
        for line in preview_lines {
            lines.push(line);
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ navigate  Enter apply  Esc cancel",
            Style::default().fg(self.theme.muted),
        )));

        let block = Block::default()
            .title(" Themes ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.primary))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for ThemePickerDialog {
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
                if let Some(theme) = self.selected_theme() {
                    Some(TuiMsg::SelectTheme {
                        theme_name: theme.name.clone(),
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

        let scroll = self.scroll.get();
        let visible_themes = self.count_visible_themes(scroll);
        for (i, thm) in self.themes.iter().enumerate() {
            if i < scroll {
                continue;
            }
            if i >= scroll + visible_themes {
                break;
            }

            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };

            let swatches = vec![
                render_color_swatch(thm.primary, theme),
                render_color_swatch(thm.secondary, theme),
                render_color_swatch(thm.success, theme),
                render_color_swatch(thm.warning, theme),
                render_color_swatch(thm.error, theme),
            ];

            let marker = if is_selected { "▸ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(theme.muted)),
                Span::styled(&thm.name, style),
                Span::raw("  "),
            ]));
            lines.push(Line::from(vec![Span::raw("     "), Span::raw(" ")]));
            for swatch in &swatches {
                lines.last_mut().unwrap().spans.push(swatch.clone());
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Preview ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        let preview = &self.preview_theme;
        let preview_lines = vec![
            Line::from(vec![
                Span::styled("┌─", Style::default().fg(preview.border)),
                Span::styled("▌", Style::default().fg(preview.primary)),
                Span::styled(
                    " build ",
                    Style::default().fg(preview.foreground).bg(preview.primary),
                ),
                Span::raw("─────────────────────────────"),
                Span::styled("┐", Style::default().fg(preview.border)),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    " > ",
                    Style::default().fg(preview.muted).bg(preview.background),
                ),
                Span::styled(
                    "How do I exit vim?",
                    Style::default()
                        .fg(preview.foreground)
                        .bg(preview.background),
                ),
                Span::raw("                   │"),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    " To exit vim:  ",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
                Span::raw("                      │"),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    "   ",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
                Span::styled(":q", Style::default().fg(preview.success)),
                Span::styled(
                    " to quit                     │",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(
                    "   ",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
                Span::styled(":wq", Style::default().fg(preview.success)),
                Span::styled(
                    " to save and quit            │",
                    Style::default()
                        .fg(preview.secondary)
                        .bg(preview.background),
                ),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::raw("                              │"),
            ]),
            Line::from(vec![
                Span::styled("│", Style::default().fg(preview.border)),
                Span::styled(" ⟳ ", Style::default().fg(preview.primary)),
                Span::styled(
                    " Bash running ",
                    Style::default().fg(preview.foreground).bg(preview.primary),
                ),
                Span::raw("                  │"),
            ]),
            Line::from(vec![
                Span::styled("└", Style::default().fg(preview.border)),
                Span::raw("─────────────────────────────"),
                Span::styled("┘", Style::default().fg(preview.border)),
            ]),
        ];
        for line in preview_lines {
            lines.push(line);
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ navigate  Enter apply  Esc cancel",
            Style::default().fg(theme.muted),
        )));

        let block = Block::default()
            .title(" Themes ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Theme
    }
}
