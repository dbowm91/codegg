//! Layout management for the TUI.
//!
//! This module provides the [`TuiLayout`] struct which handles splitting the terminal
//! into distinct areas for different UI components.
//!
//! ## Layout Structure
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                        Header (1 line)                     │
//! ├─────────────────────────────────────┬────────────────────┤
//! │                                     │                    │
//! │            Viewport                 │      Sidebar       │
//! │         (flexible)                  │   (30 columns)     │
//! │                                     │                    │
//! ├─────────────────────────────────────┤                    │
//! │          Prompt (3 lines)           │                    │
//! ├─────────────────────────────────────┤                    │
//! │          Footer (1 line)            │                    │
//! └─────────────────────────────────────┴────────────────────┘
//! ```
//!
//! ## Layout Constraints
//!
//! - Sidebar only appears when terminal width > `min_main_width + sidebar_width`
//! - Viewport uses `Constraint::Min(10)` to ensure visibility
//! - All fixed-height sections use `Constraint::Length`

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Layout configuration for the TUI.
pub struct LayoutConfig {
    /// Width of the sidebar in columns
    pub sidebar_width: u16,
    /// Minimum width for the main content area
    pub min_main_width: u16,
    /// Height of the prompt area in lines
    pub prompt_height: u16,
    /// Height of the header in lines
    pub header_height: u16,
    /// Height of the footer in lines
    pub footer_height: u16,
    /// Width of the scrollbar gutter reserved inside the viewport
    pub scrollbar_width: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 30,
            min_main_width: 40,
            prompt_height: 3,
            header_height: 1,
            footer_height: 1,
            scrollbar_width: 1,
        }
    }
}

/// Manages terminal layout and area calculations.
pub struct TuiLayout {
    pub config: LayoutConfig,
}

impl TuiLayout {
    pub fn new() -> Self {
        Self::with_config(LayoutConfig::default())
    }

    pub fn with_config(config: LayoutConfig) -> Self {
        Self { config }
    }

    pub fn split(&self, area: Rect) -> Vec<Rect> {
        let has_sidebar = area.width > self.config.sidebar_width + self.config.min_main_width;

        if has_sidebar {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(self.config.min_main_width),
                    Constraint::Length(self.config.sidebar_width),
                ])
                .split(area)
                .to_vec()
        } else {
            vec![area]
        }
    }

    pub fn session_layout(&self, area: Rect, prompt_height: Option<u16>) -> Vec<Rect> {
        let height = prompt_height.unwrap_or(self.config.prompt_height);
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(self.config.header_height),
                Constraint::Min(10),
                Constraint::Length(height),
                Constraint::Length(self.config.footer_height),
            ])
            .split(area)
            .to_vec()
    }

    /// Split a viewport area into content + scrollbar gutter columns.
    ///
    /// Returns `(content, scrollbar)`. `content` has width `area.width - 1`
    /// and `scrollbar` has width `1`; both share the same `y` and `height`
    /// as the input. If `area.width < 2`, the area is too narrow to reserve
    /// a gutter and the method returns `(area, Rect::default())` so callers
    /// can still render content into the full rectangle.
    pub fn viewport_with_scrollbar(&self, area: Rect) -> (Rect, Rect) {
        let gutter = self.config.scrollbar_width.max(1);
        if area.width <= gutter {
            return (area, Rect::default());
        }
        let content = Rect {
            x: area.x,
            y: area.y,
            width: area.width - gutter,
            height: area.height,
        };
        let scrollbar = Rect {
            x: area.x + content.width,
            y: area.y,
            width: gutter,
            height: area.height,
        };
        (content, scrollbar)
    }
}

impl Default for TuiLayout {
    fn default() -> Self {
        Self::new()
    }
}
