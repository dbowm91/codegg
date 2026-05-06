use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;

use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub created_at: std::time::Instant,
    pub duration: std::time::Duration,
}

impl Toast {
    pub fn info(message: &str) -> Self {
        Self {
            message: message.to_string(),
            level: ToastLevel::Info,
            created_at: std::time::Instant::now(),
            duration: std::time::Duration::from_secs(3),
        }
    }

    pub fn success(message: &str) -> Self {
        Self {
            message: message.to_string(),
            level: ToastLevel::Success,
            created_at: std::time::Instant::now(),
            duration: std::time::Duration::from_secs(3),
        }
    }

    pub fn warning(message: &str) -> Self {
        Self {
            message: message.to_string(),
            level: ToastLevel::Warning,
            created_at: std::time::Instant::now(),
            duration: std::time::Duration::from_secs(5),
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            message: message.to_string(),
            level: ToastLevel::Error,
            created_at: std::time::Instant::now(),
            duration: std::time::Duration::from_secs(5),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.duration
    }
}

const MAX_TOAST_COUNT: usize = 10;

pub struct ToastManager {
    toasts: VecDeque<Toast>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: VecDeque::new(),
        }
    }

    pub fn add(&mut self, toast: Toast) {
        if self.toasts.len() >= MAX_TOAST_COUNT {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    pub fn info(&mut self, message: &str) {
        self.add(Toast::info(message));
    }

    pub fn success(&mut self, message: &str) {
        self.add(Toast::success(message));
    }

    pub fn warning(&mut self, message: &str) {
        self.add(Toast::warning(message));
    }

    pub fn error(&mut self, message: &str) {
        self.add(Toast::error(message));
    }

    pub fn tick(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.toasts.is_empty() {
            return;
        }

        let max_width = area.width.min(60);
        let mut current_y = area.y;

        for toast in self.toasts.iter().take(3) {
            let color = match toast.level {
                ToastLevel::Info => theme.primary,
                ToastLevel::Success => theme.success,
                ToastLevel::Warning => theme.warning,
                ToastLevel::Error => theme.error,
            };

            let title = match toast.level {
                ToastLevel::Info => " INFO ",
                ToastLevel::Success => " SUCCESS ",
                ToastLevel::Warning => " WARNING ",
                ToastLevel::Error => " ERROR ",
            };

            // Calculate height needed for the wrapped message
            // Subtract 2 for borders
            let inner_width = (max_width as usize).saturating_sub(2);
            let wrap_lines = (toast.message.len() + inner_width - 1) / inner_width;
            // Total height: title line (inside border) + wrap_lines + progress bar + 2 for top/bottom borders
            // Actually Paragraph with wrap does this, but we need to know the height for Layout
            // Title is in the block, so we need 1 for message lines + 1 for progress bar + 2 for borders
            let toast_height = (wrap_lines as u16) + 3; 

            if current_y + toast_height > area.y + area.height {
                break;
            }

            let toast_area = Rect {
                x: area.x + area.width.saturating_sub(max_width),
                y: current_y,
                width: max_width,
                height: toast_height,
            };

            let elapsed = toast.created_at.elapsed();
            let remaining = toast.duration.saturating_sub(elapsed);
            let progress = if remaining.is_zero() {
                0.0
            } else {
                remaining.as_secs_f64() / toast.duration.as_secs_f64()
            };
            let bar_len = (progress * (max_width as f64 - 4.0)) as usize;
            let bar = "█".repeat(bar_len)
                + &"░".repeat((max_width as usize - 4).saturating_sub(bar_len));

            let lines = vec![
                Line::from(Span::styled(
                    &toast.message,
                    Style::default().fg(ratatui::style::Color::White),
                )),
                Line::from(Span::styled(bar, Style::default().fg(color))),
            ];

            let paragraph = Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(Style::default().fg(color)),
                );

            frame.render_widget(paragraph, toast_area);
            current_y += toast_height + 1; // +1 for spacing between toasts
        }
    }
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}
