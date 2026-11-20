//! Layout management for the TUI.
//!
//! ## Layout Structure
//!
//! That is the default [`PanelPlacement::Right`]. The panel may instead take
//! the left column or a full-width row under the main buffer, which is the
//! same split on the other axis:
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

/// Rows the transcript keeps whatever else claims the pane.
///
/// Named once because two places are measuring against it: the session layout
/// gives the transcript this floor, and the budget the tasks overlay and the
/// tab bar share is what a pane has left *after* it. Two literals that had to
/// agree would be a floor that agrees with itself until one of them changes.
const MIN_TRANSCRIPT_HEIGHT: u16 = 10;

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

    /// Rows the tab bar and the tasks overlay share, on a pane of `pane_height`.
    ///
    /// What is left once every other claim is honoured, the transcript floor
    /// included. Sharing one budget is what stops the two from overshooting
    /// the pane together: without it the carve below yields silently and
    /// `Ctrl+T` reads as a dead key on exactly the terminals where a status
    /// overlay is most wanted.
    fn spare_rows(
        &self,
        pane_height: u16,
        todo_rows: u16,
        hint_rows: u16,
        max_prompt_rows: u16,
    ) -> u16 {
        let claimed = self
            .header_height
            .saturating_add(self.footer_height)
            .saturating_add(MIN_TRANSCRIPT_HEIGHT)
            .saturating_add(max_prompt_rows)
            .saturating_add(todo_rows)
            .saturating_add(hint_rows);
        pane_height.saturating_sub(claimed)
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

/// Rows each row-claiming region asked for this frame.
///
/// The **asks**, not the answers: [`TuiLayout::plan_frame`] clamps the
/// on-demand ones against the budget the pane has left. `prompt_rows` is the
/// composer's `needed_height` at the pane's width, measured by the caller
/// because wrapping is a function of width and the width is the plan's own
/// output — the one claim that cannot be stated before the frame is planned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameClaims {
    /// Rows the above-input todo strip asks for.
    pub todo_rows: u16,
    /// Rows the contextual key line asks for.
    pub hint_rows: u16,
    /// Rows the subagent tab bar asks for.
    pub tab_rows: u16,
    /// Rows the unified tasks overlay asks for.
    pub tasks_rows: u16,
    /// Rows the turn-tail deliverable card asks for.
    pub present_rows: u16,
    /// Rows the composer asks for, at the pane's width and already capped.
    pub prompt_rows: u16,
}

/// The frame's outer geometry: the panel edges it was given, the strip its own
/// border takes, and the pane the session layout divides.
///
/// Settled before a row is claimed, because the hint line and the composer both
/// wrap at the pane's width and that width is a function of the frame and its
/// panels — never of a claim. Deriving it once is what lets the plan be planned
/// from numbers alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameOuter {
    /// The frame this was carved from.
    pub frame: Rect,
    /// One rect per occupied panel edge.
    pub panels: PanelRegions,
    /// The 1-column strip the outer left border is painted on.
    pub left_border: Rect,
    /// The 1-row strip the outer bottom border is painted on, when the pane
    /// can spare it.
    pub bottom_border: Option<Rect>,
    /// The area the session layout divides.
    pub pane: Rect,
}

impl FrameOuter {
    /// Rows a composer may claim: 40% of the frame, never less than a box.
    ///
    /// The cap is on the box, not on the text: text past it stays in the buffer
    /// and stays reachable by scrolling.
    pub fn max_prompt_rows(&self) -> u16 {
        (self.frame.height * 40 / 100).max(3)
    }
}

/// Every region one frame composes, and who decided each one.
///
/// [`TuiLayout::plan_frame`] derives the regions the arithmetic decides. Three
/// are not the arithmetic's to decide: `scrollbar` is the transcript's own
/// gutter, `completion` is the popup the *filtered* rows size, and a dialog's
/// rect is `App::dialog_rect`'s — one derivation that the painted box and the
/// clickable box both read. The two painters record theirs here so one value
/// still answers "where was it", rather than a second field that could
/// disagree with the painted rect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FramePlan {
    /// The panel edges this frame was planned around.
    pub panels: PanelRegions,
    /// The area the session layout divided.
    pub pane: Rect,
    /// The strip the outer left border is painted on.
    pub left_border: Rect,
    /// The strip the outer bottom border is painted on, when it exists.
    pub bottom_border: Option<Rect>,
    /// The header chunk.
    pub header: Rect,
    /// The footer chunk.
    pub footer: Rect,
    /// The composer's whole allocation: the input box *and* the strips carved
    /// off its top. The completion popup anchors on this, not on the box.
    pub composer: Rect,
    /// The input box, after every strip above it has been carved off.
    pub prompt: Rect,
    /// The transcript, after the tab bar has been carved off its top.
    pub viewport: Rect,
    /// The subagent tab bar, when a tab is open and the transcript can spare
    /// the row.
    pub tab_bar: Option<Rect>,
    /// The contextual key line.
    pub hint: Option<Rect>,
    /// The above-input todo strip.
    pub todo: Option<Rect>,
    /// The unified tasks overlay (`Ctrl+T`).
    pub tasks: Option<Rect>,
    /// The turn-tail deliverable card.
    pub present: Option<Rect>,
    /// The transcript's scrollbar gutter, recorded by its painter.
    pub scrollbar: Option<Rect>,
    /// The completion popup, recorded by its painter.
    pub completion: Option<Rect>,
}

/// Take `rows` off the top of `area`, or yield.
///
/// Yields when nothing is claimed, and when the chunk cannot spare the row: a
/// strip that took the last one would leave its neighbour — the input box or
/// the transcript — zero rows tall. Yield, never shrink the neighbour, in the
/// one place it is implemented.
fn carve_top(area: Rect, rows: u16) -> (Option<Rect>, Rect) {
    if rows == 0 || area.height <= rows {
        return (None, area);
    }
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(rows), Constraint::Min(1)])
        .split(area);
    (Some(split[0]), split[1])
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
                Constraint::Min(MIN_TRANSCRIPT_HEIGHT),
                Constraint::Length(height),
                Constraint::Length(self.config.footer_height),
            ])
            .split(area)
            .to_vec()
    }

    /// Carve the TUI's own borders out of the frame, naming the pane between
    /// them.
    ///
    /// The left border gets a reserved column rather than being painted over
    /// the content, so the header, viewport, composer and footer never write
    /// their own text into the leftmost column. The bottom border gets its own
    /// row, carved off **before** the session layout runs: `footer_height` is
    /// one, so anchoring the edge on the footer's own last row painted a
    /// `Borders::BOTTOM` block across the only row the status line had.
    pub fn frame_outer(&self, area: Rect, panels: PanelRegions) -> FrameOuter {
        let bordered = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(panels.main);
        let main_area_inner = bordered[1];
        let (pane, bottom_border) = if main_area_inner.height > 0 {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(main_area_inner);
            let usable = main_area_inner.width > 0 && split[0].height > 0;
            (split[0], if usable { Some(split[1]) } else { None })
        } else {
            (main_area_inner, None)
        };
        FrameOuter {
            frame: area,
            panels,
            left_border: bordered[0],
            bottom_border,
            pane,
        }
    }

    /// Plan the frame: the rect every region of this frame owns.
    ///
    /// **A claim is carved where it is drawn, in the order it is drawn.** The
    /// pane divides into header, transcript, composer and footer. Off the
    /// composer's top, in this order: the hint line, the todo strip, the tasks
    /// overlay, the deliverable card — and the input box takes what is left. So
    /// the strips' vertical order *is* the order of the claims, and a claim can
    /// never be honoured above a row that is drawn below it. The subagent tab
    /// bar is carved off the **transcript's** top, because that is where it is
    /// drawn: charging it to the composer would charge one row twice.
    ///
    /// **A claim that does not fit yields; it never shrinks its neighbour.**
    /// Every strip goes through [`carve_top`], and the overlay and the bar draw
    /// from one budget of rows left after every other claim, the bar first — a
    /// tab the user opened going invisible is worse than a shorter overlay.
    ///
    /// Pure. Every input is a number, a placement, or the layout's own
    /// configuration; nothing here reads `App`, paints, or touches the world,
    /// and equal inputs return equal plans.
    pub fn plan_frame(&self, outer: FrameOuter, claims: FrameClaims) -> FramePlan {
        let spare_rows = self.config.spare_rows(
            outer.pane.height,
            claims.todo_rows,
            claims.hint_rows,
            outer.max_prompt_rows(),
        );
        let tab_rows = claims.tab_rows.min(spare_rows);
        let tasks_rows = claims.tasks_rows.min(spare_rows.saturating_sub(tab_rows));
        let present_rows = claims.present_rows.min(
            spare_rows
                .saturating_sub(tab_rows)
                .saturating_sub(tasks_rows),
        );
        let prompt_height = claims
            .prompt_rows
            .saturating_add(claims.todo_rows)
            .saturating_add(tasks_rows)
            .saturating_add(present_rows)
            .saturating_add(claims.hint_rows);
        let chunks = self.session_layout(outer.pane, Some(prompt_height));

        let (hint, available) = carve_top(chunks[2], claims.hint_rows);
        let (todo, available) = carve_top(available, claims.todo_rows);
        let (tasks, available) = carve_top(available, tasks_rows);
        let (present, prompt) = carve_top(available, present_rows);
        let (tab_bar, viewport) = carve_top(chunks[1], tab_rows);

        FramePlan {
            panels: outer.panels,
            pane: outer.pane,
            left_border: outer.left_border,
            bottom_border: outer.bottom_border,
            header: chunks[0],
            footer: chunks[3],
            composer: chunks[2],
            prompt,
            viewport,
            tab_bar,
            hint,
            todo,
            tasks,
            present,
            scrollbar: None,
            completion: None,
        }
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
        assert_eq!(
            chunks[1].height,
            area.height - 5,
            "the transcript is the only chunk that absorbs slack"
        );
    }

    /// A three-row composer and nothing else claiming the pane.
    fn bare_claims() -> FrameClaims {
        FrameClaims {
            prompt_rows: 3,
            ..FrameClaims::default()
        }
    }

    /// Plan a frame from inputs alone — no `App`, no terminal, no frame.
    fn plan_at(area: Rect, claims: FrameClaims) -> FramePlan {
        let layout = layout();
        let panels = layout.split(area, &[]);
        layout.plan_frame(layout.frame_outer(area, panels), claims)
    }

    fn strips(plan: &FramePlan) -> Vec<(&'static str, Rect)> {
        let mut ordered = Vec::new();
        for (name, rect) in [
            ("hint", plan.hint),
            ("todo", plan.todo),
            ("tasks", plan.tasks),
            ("present", plan.present),
        ] {
            if let Some(rect) = rect {
                ordered.push((name, rect));
            }
        }
        ordered.push(("prompt", plan.prompt));
        ordered
    }

    /// The plan is a value: the same frame and the same claims return the same
    /// plan, and nothing outside the arguments can change the answer.
    #[test]
    fn a_plan_is_a_function_of_its_inputs() {
        let area = Rect::new(0, 0, 120, 40);
        let claims = FrameClaims {
            todo_rows: 1,
            hint_rows: 1,
            tasks_rows: 2,
            ..bare_claims()
        };
        let first = plan_at(area, claims);
        assert_eq!(
            first,
            plan_at(area, claims),
            "planning twice from equal inputs must return equal plans"
        );
        assert_eq!(first.header.height, 1, "the header keeps its row");
        assert_eq!(
            first.prompt.height, 3,
            "the composer keeps the height it asked for"
        );
    }

    /// The load-bearing rule, executable: the strips are laid out top-down in
    /// the order they are claimed, each directly beneath the one before it, and
    /// together they tile the composer's allocation exactly.
    #[test]
    fn the_carve_order_is_the_order_on_screen() {
        let area = Rect::new(0, 0, 120, 40);
        let plan = plan_at(
            area,
            FrameClaims {
                todo_rows: 1,
                hint_rows: 1,
                tab_rows: 1,
                tasks_rows: 2,
                present_rows: 3,
                prompt_rows: 3,
            },
        );
        let mut cursor = plan.composer.y;
        for (name, rect) in strips(&plan) {
            assert_eq!(
                rect.y, cursor,
                "{name} must sit directly below its predecessor"
            );
            assert_eq!(rect.width, plan.composer.width, "{name} spans the composer");
            cursor += rect.height;
        }
        assert_eq!(
            cursor,
            plan.composer.y + plan.composer.height,
            "the strips tile the composer: no row is lost and none is counted twice"
        );
        assert_eq!(plan.hint.expect("a hint row").height, 1);
        assert_eq!(plan.todo.expect("a todo row").height, 1);
        assert_eq!(plan.tasks.expect("task rows").height, 2);
        assert_eq!(plan.present.expect("card rows").height, 3);
    }

    /// Reordering the two one-row claims is the bug this catches: swapping the
    /// hint and the todo strip leaves both present and both one row tall, so
    /// only their order distinguishes the frame the comments describe from the
    /// frame a reordering produces.
    #[test]
    fn a_swapped_carve_puts_a_strip_on_the_wrong_row() {
        let area = Rect::new(0, 0, 120, 40);
        let plan = plan_at(
            area,
            FrameClaims {
                hint_rows: 1,
                todo_rows: 1,
                ..bare_claims()
            },
        );
        assert_eq!(plan.hint.expect("a hint row").y, plan.composer.y);
        assert_eq!(plan.todo.expect("a todo row").y, plan.composer.y + 1);
    }

    /// A claim that cannot be honoured takes nothing, and what it would have
    /// taken goes to the region with the floor — never to itself.
    #[test]
    fn a_claim_that_does_not_fit_yields_instead_of_taking_the_last_row() {
        // 12 rows cannot pay the transcript floor and the composer both, so
        // nothing is left over for the on-demand strips.
        let short = plan_at(
            Rect::new(0, 0, 40, 12),
            FrameClaims {
                tasks_rows: 40,
                present_rows: 40,
                ..bare_claims()
            },
        );
        assert!(
            short.tasks.is_none() && short.present.is_none(),
            "an overlay with no budget must drop out, not render short"
        );
        assert!(
            short.viewport.height >= MIN_TRANSCRIPT_HEIGHT,
            "the rows a yielded claim did not take belong to the transcript, got {}",
            short.viewport.height
        );

        // 120x40 can pay the floor and the box, but a card asking for more than
        // the chunk holds must not take the box's rows.
        let tall = plan_at(
            Rect::new(0, 0, 120, 40),
            FrameClaims {
                present_rows: 80,
                ..bare_claims()
            },
        );
        assert_eq!(
            tall.prompt.height, 3,
            "a card that outgrew the chunk must not take the input box's rows"
        );
        assert_eq!(
            tall.present.expect("the card is on screen").height,
            tall.composer.height - 3,
            "the card takes what is left above the box, no more"
        );
    }

    /// The tab bar is drawn on the transcript, so it is charged to the
    /// transcript. Charging the composer as well would take two rows for one
    /// bar and move the input box under the user's hands when a tab opens.
    #[test]
    fn the_tab_bar_is_charged_to_the_transcript_not_the_composer() {
        let area = Rect::new(0, 0, 120, 40);
        let without = plan_at(area, bare_claims());
        let with = plan_at(
            area,
            FrameClaims {
                tab_rows: 1,
                ..bare_claims()
            },
        );
        let tab = with.tab_bar.expect("an open tab reserves its bar");
        assert_eq!(
            tab.y, without.viewport.y,
            "the bar is the viewport's top row"
        );
        assert_eq!(
            with.viewport.y,
            tab.y + tab.height,
            "the transcript starts below the bar"
        );
        assert_eq!(
            with.viewport.height,
            without.viewport.height - 1,
            "the bar costs the transcript exactly one row"
        );
        assert_eq!(
            with.prompt, without.prompt,
            "and the composer not one row at all"
        );
    }

    /// The bar is persistent and the overlay is on demand, so the bar wins the
    /// last row the pane can spare.
    #[test]
    fn the_tab_bar_wins_the_last_row_over_the_overlay() {
        // 23 rows leaves exactly one spare after the transcript floor.
        let plan = plan_at(
            Rect::new(0, 0, 80, 23),
            FrameClaims {
                tab_rows: 1,
                tasks_rows: 2,
                ..bare_claims()
            },
        );
        assert!(plan.tab_bar.is_some(), "the persistent bar keeps its row");
        assert!(
            plan.tasks.is_none(),
            "the on-demand overlay yields the row to it"
        );
    }

    /// A claim set larger than the pane produces a plan, not a panic and not
    /// an inverted rect.
    #[test]
    fn claims_larger_than_the_pane_produce_a_valid_plan() {
        let area = Rect::new(0, 0, 20, 6);
        let plan = plan_at(
            area,
            FrameClaims {
                todo_rows: 5,
                hint_rows: 5,
                tab_rows: 5,
                tasks_rows: 5,
                present_rows: 5,
                prompt_rows: 9,
            },
        );
        for (name, rect) in [
            ("pane", plan.pane),
            ("header", plan.header),
            ("viewport", plan.viewport),
            ("composer", plan.composer),
            ("prompt", plan.prompt),
            ("footer", plan.footer),
        ] {
            assert!(
                rect.y + rect.height <= area.height && rect.x + rect.width <= area.width,
                "{name} must stay inside the frame, got {rect:?}"
            );
        }
        assert!(
            plan.prompt.height <= plan.composer.height,
            "the box cannot outgrow its allocation"
        );
    }

    /// Every terminal size and claim combination the frame can meet: the
    /// regions tile the pane, no two regions overlap, and the composer's
    /// strips tile its allocation. A carve that lost a row, counted one twice,
    /// or painted outside its chunk breaks one of these.
    #[test]
    fn the_plan_tiles_the_pane_across_the_size_and_claim_matrix() {
        let sizes = [(120, 40), (80, 24), (40, 12), (20, 6)];
        let combinations = [
            bare_claims(),
            FrameClaims {
                todo_rows: 1,
                ..bare_claims()
            },
            FrameClaims {
                tasks_rows: 2,
                ..bare_claims()
            },
            FrameClaims {
                todo_rows: 1,
                tasks_rows: 2,
                ..bare_claims()
            },
            FrameClaims {
                todo_rows: 1,
                hint_rows: 1,
                tab_rows: 1,
                tasks_rows: 2,
                ..bare_claims()
            },
        ];
        for (width, height) in sizes {
            for claims in combinations {
                let plan = plan_at(Rect::new(0, 0, width, height), claims);
                let tiled = plan.header.height
                    + plan.tab_bar.map_or(0, |rect| rect.height)
                    + plan.viewport.height
                    + plan.composer.height
                    + plan.footer.height;
                assert_eq!(
                    tiled, plan.pane.height,
                    "{width}x{height} {claims:?}: the regions must tile the pane"
                );
                let composer_rows: u16 = strips(&plan).iter().map(|(_, rect)| rect.height).sum();
                assert_eq!(
                    composer_rows, plan.composer.height,
                    "{width}x{height} {claims:?}: the strips must tile the composer"
                );
                // The strips live in the composer chunk, so the chain starts
                // there — not below the header, which is the other end of the
                // transcript.
                let mut previous_bottom = plan.composer.y;
                for (name, rect) in strips(&plan) {
                    assert_eq!(
                        rect.y, previous_bottom,
                        "{width}x{height}: {name} must not overlap its predecessor"
                    );
                    previous_bottom = rect.y + rect.height;
                }
                assert_eq!(
                    previous_bottom, plan.footer.y,
                    "{width}x{height}: the composer is the chunk above the footer"
                );
            }
        }
    }

    /// The pane the claims are measured against is narrowed by the frame's own
    /// border strip, so the width the composer wraps at is the width it is
    /// painted into.
    #[test]
    fn a_panel_edge_and_the_border_narrow_the_pane_the_claims_are_measured_against() {
        let area = Rect::new(0, 0, 120, 40);
        let panels = layout().split(area, &[PanelPlacement::Right]);
        let outer = layout().frame_outer(area, panels);
        assert_eq!(
            outer.pane.width,
            panels.main.width - 1,
            "the border strip comes out of the main area"
        );
        assert_eq!(outer.left_border.width, 1);
        let plan = layout().plan_frame(outer, bare_claims());
        assert_eq!(plan.pane, outer.pane);
        assert_eq!(
            plan.panels, panels,
            "the plan records the edges it planned around"
        );
    }
}
