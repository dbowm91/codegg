use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::theme::Theme;

pub struct HelpOverlay {
    pub theme: Arc<Theme>,
    pub bindings: Vec<(String, String)>,
    pub scroll: usize,
    pub visible_height: usize,
}

impl HelpOverlay {
    pub fn new(theme: Arc<Theme>) -> Self {
        let bindings = Self::load_bindings();
        Self {
            theme,
            bindings,
            scroll: 0,
            visible_height: 20,
        }
    }

    fn load_bindings() -> Vec<(String, String)> {
        use crate::tui::input::InputAction;

        let default_bindings = crate::tui::input::default_bindings();
        let mut pairs: Vec<(String, String)> = default_bindings
            .iter()
            .filter_map(|((mods, code), action)| {
                let action_str = match action {
                    InputAction::Send => Some("Send prompt"),
                    InputAction::Newline => Some("New line"),
                    InputAction::Cancel => Some("Cancel / Close"),
                    InputAction::NavigateUp => Some("Navigate up"),
                    InputAction::NavigateDown => Some("Navigate down"),
                    InputAction::SwitchAgent => Some("Switch agent"),
                    InputAction::SelectModel => Some("Select model"),
                    InputAction::ClearSession => Some("Clear session"),
                    InputAction::NewSession => Some("New session"),
                    InputAction::ToggleSidebar => Some("Toggle sidebar"),
                    InputAction::ToggleSection => Some("Toggle section"),
                    InputAction::CloseSession => Some("Close session"),
                    InputAction::Help => Some("Show help"),
                    InputAction::FocusPrompt => Some("Focus prompt"),
                    InputAction::StashPrompt => Some("Stash prompt"),
                    InputAction::RestorePrompt => Some("Restore prompt"),
                    InputAction::CopyMessage => Some("Copy message"),
                    InputAction::CycleModelForward => Some("Cycle model forward"),
                    InputAction::CycleModelBackward => Some("Cycle model backward"),
                    InputAction::ToggleReasoning => Some("Toggle reasoning"),
                    InputAction::Quit => Some("Quit"),
                    InputAction::ExternalEditor => Some("External editor"),
                    InputAction::Backspace => Some("Backspace"),
                    InputAction::Delete => Some("Delete"),
                    InputAction::Left => Some("Move left"),
                    InputAction::Right => Some("Move right"),
                    InputAction::Home => Some("Home"),
                    InputAction::End => Some("End"),
                    InputAction::PageUp => Some("Page up"),
                    InputAction::PageDown => Some("Page down"),
                    InputAction::Search => Some("Search"),
                    InputAction::SearchNext => Some("Search next"),
                    InputAction::SearchPrev => Some("Search previous"),
                    InputAction::ClearSearch => Some("Clear search"),
                    InputAction::Command => Some("Command mode"),
                    InputAction::ToggleTts => Some("Toggle TTS"),
                    InputAction::StopTts => Some("Stop TTS"),
                    InputAction::ToggleFullscreen => Some("Toggle fullscreen"),
                    InputAction::ToggleSelect => Some("Toggle select"),
                    InputAction::SelectAll => Some("Select all"),
                    InputAction::DeselectAll => Some("Deselect all"),
                    InputAction::Char(_) => None,
                }?;

                let key_str = format_key_combo(*mods, *code);
                Some((key_str, action_str.to_string()))
            })
            .collect();

        pairs.sort_by(|a, b| a.1.cmp(&b.1));
        pairs
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.bindings.len().saturating_sub(self.visible_height);
        if self.scroll < max_scroll {
            self.scroll += 1;
        }
    }

    pub fn page_up(&mut self) {
        let new_scroll = self.scroll.saturating_sub(self.visible_height);
        self.scroll = new_scroll;
    }

    pub fn page_down(&mut self) {
        let max_scroll = self.bindings.len().saturating_sub(self.visible_height);
        let new_scroll = self.scroll.saturating_add(self.visible_height);
        self.scroll = new_scroll.min(max_scroll);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        let max_scroll = self.bindings.len().saturating_sub(self.visible_height);
        self.scroll = max_scroll;
    }
}

fn format_key_combo(mods: KeyModifiers, code: KeyCode) -> String {
    let mut parts = Vec::new();

    if mods.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if mods.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    if mods.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }

    let key_part = match code {
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Char(c) => c.to_uppercase().to_string(),
        KeyCode::F(n) => format!("F{}", n),
        _ => "?".to_string(),
    };
    parts.push(key_part);

    parts.join("+")
}

impl HelpOverlay {
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let width = (area.width as f64 * 0.6).ceil() as u16;
        let height = (area.height as f64 * 0.7).ceil() as u16;

        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;

        let rect = Rect::new(x, y, width, height);

        let max_scroll = self.bindings.len().saturating_sub(self.visible_height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }

        let header_block = Block::default()
            .title(" Keyboard Shortcuts ")
            .title_style(Style::default().bold().fg(self.theme.primary))
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(self.theme.border));

        let visible_bindings: Vec<Line> = self
            .bindings
            .iter()
            .skip(self.scroll)
            .take(self.visible_height as usize)
            .map(|(key, desc)| {
                let key_width = 18;
                let key_padded = if key.len() < key_width {
                    format!("{}{}", key, " ".repeat(key_width - key.len()))
                } else {
                    key[..key_width].to_string()
                };
                Line::from(vec![
                    Span::styled(key_padded, Style::default().fg(self.theme.secondary).bold()),
                    Span::styled(desc, Style::default().fg(self.theme.foreground)),
                ])
            })
            .collect();

        let content_block = Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(self.theme.border));

        let footer_text = if self.bindings.len() > self.visible_height as usize {
            format!(
                " Scroll: ↑/↓ j/k  Page: PgUp/PgDn  Home/End  [{}..{}] / {} ",
                self.scroll + 1,
                (self.scroll + self.visible_height as usize).min(self.bindings.len()),
                self.bindings.len()
            )
        } else {
            " Press Esc or ? to close ".to_string()
        };
        let footer_block = Block::default()
            .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(self.theme.border));

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(rect);

        frame.render_widget(header_block, chunks[0]);
        frame.render_widget(
            Paragraph::new(visible_bindings).block(content_block.clone()),
            chunks[1],
        );
        frame.render_widget(
            Paragraph::new(footer_text).block(footer_block),
            chunks[2],
        );
    }
}