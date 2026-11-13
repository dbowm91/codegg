//! M003 — the docked-dialog assertion, for **every** dockable box.
//!
//! ## What this covers that `chrome.rs` does not
//!
//! `chrome.rs` asserts I3b for the **panel** surfaces: a focused column and an
//! unfocused one must differ in border or title style. A dialog that paints its
//! own border over the host's destroys that difference as a side effect, but
//! nothing in that sweep is looking at a docked *dialog*. The defect is worse
//! than a lost focus cue: the host paints a border, the panel's name and the
//! `●`, and a second block over the same rect does not layer — it overwrites
//! all three. `AgentDialog` shipped exactly that (`docks_as_viewer` was `true`,
//! `render` painted `Borders::ALL`, and the file had no `render_in_panel`), and
//! the single-dialog test that caught it did not generalise.
//!
//! This module is the generalisation: every fixture whose
//! [`Component::docks_as_viewer`] is true is rendered through the production
//! hosted seam, and the host's three pieces of chrome are asserted intact.
//!
//! ## Why the non-vacuity test is a wrapper and not a second dialog
//!
//! The assertion has to be shown to bite on the shape it exists to catch — a
//! dockable box with no `render_in_panel` override, whose default therefore
//! delegates to the modal render. Wrapping a corrected dialog in a component
//! that answers `docks_as_viewer` but does **not** override the seam recreates
//! that shape exactly, so the rejection proves the checker reads the frame
//! rather than a flag.

use std::sync::Arc;

use codegg::tui::app::TuiMsg;
use codegg::tui::components::component::{Component, DialogType};
use codegg::tui::components::dialogs::agent::AgentDialog;
use codegg::tui::theme::Theme;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

use super::capture::{self, Frame as CapturedFrame};

/// A label that is deliberately not any dialog's own title, so a surviving
/// host title is evidence rather than coincidence.
const HOST_LABEL: &str = "Roster";

/// The size every dockable dialog is rendered at. Large enough for each box's
/// own minimum-size guard (`DiffDialog` needs seven rows) and small enough that
/// a body painting one cell wide is caught.
const DOCKED: (u16, u16) = (44, 16);

fn theme() -> Arc<Theme> {
    Arc::new(Theme::dark())
}

/// Whether `frame` still carries the host's border, title and `●`.
///
/// Returns the first violation rather than a count: one named defect localises
/// the regression, a count does not. The title arm reports the actual top row,
/// which is what names the overwriting title in a failure.
pub fn host_chrome_intact(frame: &CapturedFrame, label: &str) -> Result<(), String> {
    let height = frame.height;
    if height < 3 || frame.width < 2 {
        return Err(format!(
            "the hosted frame is too small to carry chrome: {}x{height}",
            frame.width
        ));
    }
    let top = frame.row(0);
    if !(top.starts_with('┌') && top.ends_with('┐')) {
        return Err(format!(
            "the host top border is gone — row 0 is {top:?}, so the dialog \
             painted an edge it does not own"
        ));
    }
    let bottom = frame.row(height - 1);
    if !(bottom.starts_with('└') && bottom.ends_with('┘')) {
        return Err(format!(
            "the host bottom border is gone — row {} is {bottom:?}",
            height - 1
        ));
    }
    for y in 1..height - 1 {
        let row = frame.row(y);
        let first = row.chars().next().unwrap_or(' ');
        let last = row.chars().last().unwrap_or(' ');
        if first != '│' || last != '│' {
            return Err(format!(
                "the host side border is broken at row {y} — {row:?}"
            ));
        }
    }
    if !top.contains(label) {
        return Err(format!(
            "the host's own title `{label}` is not on screen — row 0 is {top:?}; \
             the dialog's title overwrote it"
        ));
    }
    if !top.contains('●') {
        return Err(format!("the `●` focus marker is gone — row 0 is {top:?}"));
    }
    Ok(())
}

/// A dockable component that never overrides `render_in_panel`.
///
/// This is the pre-fix shape as a value: the trait default delegates to
/// `render`, so the wrapped dialog's owned frame is painted over the host's
/// chrome. It wraps the real dialog rather than rebuilding a fake one, so the
/// failure comes from production's own render path.
struct DockedWithoutSeam(AgentDialog);

impl Component for DockedWithoutSeam {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        self.0.handle_key(key)
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        self.0.update(msg)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.0.render(frame, area, theme);
    }

    fn docks_as_viewer(&self) -> bool {
        true
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Agent
    }
}

#[test]
fn every_dockable_dialog_leaves_the_hosts_border_title_and_focus_dot_alone() {
    let theme = theme();
    let area = Rect::new(0, 0, DOCKED.0, DOCKED.1);
    let mut docked = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in super::fixtures::dialogs() {
        let mut component = (fixture.make)(&theme);
        if !component.docks_as_viewer() {
            continue;
        }
        let frame = capture::hosted(component.as_mut(), area, &theme, HOST_LABEL, true);
        if let Err(message) = host_chrome_intact(&frame, HOST_LABEL) {
            failures.push(format!("{}: {message}", fixture.surface));
        }
        docked += 1;
    }

    assert!(
        failures.is_empty(),
        "every dockable dialog must leave the host's edge, title and `●` intact; \
         these paint over them:\n{}",
        failures.join("\n")
    );
    assert!(
        docked >= 15,
        "the sweep only docked {docked} surfaces, which is too few to be the live \
         fan-out of `docks_as_viewer`"
    );
}

#[test]
fn the_docked_chrome_assertion_detects_a_box_that_paints_its_own_border() {
    // The pre-fix shape: `docks_as_viewer` true, no `render_in_panel`. If this
    // passed, the sweep above would prove nothing.
    let theme = theme();
    let area = Rect::new(0, 0, DOCKED.0, DOCKED.1);
    let mut unfixed = DockedWithoutSeam(AgentDialog::new(Arc::clone(&theme)));
    let frame = capture::hosted(&mut unfixed, area, &theme, HOST_LABEL, true);

    let verdict = host_chrome_intact(&frame, HOST_LABEL);
    let message = verdict.expect_err(
        "a box that paints its own border must fail the hosted-chrome assertion, \
         or the sweep proves nothing",
    );
    assert!(
        message.contains("Agents"),
        "the report must name the overwritten title, got: {message}"
    );
    assert!(
        message.contains("overwrote"),
        "the report must say what happened to the host title, got: {message}"
    );
}

/// The cells on the outer ring of a frame: the top and bottom rows and the
/// two side columns.
fn ring_cells(frame: &CapturedFrame) -> Vec<(u16, u16, String)> {
    let mut out = Vec::new();
    for y in 0..frame.height {
        for x in 0..frame.width {
            if y == 0 || y + 1 == frame.height || x == 0 || x + 1 == frame.width {
                out.push((x, y, frame.at(x, y).symbol.clone()));
            }
        }
    }
    out
}

/// A component that paints a marked frame over the whole area.
///
/// It exists so the no-marker sweep has something to fail on: without it the
/// assertion could pass because nothing ever painted a `●`, which is not the
/// same claim as "a modal never paints its own".
struct MarkedFrame;

impl Component for MarkedFrame {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" Marked ● ");
        frame.render_widget(block, area);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Agent
    }
}

fn ring_markers(frame: &CapturedFrame) -> Vec<(u16, u16)> {
    ring_cells(frame)
        .into_iter()
        .filter(|(_, _, symbol)| symbol == "●")
        .map(|(x, y, _)| (x, y))
        .collect()
}

#[test]
fn no_modal_paints_a_focus_marker_on_its_own_frame() {
    // The owner's rule: a modal styles as focused, and the `●` names the panel
    // column that holds the keyboard, so a modal's own frame must carry none.
    // `Section` is the only producer of a modal frame, and its title is never
    // marked; this sweep is the frame-level evidence.
    let theme = theme();
    let area = Rect::new(0, 0, 80, 24);
    let mut marked = Vec::new();
    let mut swept = 0usize;
    for fixture in super::fixtures::dialogs() {
        let mut component = (fixture.make)(&theme);
        let frame = capture::modal(component.as_mut(), area, &theme);
        let markers = ring_markers(&frame);
        if !markers.is_empty() {
            marked.push(format!("{} at {markers:?}", fixture.surface));
        }
        swept += 1;
    }
    assert!(
        swept >= 40,
        "the marker sweep rendered {swept} modals, which is too few to be the \
         fixture table — a shrunken table would make this pass vacuously"
    );
    assert!(
        marked.is_empty(),
        "these modals paint a `●` on their own frame, so the marker no longer \
         says which panel has the keyboard: {marked:?}"
    );
}

#[test]
fn the_no_marker_sweep_detects_a_marked_frame() {
    let theme = theme();
    let area = Rect::new(0, 0, 20, 6);
    let mut marked = MarkedFrame;
    let frame = capture::modal(&mut marked, area, &theme);
    assert!(
        !ring_markers(&frame).is_empty(),
        "a frame whose title carries a `●` must be detected"
    );
}
