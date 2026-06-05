use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;
use std::sync::Arc;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::util::clipboard;

#[derive(Clone)]
pub struct ShareDialog {
    pub theme: Arc<Theme>,
    pub share_url: String,
    pub copied: bool,
    qr_code: Option<String>,
    cursor: usize,
}

impl ShareDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            share_url: String::new(),
            copied: false,
            qr_code: None,
            cursor: 0,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_url(&mut self, url: String) {
        self.share_url = url.clone();
        self.copied = false;
        self.qr_code = Self::generate_qr_code(&url);
    }

    pub fn copy_url(&mut self) -> bool {
        if let Err(e) = clipboard::copy_to_clipboard(&self.share_url) {
            tracing::warn!("Failed to copy to clipboard: {}", e);
            return false;
        }
        self.copied = true;
        true
    }

    fn generate_qr_code(data: &str) -> Option<String> {
        if data.is_empty() {
            return None;
        }
        let qr_result = qrcode::QrCode::new(data.as_bytes());
        let qr = match qr_result {
            Ok(qr) => qr,
            Err(e) => {
                tracing::debug!("QR generation failed for '{}': {:?}", data, e);
                return None;
            }
        };
        let size = qr.width();
        if size == 0 || size > 64 {
            tracing::debug!("QR size check failed: size={}", size);
            return None;
        }

        let ascii: String = qr.render().light_color(' ').dark_color('#').build();

        Some(ascii)
    }
}

impl Default for ShareDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &ShareDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                " Share Session ",
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Share this session with others by copying the URL below:",
                Style::default().fg(self.theme.muted),
            )]),
            Line::from(""),
            {
                let url_style = if self.copied {
                    Style::default().fg(self.theme.success)
                } else {
                    Style::default().fg(self.theme.primary)
                };
                Line::from(Span::styled(format!("  {}  ", self.share_url), url_style))
            },
            Line::from(""),
            {
                let btn_text = if self.copied {
                    "[ Copied! ]"
                } else {
                    "[ Copy URL ]"
                };
                let btn_style = if self.copied {
                    Style::default()
                        .fg(self.theme.success)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD)
                };
                Line::from(Span::styled(btn_text, btn_style))
            },
            Line::from(""),
            Line::from(Span::styled(
                " QR code for mobile viewing:",
                Style::default().fg(self.theme.muted),
            )),
        ];

        if let Some(ref qr) = self.qr_code {
            for line in qr.lines() {
                lines.push(Line::from(Span::styled(
                    format!(" {}", line),
                    Style::default().fg(self.theme.primary),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (QR code generation not available)",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter copy to clipboard  Esc close",
            Style::default().fg(self.theme.muted),
        )));

        let block = Block::default()
            .title(" Share Session ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for ShareDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Enter => Some(TuiMsg::CopyShareUrl),
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.cursor < 1 {
                    self.cursor += 1;
                }
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
        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                " Share Session ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Share this session with others by copying the URL below:",
                Style::default().fg(theme.muted),
            )]),
            Line::from(""),
            {
                let url_style = if self.copied {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.primary)
                };
                Line::from(Span::styled(format!("  {}  ", self.share_url), url_style))
            },
            Line::from(""),
            {
                let btn_text = if self.copied {
                    "[ Copied! ]"
                } else {
                    "[ Copy URL ]"
                };
                let btn_style = if self.copied {
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                };
                Line::from(Span::styled(btn_text, btn_style))
            },
            Line::from(""),
            Line::from(Span::styled(
                " QR code for mobile viewing:",
                Style::default().fg(theme.muted),
            )),
        ];

        if let Some(ref qr) = self.qr_code {
            for line in qr.lines() {
                lines.push(Line::from(Span::styled(
                    format!(" {}", line),
                    Style::default().fg(theme.primary),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (QR code generation not available)",
                Style::default().fg(theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter copy to clipboard  Esc close",
            Style::default().fg(theme.muted),
        )));

        let block = Block::default()
            .title(" Share Session ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Share
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_code_generation() {
        let qr = ShareDialog::generate_qr_code("https://example.com");
        assert!(qr.is_some());
        let qr_str = qr.unwrap();
        assert!(!qr_str.is_empty());
        assert!(qr_str.contains('#') || qr_str.contains(' '));
    }

    #[test]
    fn test_qr_code_empty() {
        let qr = ShareDialog::generate_qr_code("");
        assert!(qr.is_none());
    }
}
