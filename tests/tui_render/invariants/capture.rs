//! The one render capture helper every invariant reads through.
//!
//! ## Why cells and not strings
//!
//! `tests/tui_render/tui_info_dialog_panel_chrome.inc` already renders a
//! hosted column and reads it back, but it maps every cell to
//! `cell.symbol()` and discards the style. A colour regression therefore
//! passes it — and the I5 invariant is *about* colour, so a string-only
//! helper can not express it at all. This module returns [`Cell`] records
//! carrying `symbol`, `fg`, `bg` and `modifiers`, so "the same glyph, in a
//! different colour" is a difference the assertions can see.
//!
//! ## The three entry points
//!
//! * [`modal`] renders a [`Component`] through the modal path — the same
//!   `render(frame, area, theme)` call `App::render_dialog` makes.
//! * [`hosted`] renders a [`Component`] through the panel host's
//!   `render_hosted`, so the host's border, title and focus marker are on
//!   screen exactly as they are in the docked column.
//! * [`app_frame`] renders a whole [`App`] frame.
//!
//! Each has a `*_sentinel` twin: the same render into a region of a larger
//! surface that was pre-filled with [`sentinel_style`], which is what makes
//! I1 (nothing painted outside the given rect) checkable rather than assumed.

use std::sync::Arc;

use codegg::tui::app::state::ChatState;
use codegg::tui::app::App;
use codegg::tui::components::component::{Component, FocusManager};
use codegg::tui::components::sidebar::SidebarWidget;
use codegg::tui::input::InputMode;
use codegg::tui::interactive_terminal::InteractiveTerminalController;
use codegg::tui::panels::{PanelCtx, PanelPlacement, SidePanelHost};
use codegg::tui::theme::Theme;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell as RawCell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::Terminal;

/// The glyph a sentinel cell carries.
pub const SENTINEL_SYMBOL: &str = "¤";

/// The magenta-on-cyan style a sentinel cell carries.
///
/// Deliberately garish and deliberately not a theme colour: a widget that
/// painted the sentinel by accident would be indistinguishable from one that
/// left the cell alone, so the sentinel must be a value no render path
/// produces.
pub fn sentinel_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xff, 0x00, 0xff))
        .bg(Color::Rgb(0x00, 0xff, 0xff))
}

/// Whether `cell` is an untouched sentinel.
pub fn is_sentinel(cell: &Cell) -> bool {
    cell.symbol == SENTINEL_SYMBOL && Some(cell.style()) == Some(sentinel_style())
}

/// One rendered cell, with everything the frame made it look like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    pub fg: Color,
    pub bg: Color,
    pub modifiers: Modifier,
}

impl Cell {
    fn from_raw(x: u16, y: u16, raw: &RawCell) -> Self {
        Self {
            x,
            y,
            symbol: raw.symbol().to_string(),
            fg: raw.fg,
            bg: raw.bg,
            modifiers: raw.modifier,
        }
    }

    /// The style this cell was painted with, as one comparable value.
    pub fn style(&self) -> Style {
        Style::default()
            .fg(self.fg)
            .bg(self.bg)
            .add_modifier(self.modifiers)
    }

    /// Whether the cell holds no visible glyph.
    pub fn is_blank(&self) -> bool {
        self.symbol.trim().is_empty()
    }
}

/// Every cell of one rendered surface.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
}

impl Frame {
    /// Read a `Buffer` into a frame.
    pub fn from_buffer(buf: &Buffer) -> Self {
        let width = buf.area.width;
        let height = buf.area.height;
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                let raw = buf.cell((x, y)).cloned().unwrap_or_default();
                cells.push(Cell::from_raw(x, y, &raw));
            }
        }
        Self {
            width,
            height,
            cells,
        }
    }

    /// The cell count, which must be `width * height`.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the surface is empty (only a zero-sized frame is).
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Every cell, in row-major order.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    fn index(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// The cell at `(x, y)`.
    pub fn at(&self, x: u16, y: u16) -> &Cell {
        &self.cells[self.index(x, y)]
    }

    /// Every cell of row `y`.
    pub fn row_cells(&self, y: u16) -> &[Cell] {
        let start = self.index(0, y);
        &self.cells[start..start + self.width as usize]
    }

    /// Row `y`'s glyphs, joined.
    pub fn row(&self, y: u16) -> String {
        self.row_cells(y)
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect()
    }

    /// Every row's glyphs.
    pub fn rows(&self) -> Vec<String> {
        (0..self.height).map(|y| self.row(y)).collect()
    }

    /// The whole surface's glyphs, row-major and unseparated.
    pub fn text(&self) -> String {
        self.cells.iter().map(|cell| cell.symbol.as_str()).collect()
    }

    /// The cells that carry a visible glyph.
    pub fn painted(&self) -> impl Iterator<Item = &Cell> {
        self.cells.iter().filter(|cell| !cell.is_blank())
    }

    /// The cells whose position lies outside `rect`.
    pub fn outside(&self, rect: Rect) -> impl Iterator<Item = &Cell> {
        self.cells.iter().filter(move |cell| {
            cell.x < rect.x
                || cell.y < rect.y
                || cell.x >= rect.x.saturating_add(rect.width)
                || cell.y >= rect.y.saturating_add(rect.height)
        })
    }

    /// The cells of `rect`, row-major.
    pub fn region(&self, rect: Rect) -> Vec<&Cell> {
        let mut out = Vec::new();
        for y in rect.y..rect.y.saturating_add(rect.height) {
            for x in rect.x..rect.x.saturating_add(rect.width) {
                if x < self.width && y < self.height {
                    out.push(self.at(x, y));
                }
            }
        }
        out
    }

    /// Every position at which `self` and `other` disagree, as
    /// `(left, right)` pairs.
    ///
    /// Panics on mismatched dimensions: two frames of different sizes are not
    /// comparable, and silently comparing the overlap would make a resized
    /// surface look identical.
    pub fn differing(&self, other: &Frame) -> Vec<(Cell, Cell)> {
        assert_eq!(
            (self.width, self.height),
            (other.width, other.height),
            "two frames of different sizes are not comparable"
        );
        self.cells
            .iter()
            .zip(other.cells.iter())
            .filter(|(left, right)| left != right)
            .map(|(left, right)| (left.clone(), right.clone()))
            .collect()
    }
}

fn fill_sentinel(buf: &mut Buffer) {
    for raw in buf.content.iter_mut() {
        raw.set_symbol(SENTINEL_SYMBOL);
        raw.set_style(sentinel_style());
    }
}

fn blank_region(buf: &mut Buffer, rect: Rect) {
    for y in rect.y..rect.y.saturating_add(rect.height) {
        for x in rect.x..rect.x.saturating_add(rect.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
    }
}

fn draw_modal(
    component: &mut dyn Component,
    outer: Rect,
    inner: Rect,
    theme: &Arc<Theme>,
    sentinel: bool,
) -> Frame {
    let mut terminal = Terminal::new(TestBackend::new(outer.width, outer.height))
        .expect("a test terminal is constructible");
    if sentinel {
        fill_sentinel(terminal.current_buffer_mut());
    }
    let completed = terminal
        .draw(|frame| component.render(frame, inner, theme))
        .expect("the test backend is infallible");
    Frame::from_buffer(completed.buffer)
}

/// Render `component` through the modal path and capture every cell.
pub fn modal(component: &mut dyn Component, area: Rect, theme: &Arc<Theme>) -> Frame {
    draw_modal(component, area, area, theme, false)
}

/// Render `component` into `inner` of a sentinel-filled `outer` surface.
///
/// The cells outside `inner` are what I1 reads: a widget that paints outside
/// the rect it was given overwrites a sentinel it never owned.
pub fn modal_in_sentinel(
    component: &mut dyn Component,
    outer: Rect,
    inner: Rect,
    theme: &Arc<Theme>,
) -> Frame {
    draw_modal(component, outer, inner, theme, true)
}

fn draw_hosted(
    component: &mut dyn Component,
    outer: Rect,
    inner: Rect,
    theme: &Arc<Theme>,
    label: &str,
    focused: bool,
    sentinel: bool,
) -> Frame {
    let mut buf = Buffer::empty(outer);
    if sentinel {
        fill_sentinel(&mut buf);
    }
    codegg::tui::panels::render_hosted(component, &mut buf, inner, theme, label, focused);
    Frame::from_buffer(&buf)
}

/// Render `component` through the panel host's `render_hosted`.
pub fn hosted(
    component: &mut dyn Component,
    area: Rect,
    theme: &Arc<Theme>,
    label: &str,
    focused: bool,
) -> Frame {
    draw_hosted(component, area, area, theme, label, focused, false)
}

/// Render `component` through `render_hosted` into `inner` of a sentinel
/// surface.
pub fn hosted_in_sentinel(
    component: &mut dyn Component,
    outer: Rect,
    inner: Rect,
    theme: &Arc<Theme>,
    label: &str,
    focused: bool,
) -> Frame {
    draw_hosted(component, outer, inner, theme, label, focused, true)
}

/// Render the modal layer as production dispatches it.
///
/// `FocusManager` is the component that receives a modal key and paints the
/// top of its stack, so an invariant about "what the dialog does with a key"
/// has to read the manager's frame: the manager owns `Tab` for its focus ring
/// before the dialog ever sees it.
pub fn managed(focus: &mut FocusManager, area: Rect, theme: &Arc<Theme>) -> Frame {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
        .expect("a test terminal is constructible");
    let completed = terminal
        .draw(|frame| focus.render(frame, area, theme))
        .expect("the test backend is infallible");
    Frame::from_buffer(completed.buffer)
}

/// Render a whole `App` frame.
pub fn app_frame(app: &mut App, width: u16, height: u16) -> Frame {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("a test terminal is constructible");
    let completed = terminal
        .draw(|frame| app.render(frame))
        .expect("the test backend is infallible");
    Frame::from_buffer(completed.buffer)
}

/// The frontend state a panel render borrows, held together so one harness
/// can paint the panel host without touching an `App`.
///
/// Production builds this same context in `App::render_panel_at`; the fields
/// here are the same grants. `now_ms` is pinned to `0` rather than read from
/// the clock, because a render invariant must not depend on wall time.
pub struct Panels {
    pub host: SidePanelHost,
    pub sidebar: SidebarWidget,
    pub chat: ChatState,
    pub terminal: InteractiveTerminalController,
    pub theme: Arc<Theme>,
}

impl Panels {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            host: SidePanelHost::new(),
            sidebar: SidebarWidget::new(Arc::clone(&theme)),
            chat: ChatState::new(),
            terminal: InteractiveTerminalController::new(),
            theme,
        }
    }

    /// Paint `placement`'s panel into `area`, as the frame pass does: the
    /// previous panel's pixels are cleared first.
    pub fn capture(&mut self, placement: PanelPlacement, area: Rect, focused: bool) -> Frame {
        self.capture_in(placement, area, area, focused, false)
    }

    /// Paint `placement`'s panel with the focus flag the host currently
    /// reports, exactly as `App::render_panel_at` derives it.
    pub fn capture_live(&mut self, placement: PanelPlacement, area: Rect) -> Frame {
        let focused = self.host.is_focused_at(placement);
        self.capture_in(placement, area, area, focused, false)
    }

    /// Paint `placement`'s panel into `inner` of a sentinel-filled `outer`
    /// surface, for the I1 containment check.
    pub fn capture_in_sentinel(
        &mut self,
        placement: PanelPlacement,
        outer: Rect,
        inner: Rect,
        focused: bool,
    ) -> Frame {
        self.capture_in(placement, outer, inner, focused, true)
    }

    fn capture_in(
        &mut self,
        placement: PanelPlacement,
        outer: Rect,
        inner: Rect,
        focused: bool,
        sentinel: bool,
    ) -> Frame {
        let mut buf = Buffer::empty(outer);
        if sentinel {
            fill_sentinel(&mut buf);
        } else {
            blank_region(&mut buf, inner);
        }
        if focused {
            self.host.select_placement(placement);
            self.host.focus_at(placement);
        } else {
            self.host.blur();
        }
        // Disjoint field borrows: the host paints while the context borrows the
        // three state values, exactly as `App::render_panel_at` splits them.
        let Panels {
            host,
            sidebar,
            chat,
            terminal,
            theme,
        } = self;
        let mut ctx = PanelCtx {
            theme: Arc::clone(theme),
            now_ms: 0,
            session: sidebar,
            chat,
            terminal,
            vim_mode: false,
            chat_project: None,
            area: inner,
            focused,
            input_mode: InputMode::Normal,
            caret_blink_on: true,
        };
        host.render_at(placement, &mut ctx, &mut buf, inner);
        Frame::from_buffer(&buf)
    }
}
