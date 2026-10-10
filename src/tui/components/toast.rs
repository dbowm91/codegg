use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;
use std::time::Duration;

use crate::tui::theme::Theme;

/// Base display time for a toast whose message fits on one rendered line.
/// One named constant per level; the effective duration is derived from
/// this plus [`PER_LINE_DURATION`] (see [`duration_for`]).
const INFO_BASE_DURATION: Duration = Duration::from_secs(3);
const SUCCESS_BASE_DURATION: Duration = Duration::from_secs(3);
const WARNING_BASE_DURATION: Duration = Duration::from_secs(5);
const ERROR_BASE_DURATION: Duration = Duration::from_secs(5);

/// Extra display time granted for every rendered (wrapped) line beyond the
/// first, so a multi-line message stays readable.
const PER_LINE_DURATION: Duration = Duration::from_millis(900);

/// Clamp bounds for the computed duration. The ceiling stops a
/// pathological message from pinning a toast on screen indefinitely; the
/// floor is the shortest base so clamping never shortens a one-line toast.
const MIN_TOAST_DURATION: Duration = Duration::from_secs(3);
const MAX_TOAST_DURATION: Duration = Duration::from_secs(30);

/// Nominal inner width used to estimate a toast's wrapped-line count at
/// construction time. `render` measures against the live terminal instead;
/// this constant keeps construction terminal-independent and matches the
/// widest toast the renderer can produce (`60 - 2` border columns).
const NOMINAL_INNER_WIDTH: usize = 58;

/// Rows a message occupies when wrapped to `inner_width` columns.
/// `inner_width` is clamped to at least 1 because `div_ceil(0)` panics.
fn wrapped_line_count(message: &str, inner_width: usize) -> usize {
    message.chars().count().div_ceil(inner_width.max(1))
}

/// Display time for `message` at `level`: the level's base plus a
/// per-wrapped-line allowance, clamped to
/// `[MIN_TOAST_DURATION, MAX_TOAST_DURATION]`.
fn duration_for(level: &ToastLevel, message: &str) -> Duration {
    let extra_lines = wrapped_line_count(message, NOMINAL_INNER_WIDTH).saturating_sub(1) as u32;
    level
        .base_duration()
        .saturating_add(PER_LINE_DURATION.saturating_mul(extra_lines))
        .clamp(MIN_TOAST_DURATION, MAX_TOAST_DURATION)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastLevel {
    /// One-line display time for this severity.
    pub const fn base_duration(&self) -> Duration {
        match self {
            Self::Info => INFO_BASE_DURATION,
            Self::Success => SUCCESS_BASE_DURATION,
            Self::Warning => WARNING_BASE_DURATION,
            Self::Error => ERROR_BASE_DURATION,
        }
    }

    /// Stable lowercase label used by the `/logs` notification history.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub created_at: std::time::Instant,
    pub duration: Duration,
}

impl Toast {
    pub fn info(message: &str) -> Self {
        Self::new(message, ToastLevel::Info)
    }

    pub fn success(message: &str) -> Self {
        Self::new(message, ToastLevel::Success)
    }

    pub fn warning(message: &str) -> Self {
        Self::new(message, ToastLevel::Warning)
    }

    pub fn error(message: &str) -> Self {
        Self::new(message, ToastLevel::Error)
    }

    fn new(message: &str, level: ToastLevel) -> Self {
        let duration = duration_for(&level, message);
        Self {
            message: message.to_string(),
            level,
            created_at: std::time::Instant::now(),
            duration,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.duration
    }
}

const MAX_TOAST_COUNT: usize = 10;

/// Number of expired notifications retained for `/logs`. Without this a
/// toast is unrecoverable once `tick` purges it.
pub const MAX_TOAST_HISTORY: usize = 50;

/// A past notification retained after its live toast expired.
#[derive(Debug, Clone)]
pub struct ToastRecord {
    pub message: String,
    pub level: ToastLevel,
    pub created_at: std::time::Instant,
}

pub struct ToastManager {
    toasts: VecDeque<Toast>,
    history: VecDeque<ToastRecord>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: VecDeque::new(),
            history: VecDeque::new(),
        }
    }

    pub fn add(&mut self, toast: Toast) {
        if self.history.len() >= MAX_TOAST_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(ToastRecord {
            message: toast.message.clone(),
            level: toast.level,
            created_at: toast.created_at,
        });
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

    pub fn tick(&mut self) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| !t.is_expired());
        self.toasts.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// Retained past notifications, most recent first. Bounded by
    /// [`MAX_TOAST_HISTORY`]; `tick` does not shrink this ring.
    pub fn history(&self) -> impl Iterator<Item = &ToastRecord> {
        self.history.iter().rev()
    }

    /// Number of retained past notifications.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Iterate over the active toasts in insertion order. Intended for
    /// assertions and tests; production rendering should use `render`.
    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.toasts.iter()
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
            // Subtract 2 for borders. A toast narrower than 3 columns has
            // no usable inner width — `div_ceil(0)` panics — so clamp to
            // 1 column rather than dividing by zero.
            let inner_width = (max_width as usize).saturating_sub(2).max(1);
            let wrap_lines = wrapped_line_count(&toast.message, inner_width);
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
            let bar_len = (progress * (max_width as f64 - 4.0).max(0.0)) as usize;
            let bar = "█".repeat(bar_len)
                + &"░".repeat(
                    (max_width as usize)
                        .saturating_sub(4)
                        .saturating_sub(bar_len),
                );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_gets_the_level_base_duration() {
        for level in [
            ToastLevel::Info,
            ToastLevel::Success,
            ToastLevel::Warning,
            ToastLevel::Error,
        ] {
            let toast = Toast::new("saved", level);
            assert_eq!(
                toast.duration,
                level.base_duration(),
                "{:?} one-line toast must use its base",
                level
            );
        }
    }

    #[test]
    fn empty_message_gets_the_level_base_duration() {
        assert_eq!(Toast::info("").duration, INFO_BASE_DURATION);
        assert_eq!(Toast::warning("").duration, WARNING_BASE_DURATION);
    }

    #[test]
    fn message_at_the_wrap_boundary_is_still_one_line() {
        let message = "x".repeat(NOMINAL_INNER_WIDTH);
        assert_eq!(wrapped_line_count(&message, NOMINAL_INNER_WIDTH), 1);
        assert_eq!(
            Toast::error(&message).duration,
            ERROR_BASE_DURATION,
            "an exactly-one-line message must not be charged for a second line"
        );
    }

    #[test]
    fn longer_message_gets_strictly_more_time() {
        let short = Toast::error("short message");
        let long = Toast::error(&"x".repeat(NOMINAL_INNER_WIDTH * 5));
        assert_eq!(
            wrapped_line_count(&"x".repeat(NOMINAL_INNER_WIDTH * 5), NOMINAL_INNER_WIDTH),
            5
        );
        assert!(
            long.duration > short.duration,
            "5 wrapped lines ({:?}) must outlast 1 wrapped line ({:?})",
            long.duration,
            short.duration
        );
        assert_eq!(
            long.duration,
            ERROR_BASE_DURATION + PER_LINE_DURATION * 4,
            "each wrapped line beyond the first adds exactly one allowance"
        );
    }

    #[test]
    fn every_level_scales_with_length() {
        for level in [
            ToastLevel::Info,
            ToastLevel::Success,
            ToastLevel::Warning,
            ToastLevel::Error,
        ] {
            let long = Toast::new(&"x".repeat(NOMINAL_INNER_WIDTH * 4), level);
            assert_eq!(long.duration, level.base_duration() + PER_LINE_DURATION * 3);
            assert!(long.duration > MIN_TOAST_DURATION);
        }
    }

    #[test]
    fn cap_holds_for_a_pathological_message() {
        for toast in [
            Toast::info(&"x".repeat(1_000_000)),
            Toast::success(&"x".repeat(1_000_000)),
            Toast::warning(&"y".repeat(1_000_000)),
            Toast::error(&"z".repeat(1_000_000)),
        ] {
            assert_eq!(
                toast.duration, MAX_TOAST_DURATION,
                "a million-character message must clamp to the ceiling, got {:?}",
                toast.duration
            );
        }
    }

    #[test]
    fn duration_never_falls_below_the_floor() {
        assert_eq!(
            duration_for(&ToastLevel::Info, ""),
            MIN_TOAST_DURATION,
            "the floor equals the shortest base"
        );
        for level in [
            ToastLevel::Info,
            ToastLevel::Success,
            ToastLevel::Warning,
            ToastLevel::Error,
        ] {
            assert!(duration_for(&level, "") >= MIN_TOAST_DURATION);
        }
    }

    #[test]
    fn the_four_levels_still_differ_at_base() {
        assert_eq!(Toast::info("x").duration, INFO_BASE_DURATION);
        assert_eq!(Toast::success("x").duration, SUCCESS_BASE_DURATION);
        assert_eq!(Toast::warning("x").duration, WARNING_BASE_DURATION);
        assert_eq!(Toast::error("x").duration, ERROR_BASE_DURATION);
        assert_ne!(INFO_BASE_DURATION, WARNING_BASE_DURATION);
        assert_ne!(SUCCESS_BASE_DURATION, ERROR_BASE_DURATION);
    }

    #[test]
    fn narrow_wrap_width_produces_more_lines_than_wide() {
        let message = "x".repeat(200);
        assert!(wrapped_line_count(&message, 20) > wrapped_line_count(&message, 58));
    }

    #[test]
    fn wrapped_line_count_never_divides_by_zero() {
        assert_eq!(wrapped_line_count("", 0), 0);
        assert_eq!(wrapped_line_count("abc", 0), 3);
    }

    #[test]
    fn history_retains_toasts_after_tick_purges_them() {
        let mut manager = ToastManager::new();
        // Age the live toasts so `tick` purges them on the first pass.
        for toast in [Toast::error("first"), Toast::info("second")] {
            let mut toast = toast;
            toast.created_at = std::time::Instant::now() - Duration::from_secs(3_600);
            manager.add(toast);
        }
        assert_eq!(manager.len(), 2);

        assert!(manager.tick());
        assert!(manager.is_empty(), "live toasts must be purged");

        let history: Vec<_> = manager.history().collect();
        assert_eq!(history.len(), 2, "purged toasts stay recoverable");
        assert_eq!(history[0].message, "second", "history is newest first");
        assert_eq!(history[0].level, ToastLevel::Info);
        assert_eq!(history[1].message, "first");
        assert_eq!(history[1].level, ToastLevel::Error);
        assert_eq!(history[1].level.label(), "error");
    }

    #[test]
    fn history_is_bounded_and_drops_the_oldest() {
        let mut manager = ToastManager::new();
        for i in 0..MAX_TOAST_HISTORY + 10 {
            manager.info(&format!("toast {i}"));
        }
        let history: Vec<_> = manager.history().collect();
        assert_eq!(history.len(), MAX_TOAST_HISTORY);
        assert_eq!(
            history[0].message,
            format!("toast {}", MAX_TOAST_HISTORY + 9)
        );
        assert_eq!(
            history[MAX_TOAST_HISTORY - 1].message,
            "toast 10",
            "the oldest retained record is the tenth notification"
        );
        assert_eq!(manager.history_len(), MAX_TOAST_HISTORY);
    }
}
