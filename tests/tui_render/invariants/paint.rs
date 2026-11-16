//! I1 — nothing is painted outside the rect a widget was given — and I2 — no
//! panic and no blank frame at the smallest supported size.
//!
//! ## How I1 is checked
//!
//! The surface is pre-filled with a sentinel style, the widget is rendered
//! into a sub-rect of it, and every cell outside that sub-rect must still be
//! the sentinel. A widget that computes an inner rect from the one it was
//! given writes only inside; a widget that writes at an absolute coordinate,
//! or grows its rect by one, overwrites a cell it never owned — and the
//! sentinel is chosen to be a value no render path produces, so `painted
//! something that happens to look like its neighbour` cannot hide behind it.
//!
//! [`the_containment_check_is_not_vacuous`] is the proof: the same assertion,
//! run against a fixture widget that paints one cell wider than its rect,
//! must fail and must name the cell.

use std::sync::Arc;

use codegg::tui::app::TuiMsg;
use codegg::tui::components::component::{Component, DialogType};
use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use codegg::tui::theme::Theme;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame as RaFrame;

use super::capture::{self, Frame, Panels};
use super::fixtures;

/// The sizes I2 sweeps: the smallest supported one, and a degenerate one.
pub const SMALL_SIZES: &[(u16, u16)] = &[(20, 6), (1, 1)];

/// The sizes §10's integration list names, for the no-panic sweep.
pub const GRID_SIZES: &[(u16, u16)] = &[(20, 6), (40, 12), (80, 24), (120, 40)];

fn theme() -> Arc<Theme> {
    Arc::new(Theme::dark())
}

/// Every cell outside `inner` must still be the sentinel.
///
/// Returns the first violation rather than a count: one cell of evidence
/// localises the defect, a count does not.
pub fn paint_is_confined(frame: &Frame, inner: Rect) -> Result<(), String> {
    if let Some(cell) = frame
        .outside(inner)
        .find(|cell| !capture::is_sentinel(cell))
    {
        return Err(format!(
            "the widget painted outside its rect: ({}, {}) holds {:?} with fg {:?} / bg {:?}, \
             but its rect is {inner:?}",
            cell.x, cell.y, cell.symbol, cell.fg, cell.bg
        ));
    }
    Ok(())
}

/// A component that paints one cell wider than the rect it is given.
///
/// Not a defect to fix: it exists so the containment assertion can be shown to
/// fail on a real overflow. A guard that cannot be made to fail proves
/// nothing.
struct PaintsOneCellTooWide;

impl Component for PaintsOneCellTooWide {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut RaFrame, area: Rect, _theme: &Arc<Theme>) {
        let too_wide = Rect {
            width: area.width + 1,
            ..area
        };
        frame.render_widget(Block::default().borders(Borders::ALL), too_wide);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

/// A component that paints nothing at all.
struct PaintsNothing;

impl Component for PaintsNothing {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, _frame: &mut RaFrame, _area: Rect, _theme: &Arc<Theme>) {}

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

// ── Work package A: the helper carries style ─────────────────────────

#[test]
fn the_capture_helper_returns_one_styled_cell_per_buffer_position() {
    // WP A's acceptance evidence: the count is exactly `width * height`, and a
    // cell known to carry the host's border style reports that style. Both
    // halves matter — a helper that returned the right count and dropped the
    // colour is the helper this milestone replaces.
    let theme = theme();
    let area = Rect::new(0, 0, 44, 16);
    let mut dialog = InfoDialog::new(
        Arc::clone(&theme),
        InfoType::ShellShow,
        vec!["$ cargo test".to_string()],
    );
    let frame = capture::hosted(
        &mut dialog,
        area,
        &theme,
        SidePanelId::ShellShow.label(),
        false,
    );

    assert_eq!(
        frame.len(),
        area.width as usize * area.height as usize,
        "the helper must return one cell per buffer position"
    );

    let corner = frame.at(0, 0);
    assert_eq!(
        corner.symbol, "┌",
        "the host's top-left corner is not there"
    );
    assert_eq!(
        corner.fg, theme.border,
        "the host paints its border with `Theme::border`; the capture reports {:?}",
        corner.fg
    );
    assert_ne!(
        corner.fg,
        Color::Reset,
        "a captured border cell must carry the theme colour, not terminal default"
    );
}

#[test]
fn a_symbol_only_comparison_misses_a_cell_whose_style_differs() {
    // The property the old string helper lacked: two frames can hold exactly
    // the same glyphs and still be different pictures. The assertion proves
    // the capture can tell them apart, which is what makes I5 expressible.
    let plain = Arc::new(Theme::dark());
    let mut recoloured = Theme::dark();
    recoloured.border = Color::Rgb(0x12, 0x34, 0x56);

    let area = Rect::new(0, 0, 40, 12);
    let mut left = InfoDialog::new(Arc::clone(&plain), InfoType::Cost, vec!["x".to_string()]);
    let left_frame = capture::hosted(&mut left, area, &plain, SidePanelId::Cost.label(), false);
    let recoloured = Arc::new(recoloured);
    let mut right = InfoDialog::new(
        Arc::clone(&recoloured),
        InfoType::Cost,
        vec!["x".to_string()],
    );
    let right_frame = capture::hosted(
        &mut right,
        area,
        &recoloured,
        SidePanelId::Cost.label(),
        false,
    );

    assert_eq!(
        left_frame.text(),
        right_frame.text(),
        "the two renders are meant to be glyph-identical for this comparison"
    );
    assert!(
        !left_frame.differing(&right_frame).is_empty(),
        "a border-only colour change is invisible to a symbol comparison, so the \
         capture must report the difference"
    );
}

// ── I1: containment ─────────────────────────────────────────────────

#[test]
fn no_dialog_paints_outside_the_rect_it_was_given() {
    let theme = theme();
    let outer = Rect::new(0, 0, 48, 18);
    let inner = Rect::new(3, 2, 40, 14);
    for fixture in fixtures::dialogs() {
        let mut component = (fixture.make)(&theme);
        let frame = capture::modal_in_sentinel(component.as_mut(), outer, inner, &theme);
        assert_eq!(
            paint_is_confined(&frame, inner),
            Ok(()),
            "{} paints outside the rect it was given",
            fixture.surface
        );
    }
}

#[test]
fn the_containment_check_is_not_vacuous() {
    // I1's acceptance evidence: a widget given an area one cell larger than it
    // paints must be caught, and the report must name the cell.
    let theme = theme();
    let outer = Rect::new(0, 0, 20, 8);
    let inner = Rect::new(2, 1, 10, 5);
    let mut component = PaintsOneCellTooWide;
    let frame = capture::modal_in_sentinel(&mut component, outer, inner, &theme);

    let verdict = paint_is_confined(&frame, inner);
    assert!(
        verdict.is_err(),
        "a widget painting one cell too wide must fail I1, but the check passed"
    );
    let message = verdict.unwrap_err();
    assert!(
        message.contains("(12, 1)") || message.contains("(12, 2)"),
        "the report must localise the overflowing cell, got: {message}"
    );
}

#[test]
fn a_hosted_box_paints_inside_its_column() {
    // The hosted path copies a scratch rect into the column at the column's
    // own origin, so a box that wrote at an absolute coordinate would land
    // outside the column instead of inside it. The sentinel is what makes that
    // visible rather than plausible.
    let theme = theme();
    let outer = Rect::new(0, 0, 50, 20);
    let inner = Rect::new(4, 3, 38, 14);
    let mut dialog = InfoDialog::new(
        Arc::clone(&theme),
        InfoType::Logs,
        vec!["one".to_string(), "two".to_string()],
    );
    let frame = capture::hosted_in_sentinel(
        &mut dialog,
        outer,
        inner,
        &theme,
        SidePanelId::Logs.label(),
        false,
    );
    assert_eq!(
        paint_is_confined(&frame, inner),
        Ok(()),
        "a hosted box painted outside its column"
    );
}

#[test]
fn every_panel_paints_inside_the_column_it_was_given() {
    let theme = theme();
    let outer = Rect::new(0, 0, 52, 22);
    let inner = Rect::new(5, 3, 40, 16);
    let mut panels = Panels::new(Arc::clone(&theme));
    for panel in SidePanelId::ALL {
        for placement in PanelPlacement::ALL {
            panels.host.dock(*panel, *placement);
            let frame = panels.capture_in_sentinel(*placement, outer, inner, false);
            assert_eq!(
                paint_is_confined(&frame, inner),
                Ok(()),
                "{panel:?} on {placement:?} paints outside the column it was given"
            );
        }
    }
}

// ── I2: smallest sizes ──────────────────────────────────────────────

#[test]
fn every_dialog_renders_at_the_smallest_size_and_only_recorded_ones_decline() {
    let theme = theme();
    let mut declining: Vec<&str> = Vec::new();
    for fixture in fixtures::dialogs() {
        for (width, height) in SMALL_SIZES {
            let mut component = (fixture.make)(&theme);
            let frame =
                capture::modal(component.as_mut(), Rect::new(0, 0, *width, *height), &theme);
            assert_eq!(
                frame.len(),
                *width as usize * *height as usize,
                "{} did not fill a {width}x{height} frame",
                fixture.surface
            );
        }
        let mut component = (fixture.make)(&theme);
        let frame = capture::modal(component.as_mut(), Rect::new(0, 0, 20, 6), &theme);
        if frame.painted().next().is_none() {
            declining.push(fixture.surface);
        }
    }

    declining.sort_unstable();
    let recorded = fixtures::small_size_decliners();
    for (surface, reason) in &recorded {
        assert!(
            !reason.trim().is_empty(),
            "{surface} is recorded as a decliner without saying why"
        );
    }
    let mut recorded_surfaces: Vec<&str> = recorded.iter().map(|(surface, _)| *surface).collect();
    recorded_surfaces.sort_unstable();
    assert_eq!(
        declining, recorded_surfaces,
        "the set of dialogs that paint nothing at 20x6 changed; a surface that \
         silently stopped painting is not a recorded decliner, and a recorded \
         one that started painting has a stale row"
    );
}

#[test]
fn every_dialog_renders_without_panicking_across_the_size_grid() {
    // §10's integration list. A panic fails the test by unwinding; the
    // assertion here is the one a panic cannot make: the frame is the size it
    // was asked for, so a render that bailed out still returned a full buffer.
    let theme = theme();
    for fixture in fixtures::dialogs() {
        for (width, height) in GRID_SIZES {
            let mut component = (fixture.make)(&theme);
            let frame =
                capture::modal(component.as_mut(), Rect::new(0, 0, *width, *height), &theme);
            assert_eq!(
                frame.len(),
                *width as usize * *height as usize,
                "{} returned a short frame at {width}x{height}",
                fixture.surface
            );
        }
    }
}

#[test]
fn every_panel_renders_without_panicking_across_the_size_grid() {
    let theme = theme();
    let mut panels = Panels::new(Arc::clone(&theme));
    for panel in SidePanelId::ALL {
        panels.host.dock(*panel, PanelPlacement::Right);
        for (width, height) in GRID_SIZES {
            let frame = panels.capture(
                PanelPlacement::Right,
                Rect::new(0, 0, *width, *height),
                false,
            );
            assert_eq!(
                frame.len(),
                *width as usize * *height as usize,
                "{panel:?} returned a short frame at {width}x{height}"
            );
            if *width > 1 && *height > 1 {
                assert!(
                    frame.painted().next().is_some(),
                    "{panel:?} paints nothing at {width}x{height}"
                );
            }
        }
    }
}

#[test]
fn a_blank_component_is_what_the_non_degenerate_size_check_rejects() {
    // I2's second half is "a non-degenerate size paints at least one non-blank
    // cell". A check that accepted an empty frame would pass for every widget,
    // including one that renders nothing at all.
    let theme = theme();
    let mut component = PaintsNothing;
    let frame = capture::modal(&mut component, Rect::new(0, 0, 20, 6), &theme);
    assert!(
        frame.painted().next().is_none(),
        "the fixture is meant to paint nothing"
    );
    assert!(
        frame.len() == 120,
        "the frame still has to be a full 20x6 surface, got {} cells",
        frame.len()
    );
}
