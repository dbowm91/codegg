use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;
use std::cell::Cell;
use std::time::{Duration, Instant};

pub struct SpinnerWidget {
    frames: Vec<&'static str>,
    current_frame: Cell<usize>,
    speed_ms: u64,
    last_update: Cell<Instant>,
    color: Color,
    active: bool,
    label: String,
}

impl SpinnerWidget {
    pub fn new() -> Self {
        Self {
            frames: vec!["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"],
            current_frame: Cell::new(0),
            speed_ms: 80,
            last_update: Cell::new(Instant::now()),
            color: Color::Reset,
            active: false,
            label: String::new(),
        }
    }

    pub fn with_speed(self, speed_ms: u64) -> Self {
        Self { speed_ms, ..self }
    }

    pub fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    pub fn with_label(self, label: &str) -> Self {
        Self { label: label.to_string(), ..self }
    }

    pub fn start(&mut self, label: Option<String>) {
        self.active = true;
        self.label = label.unwrap_or_default();
        self.current_frame.set(0);
        self.last_update.set(Instant::now());
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.label = String::new();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn tick(&self) {
        if !self.active {
            return;
        }
        let now = Instant::now();
        let last_update = self.last_update.get();
        if now.duration_since(last_update) >= Duration::from_millis(self.speed_ms) {
            let next_frame = (self.current_frame.get() + 1) % self.frames.len();
            self.current_frame.set(next_frame);
            self.last_update.set(now);
        }
    }

    pub fn frame(&self) -> &str {
        if !self.active {
            return "";
        }
        self.frames[self.current_frame.get()]
    }
}

impl Default for SpinnerWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &SpinnerWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        let text = if !self.label.is_empty() {
            format!("{} {}", self.frame(), &self.label)
        } else {
            self.frame().to_string()
        };

        let style = Style::default().fg(self.color);
        let line = Line::from(vec![Span::styled(text, style)]);
        let paragraph = Paragraph::new(line);
        paragraph.render(area, buf);
    }
}
