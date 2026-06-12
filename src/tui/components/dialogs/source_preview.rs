use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;

const MAX_PREVIEW_LINES: usize = 500;
const MAX_FILE_SIZE: usize = 1024 * 1024;

#[derive(Clone)]
pub struct SourcePreviewDialog {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub context_radius: usize,
    pub lines: Vec<SourcePreviewLine>,
    pub error: Option<String>,
    pub scroll: u16,
    pub theme: Arc<Theme>,
}

#[derive(Clone)]
pub struct SourcePreviewLine {
    pub number: u32,
    pub text: String,
    pub highlighted: bool,
}

impl SourcePreviewDialog {
    pub fn new(theme: Arc<Theme>, path: PathBuf, line: Option<u32>) -> Self {
        let context_radius = 10;
        let (lines, error) = Self::read_file(&path, line, context_radius);
        Self {
            path,
            line,
            context_radius,
            lines,
            error,
            scroll: 0,
            theme,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    fn read_file(
        path: &Path,
        highlight_line: Option<u32>,
        context_radius: usize,
    ) -> (Vec<SourcePreviewLine>, Option<String>) {
        let content = match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.len() > MAX_FILE_SIZE {
                    return (
                        Vec::new(),
                        Some(format!(
                            "File too large ({} bytes, max {})",
                            bytes.len(),
                            MAX_FILE_SIZE
                        )),
                    );
                }
                match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => {
                        return (
                            Vec::new(),
                            Some("File is not valid UTF-8 (binary file?)".to_string()),
                        );
                    }
                }
            }
            Err(e) => return (Vec::new(), Some(format!("Cannot read file: {e}"))),
        };

        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > MAX_PREVIEW_LINES {
            return (
                Vec::new(),
                Some(format!(
                    "File too long ({} lines, max {})",
                    all_lines.len(),
                    MAX_PREVIEW_LINES
                )),
            );
        }

        let highlight = highlight_line.unwrap_or(0) as usize;
        let start = if highlight > 0 {
            highlight.saturating_sub(1).saturating_sub(context_radius)
        } else {
            0
        };
        let end = (highlight + context_radius).min(all_lines.len());

        let preview_lines: Vec<SourcePreviewLine> = all_lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, text)| SourcePreviewLine {
                number: (start + i + 1) as u32,
                text: text.to_string(),
                highlighted: highlight_line.is_some_and(|hl| hl == (start + i + 1) as u32),
            })
            .collect();

        (preview_lines, None)
    }

    fn visible_lines(&self, height: usize) -> &[SourcePreviewLine] {
        let start = self.scroll as usize;
        let end = (start + height).min(self.lines.len());
        if start < self.lines.len() {
            &self.lines[start..end]
        } else {
            &[]
        }
    }
}

impl Component for SourcePreviewDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                if key.modifiers.is_empty() =>
            {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                if key.modifiers.is_empty() =>
            {
                if (self.scroll as usize) + 1 < self.lines.len() {
                    self.scroll = self.scroll.saturating_add(1);
                }
                None
            }
            crossterm::event::KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                None
            }
            crossterm::event::KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                None
            }
            crossterm::event::KeyCode::Home => {
                self.scroll = 0;
                None
            }
            crossterm::event::KeyCode::End => {
                self.scroll = self.lines.len().saturating_sub(1) as u16;
                None
            }
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                Some(TuiMsg::CloseDialog)
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

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        if area.height < 4 || area.width < 20 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(2),
            ])
            .split(area);

        let filename = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string());
        let line_str = self.line.map(|l| format!(":{}", l)).unwrap_or_default();
        let header_text = format!("{}{}", filename, line_str);
        let header_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Source Preview ");
        let header_para = Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )))
        .block(header_block);
        frame.render_widget(header_para, chunks[0]);

        let content_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border));
        let content_inner = content_block.inner(chunks[1]);
        frame.render_widget(content_block, chunks[1]);

        if let Some(ref err) = self.error {
            let err_para = Paragraph::new(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(self.theme.error),
            )));
            frame.render_widget(err_para, content_inner);
        } else {
            let visible = self.visible_lines(content_inner.height as usize);
            let lines: Vec<Line> = visible
                .iter()
                .map(|pl| {
                    let line_style = if pl.highlighted {
                        Style::default()
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(self.theme.foreground)
                    };
                    let num_style = Style::default().fg(self.theme.muted);
                    Line::from(vec![
                        Span::styled(format!("{:>4} ", pl.number), num_style),
                        Span::styled(pl.text.clone(), line_style),
                    ])
                })
                .collect();
            let para = Paragraph::new(lines);
            frame.render_widget(para, content_inner);
        }

        let footer_text = format!(
            "j/k scroll | PgUp/PgDn | Home/End | Esc close ({}/{} lines)",
            (self.scroll as usize + 1).min(self.lines.len()),
            self.lines.len(),
        );
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(self.theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border)),
        );
        frame.render_widget(footer, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::SourcePreview
    }
}
