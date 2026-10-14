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
//! │            Viewport                 │       Panel        │
//! │         (flexible)                  │    (30 columns)    │
//! │                                     │                    │
//! ├─────────────────────────────────────┤                    │
//! │          Prompt (3 lines)           │                    │
//! ├─────────────────────────────────────┤                    │
//! │          Footer (1 line)            │                    │
//! └─────────────────────────────────────┴────────────────────┘
//! ```
//!
//! That is the default [`PanelPlacement::Right`]. The panel may instead take
//! the left column or a full-width row under the main buffer, which is the
//! same split on the other axis:
//!
//! ```text
//! ┌───────────────┬──────────────────────────────────────────┐
//! │     Panel     │                Header                   │
//! ├───────────────┼──────────────────────────────────────────┤
//! │               │                 Viewport                 │
//! │               ├──────────────────────────────────────────┤
//! │               │            Prompt / Footer               │
//! └───────────────┴──────────────────────────────────────────┘
//! ```
//!
//! ## Layout Constraints
//!
//! - A column panel only appears when terminal width >
//!   `min_main_width + sidebar_width`
//! - A bottom panel only appears when the frame is at least
//!   `MIN_MAIN_PANEL_HEIGHT + MIN_BOTTOM_PANEL_HEIGHT` rows tall
//! - Viewport uses `Constraint::Min(10)` to ensure visibility
//! - All fixed-height sections use `Constraint::Length`

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::tui::panels::PanelPlacement;

/// Smallest bottom-row panel worth drawing: below this a docked chat or log
/// browser has no room to render a row plus its border, so the split gives
/// the height to the transcript instead and the panel is not shown at all.
const MIN_BOTTOM_PANEL_HEIGHT: u16 = 8;

/// Where each part of the frame is painted.
///
/// One field per edge rather than a list, because the edges are a fixed set
/// with fixed names and every caller asks the same question: "where is the
/// left panel?" A list would make each caller re-derive the answer from a
/// position, which is the mistake that paints a panel over the transcript.
///
/// `None` means that edge is not being shown — collapsed, or squeezed out of
/// a frame too small to host it honestly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelRegions {
    /// The transcript, composer and everything that is not a panel.
    pub main: Rect,
    /// The left column, when a panel is docked there.
    pub left: Option<Rect>,
    /// The right column, when a panel is docked there.
    pub right: Option<Rect>,
    /// The full-width row under the buffer, when a panel is docked there.
    pub bottom: Option<Rect>,
}

impl PanelRegions {
    /// The rect for `placement`, or `None` when that edge is not shown.
    pub fn get(&self, placement: PanelPlacement) -> Option<Rect> {
        match placement {
            PanelPlacement::Left => self.left,
            PanelPlacement::Right => self.right,
            PanelPlacement::Bottom => self.bottom,
        }
    }

    /// Every shown panel edge, in a stable order.
    pub fn occupied(&self) -> impl Iterator<Item = (PanelPlacement, Rect)> + '_ {
        PanelPlacement::ALL
            .iter()
            .copied()
            .filter_map(|placement| self.get(placement).map(|rect| (placement, rect)))
    }
}

/// Split one column off an area, leaving the rest as the main pane.
///
/// Built explicitly rather than through `Layout::split` ordering because the
/// two sides differ only in which rect starts at the origin, and a split that
/// happened to emit them in the right order would still leave the *panel* at
/// x=0 for both — which is the shipped left geometry wearing a right-hand
/// placement's name.
fn split_column(area: Rect, placement: PanelPlacement, panel_width: u16) -> (Rect, Rect) {
    let main = Rect {
        x: area.x,
        width: area.width - panel_width,
        ..area
    };
    let panel = Rect {
        width: panel_width,
        ..area
    };
    match placement {
        PanelPlacement::Left => (
            Rect {
                x: area.x + panel_width,
                ..main
            },
            panel,
        ),
        PanelPlacement::Right | PanelPlacement::Bottom => (
            main,
            Rect {
                x: area.x + area.width - panel_width,
                ..panel
            },
        ),
    }
}

/// Height the main buffer keeps when a bottom panel is docked. The viewport
/// alone asks for ten rows (`session_layout`), so anything less than this
/// would leave the transcript with a fraction of what it has today.
const MIN_MAIN_PANEL_HEIGHT: u16 = 12;

/// Layout configuration for the TUI.
pub struct LayoutConfig {
    /// Default width of a side column in columns.
    ///
    /// The seed for both edges, and the value a column falls back to when it
    /// has never been dragged. Per-edge overrides below exist because "make
    /// the right column wider" and "make the left column wider" are different
    /// requests; one shared number can only satisfy one of them.
    pub sidebar_width: u16,
    /// Left-column width after a resize, or `None` for [`Self::sidebar_width`].
    pub left_panel_width: Option<u16>,
    /// Right-column width after a resize, or `None` for [`Self::sidebar_width`].
    pub right_panel_width: Option<u16>,
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
    /// Share of the frame a bottom-docked panel takes, in percent.
    ///
    /// A ratio rather than a fixed row count because a bottom panel is
    /// useless when it is three rows tall and wasteful when it is half the
    /// terminal; percentage keeps it proportional at any window size.
    pub bottom_panel_ratio: u16,
    /// Exact bottom-panel height once the reader has dragged that edge.
    ///
    /// Set by [`crate::tui::panel_resize`], and the only unit a resize step can
    /// use: the ratio is integer-divided against the frame height, so a
    /// one-point change is zero rows on a short frame and two on a tall one.
    pub bottom_panel_rows: Option<u16>,
}

impl LayoutConfig {
    /// Width of the column on `placement`, falling back to `sidebar_width`.
    pub fn column_width(&self, placement: PanelPlacement) -> u16 {
        let override_width = match placement {
            PanelPlacement::Left => self.left_panel_width,
            PanelPlacement::Right | PanelPlacement::Bottom => self.right_panel_width,
        };
        override_width.unwrap_or(self.sidebar_width)
    }

    /// Resolved width of the left column.
    pub fn left_panel_width(&self, fallback: u16) -> u16 {
        self.left_panel_width.unwrap_or(fallback)
    }

    /// Resolved width of the right column.
    pub fn right_panel_width(&self, fallback: u16) -> u16 {
        self.right_panel_width.unwrap_or(fallback)
    }

    /// Record a resized left column.
    pub fn set_left_panel_width(&mut self, width: u16) {
        self.left_panel_width = Some(width);
    }

    /// Record a resized right column.
    pub fn set_right_panel_width(&mut self, width: u16) {
        self.right_panel_width = Some(width);
    }
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 30,
            left_panel_width: None,
            right_panel_width: None,
            min_main_width: 40,
            prompt_height: 3,
            header_height: 1,
            footer_height: 1,
            scrollbar_width: 1,
            bottom_panel_ratio: 35,
            bottom_panel_rows: None,
        }
    }
}

/// Manages terminal layout and area calculations.
pub struct TuiLayout {
    pub config: LayoutConfig,
    /// Height of the frame this layout was last asked to divide.
    ///
    /// Recorded by [`Self::observe_frame`] so a resize can convert the bottom
    /// panel's *percentage* into an exact row count the first time a reader
    /// drags that edge. A percentage cannot drive a one-row step on its own —
    /// it is integer-divided against the frame height, so a point is 0 rows on
    /// a short frame and 2 on a tall one — which is exactly the kind of key
    /// that appears dead for no stated reason.
    frame_height: u16,
}

impl TuiLayout {
    pub fn new() -> Self {
        Self::with_config(LayoutConfig::default())
    }

    pub fn with_config(config: LayoutConfig) -> Self {
        Self {
            config,
            frame_height: 0,
        }
    }

    /// Record the frame this layout is about to divide.
    pub fn observe_frame(&mut self, area: Rect) {
        self.frame_height = area.height;
    }

    /// Frame height most recently observed; 0 before the first render.
    pub fn frame_height(&self) -> u16 {
        self.frame_height
    }

    /// Split the frame into the main pane plus a rect per occupied panel edge.
    ///
    /// Returns a named [`PanelRegions`] rather than a positional `Vec<Rect>`.
    /// A positional vector cannot express "the left column *and* the right
    /// column are both up" without the caller guessing which index means
    /// which edge — and a caller that guessed wrong would paint the chat
    /// panel over the transcript. Naming the regions makes a mistake a
    /// compile error instead of a mangled frame.
    ///
    /// An edge that is collapsed, or that the frame is too small to host
    /// honestly, is simply absent from the result. That refusal is the same
    /// policy on every axis: a panel that cannot render is not shown, because
    /// a two-row panel with a border and no content reads as a broken frame
    /// rather than a small one.
    pub fn split(&self, area: Rect, occupied: &[PanelPlacement]) -> PanelRegions {
        let mut regions = PanelRegions {
            main: area,
            left: None,
            right: None,
            bottom: None,
        };
        // Columns first, so each one can claim its columns from the frame the
        // row has not already taken. The bottom row spans whatever is left
        // over, which is what makes "chat on the left, log underneath" a
        // layout the frame can actually honour.
        let columns: Vec<PanelPlacement> = occupied
            .iter()
            .copied()
            .filter(|placement| placement.is_column())
            .collect();
        let mut remainder = area;
        for placement in columns {
            let Some(panel_width) = self.column_panel_width(remainder, placement) else {
                // Too narrow for another column: this edge stays collapsed
                // rather than squeezing the transcript out of existence.
                continue;
            };
            let (main, panel) = split_column(remainder, placement, panel_width);
            match placement {
                PanelPlacement::Left => regions.left = Some(panel),
                PanelPlacement::Right => regions.right = Some(panel),
                PanelPlacement::Bottom => unreachable!("filtered to columns above"),
            }
            remainder = main;
        }
        if occupied.contains(&PanelPlacement::Bottom) {
            if let Some(bottom) = self.bottom_panel_height(remainder.height) {
                let parts = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(bottom)])
                    .split(remainder);
                regions.bottom = Some(parts[1]);
                remainder = parts[0];
            }
        }
        regions.main = remainder;
        regions
    }

    /// Columns a side panel can take, or `None` when the frame cannot spare
    /// them without starving the transcript.
    ///
    /// Per edge rather than one shared number, so a reader who widened the
    /// right column does not silently widen the left one as well.
    fn column_panel_width(&self, area: Rect, placement: PanelPlacement) -> Option<u16> {
        let panel_width = self.config.column_width(placement);
        // Two columns need twice the width. Checking each in turn is what
        // stops the second column from silently eating the transcript when
        // the frame is only just wide enough for one.
        (area.width > panel_width + self.config.min_main_width).then_some(panel_width)
    }

    /// Rows a bottom-docked panel gets, or `None` when the frame is too
    /// short to split at all.
    ///
    /// An explicit row count from a resize wins over the ratio; the ratio is
    /// only the shape the panel is given before anyone drags its edge.
    ///
    /// The ratio is clamped against both floors rather than trusted: a window
    /// 20 rows tall would otherwise hand 7 to the panel and 13 to a buffer
    /// whose viewport alone asks for 10, and a window 12 rows tall would
    /// produce a negative remainder.
    pub(crate) fn bottom_panel_height(&self, height: u16) -> Option<u16> {
        if height < MIN_MAIN_PANEL_HEIGHT + MIN_BOTTOM_PANEL_HEIGHT {
            return None;
        }
        let ceiling = height - MIN_MAIN_PANEL_HEIGHT;
        let preferred = match self.config.bottom_panel_rows {
            Some(rows) => rows,
            None => {
                let ratio = self.config.bottom_panel_ratio.min(100);
                (u32::from(height) * u32::from(ratio) / 100) as u16
            }
        };
        Some(preferred.max(MIN_BOTTOM_PANEL_HEIGHT).min(ceiling))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> TuiLayout {
        TuiLayout::new()
    }

    #[test]
    fn right_is_the_shipped_geometry() {
        let area = Rect::new(0, 0, 120, 40);
        let regions = layout().split(area, &[PanelPlacement::Right]);
        let panel = regions.right.expect("a right column");
        assert_eq!(panel.width, 30, "panel keeps the configured width");
        assert_eq!(
            regions.main.width, 90,
            "main takes the rest, exactly as before placements existed"
        );
        assert_eq!(regions.main.x, 0);
        assert_eq!(panel.x, 90, "a right panel starts after the main pane");
        assert!(regions.left.is_none() && regions.bottom.is_none());
    }

    /// Regions are named, so a caller cannot index by position and get the
    /// transcript where the panel should be.
    #[test]
    fn left_mirrors_the_geometry_but_keeps_the_main_pane_named() {
        let area = Rect::new(0, 0, 120, 40);
        let regions = layout().split(area, &[PanelPlacement::Left]);
        let panel = regions.left.expect("a left column");
        assert_eq!(regions.main.x, 30, "main starts after the panel");
        assert_eq!(regions.main.width, 90);
        assert_eq!(panel.x, 0, "a left panel starts at the origin");
        assert_eq!(panel.width, 30);
        assert!(regions.right.is_none());
    }

    #[test]
    fn bottom_is_a_full_width_row_under_the_main_buffer() {
        let area = Rect::new(0, 0, 120, 40);
        let regions = layout().split(area, &[PanelPlacement::Bottom]);
        let panel = regions.bottom.expect("a bottom row");
        assert_eq!(
            panel.width, area.width,
            "a bottom panel spans the frame, it does not inherit the column width"
        );
        assert_eq!(panel.x, 0);
        assert_eq!(
            panel.y + panel.height,
            area.height,
            "the panel is the last thing on screen"
        );
        assert!(
            regions.main.height >= MIN_MAIN_PANEL_HEIGHT,
            "the transcript keeps its floor, got {}",
            regions.main.height
        );
        assert!(regions.main.height >= panel.height / 2);
    }

    /// The case the slot history exists for: two panels up at once, each on
    /// its own edge, with the transcript squeezed between them and still
    /// whole. This is the geometry the refusal used to make impossible.
    #[test]
    fn both_columns_can_be_occupied_at_once() {
        let area = Rect::new(0, 0, 120, 40);
        let regions = layout().split(area, &[PanelPlacement::Right, PanelPlacement::Left]);
        let left = regions.left.expect("left column");
        let right = regions.right.expect("right column");
        assert_eq!(left.x, 0);
        assert_eq!(left.width, 30);
        assert_eq!(right.x, 90);
        assert_eq!(right.width, 30);
        assert_eq!(
            regions.main.x, 30,
            "the transcript sits between the two columns"
        );
        assert_eq!(regions.main.width, 60);
        // The panels must not overlap the transcript or each other.
        assert!(left.x + left.width <= regions.main.x);
        assert!(regions.main.x + regions.main.width <= right.x);
    }

    /// A bottom row and a column are independent axes, so they compose. The
    /// row spans only what the column did not take.
    #[test]
    fn a_column_and_the_bottom_row_compose() {
        let area = Rect::new(0, 0, 120, 40);
        let regions = layout().split(area, &[PanelPlacement::Right, PanelPlacement::Bottom]);
        assert!(regions.right.is_some() && regions.bottom.is_some());
        assert_eq!(regions.main.x, 0);
        assert_eq!(regions.main.width, 90);
        let bottom = regions.bottom.expect("the bottom row");
        assert_eq!(bottom.width, 90, "the row spans the frame minus the column");
        assert!(
            bottom.y >= regions.main.y + regions.main.height,
            "the row starts below the buffer"
        );
    }

    /// A hidden panel reserves nothing on either axis — the original bug was
    /// a dead strip left behind on the right.
    #[test]
    fn a_hidden_panel_reserves_nothing_on_any_axis() {
        let layout = layout();
        let area = Rect::new(0, 0, 120, 40);
        let regions = layout.split(area, &[]);
        assert_eq!(regions.main, area, "a collapsed frame keeps the whole area");
        assert!(regions.occupied().next().is_none());
    }

    #[test]
    fn too_small_a_frame_hosts_no_panel_rather_than_a_strip() {
        let layout = layout();
        // Too narrow for a column at all — the shipped refusal, unchanged.
        let narrow = Rect::new(0, 0, 10, 40);
        let regions = layout.split(narrow, &[PanelPlacement::Right]);
        assert!(
            regions.right.is_none(),
            "a column that cannot fit is absent"
        );
        assert_eq!(regions.main, narrow, "and the transcript keeps everything");
        // A column does not care about height, so it still splits here:
        // the width floor is what governs it, and conflating the two axes
        // would make a short-but-wide terminal silently lose its panel.
        assert!(layout
            .split(Rect::new(0, 0, 120, 14), &[PanelPlacement::Right])
            .right
            .is_some());
        // A bottom row does care, and must not starve the transcript.
        let short = Rect::new(0, 0, 120, MIN_MAIN_PANEL_HEIGHT + 2);
        let regions = layout.split(short, &[PanelPlacement::Bottom]);
        assert!(regions.bottom.is_none());
        assert_eq!(regions.main, short);
    }

    /// The second column must not be allowed to eat the transcript. A frame
    /// wide enough for one column and no more still hosts exactly one.
    #[test]
    fn the_second_column_yields_to_the_transcript_floor() {
        // 30 + 40 = 70 is the single-column floor; 100 fits one column and
        // leaves 70, which is not enough for a second (30 + 40).
        let area = Rect::new(0, 0, 100, 40);
        let regions = layout().split(area, &[PanelPlacement::Right, PanelPlacement::Left]);
        assert!(regions.right.is_some(), "the first column still fits");
        assert!(
            regions.left.is_none(),
            "the second column must not squeeze the transcript below its floor"
        );
        assert!(regions.main.width >= 40);
    }

    /// The ratio must never be trusted over the floors. A frame where the
    /// percentage would hand the panel less than it can render has to give
    /// the rows back.
    #[test]
    fn the_bottom_panel_ratio_is_clamped_against_both_floors() {
        // Exactly at the minimum: both halves must still be viable.
        let exact = MIN_MAIN_PANEL_HEIGHT + MIN_BOTTOM_PANEL_HEIGHT;
        let bottom = layout()
            .bottom_panel_height(exact)
            .expect("the minimum frame must still split");
        assert!(bottom >= MIN_BOTTOM_PANEL_HEIGHT);
        assert!(exact - bottom >= MIN_MAIN_PANEL_HEIGHT);

        // A very wide ratio must not swallow the transcript.
        let mut wide = layout();
        wide.config.bottom_panel_ratio = 100;
        let bottom = wide.bottom_panel_height(exact).expect("splits");
        assert_eq!(
            bottom,
            exact - MIN_MAIN_PANEL_HEIGHT,
            "a 100% ratio still owes the transcript its floor"
        );

        // A zero ratio must still leave the panel its floor.
        let mut none = layout();
        none.config.bottom_panel_ratio = 0;
        let bottom = none.bottom_panel_height(exact).expect("splits");
        assert_eq!(bottom, MIN_BOTTOM_PANEL_HEIGHT);
    }

    #[test]
    fn session_layout_is_unaffected_by_placement() {
        let area = Rect::new(0, 0, 90, 40);
        let chunks = layout().session_layout(area, Some(3));
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].height, 1, "header");
        assert_eq!(chunks[3].height, 1, "footer");
    }
}
