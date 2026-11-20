//! I3a — a focused surface renders differently from an unfocused one.
//!
//! ## Why this half only, and why it is deliberately weak
//!
//! I3 has two halves. **I3a** ("focused differs from unfocused") holds today,
//! because every panel marks focus with something; M001 asserts it so M002
//! cannot remove the only difference while it changes everything else.
//! **I3b** ("the difference is a border or title *style* change, uniformly")
//! cannot hold before M002 and belongs there. Adding it here would make this
//! milestone unclosable, so it is not added.
//!
//! ## What the assertion reads
//!
//! Two renders of the *same* surface through the production panel path, one
//! with the host focused and one with it blurred, compared cell by cell —
//! symbol, foreground, background and modifiers. The panels are reached
//! through `SidePanelHost::render_at`, so the `●` in the title is the one
//! `HostPanel::title` paints rather than a marker this test invented.
//!
//! ## Which panels are left out
//!
//! None. `TerminalPanel::is_empty` is deliberately `false` — the panel can
//! always act (`i` starts a shell when none exists), so refusing focus there
//! would strand the keyboard on the one surface that can fix an empty
//! terminal. The sweep below therefore expects every panel to take the
//! keyboard, and fails if one stops doing so.

use std::sync::Arc;

use codegg::tui::app::App;
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use codegg::tui::theme::Theme;
use ratatui::layout::Rect;

use super::capture::{self, Frame, Panels};
use super::fixtures;

/// The column every focus render uses: wide enough for a panel's own guard
/// (`width < 12 || height < 4`) to let it paint.
const COLUMN: Rect = Rect {
    x: 0,
    y: 0,
    width: 40,
    height: 16,
};

fn theme() -> Arc<Theme> {
    Arc::new(Theme::dark())
}

/// The surfaces that cannot take the keyboard. Empty, and asserted as such:
/// the set is the record, so a panel that silently stops being focusable is a
/// failure rather than a smaller sweep.
pub const NOT_FOCUSABLE: &[SidePanelId] = &[];

/// A focused render must differ from an unfocused one.
pub fn focus_is_visible(focused: &Frame, unfocused: &Frame) -> Result<(), String> {
    let differing = focused.differing(unfocused);
    if differing.is_empty() {
        return Err(
            "a focused surface renders identically to an unfocused one, so focus is \
             not indicated anywhere in the frame"
                .to_string(),
        );
    }
    Ok(())
}

/// Fill every hosted panel with a real box and bind the chat panel, so each
/// panel is in the state that can actually take the keyboard.
pub(super) fn ready_panels(theme: &Arc<Theme>) -> Panels {
    let mut panels = Panels::new(Arc::clone(theme));
    panels.host.chat_panel_mut().set_bound(true);
    for panel in SidePanelId::ALL {
        if panels.host.hosted_panel_has_content(*panel) == Some(false) {
            let widget = fixtures::hosted_box(theme, *panel);
            let adopted = panels.host.adopt(*panel, widget);
            assert!(adopted, "{panel:?} is hosted, so it must accept a box");
        }
    }
    panels
}

#[test]
fn every_panel_that_can_take_the_keyboard_renders_a_visible_focus_difference() {
    let theme = theme();
    let mut panels = ready_panels(&theme);
    let mut not_focusable = Vec::new();

    for panel in SidePanelId::ALL {
        for placement in PanelPlacement::ALL {
            panels.host.dock(*panel, *placement);
            // Every edge starts on `Session`, collapsed. `dock` moves a panel
            // that already occupies the target edge without re-expanding it,
            // so a sweep that docks one panel onto more than one edge would
            // otherwise measure a column that is not on screen at all.
            panels.host.set_visible_at(*placement, true);
            let took_focus = panels.host.focus_at(*placement);
            if !took_focus {
                not_focusable.push(*panel);
                continue;
            }
            let focused = panels.capture_live(*placement, COLUMN);
            panels.host.blur();
            let unfocused = panels.capture_live(*placement, COLUMN);
            assert_eq!(
                focus_is_visible(&focused, &unfocused),
                Ok(()),
                "{panel:?} on {placement:?} refuses to take focus or shows no indication of it"
            );
        }
    }

    not_focusable.sort_by_key(|panel| panel.label());
    not_focusable.dedup();
    let mut expected = NOT_FOCUSABLE.to_vec();
    expected.sort_by_key(|panel| panel.label());
    assert_eq!(
        not_focusable, expected,
        "the set of panels that cannot take the keyboard changed; a surface that \
         silently stopped being focusable is the defect I3a exists to catch"
    );
}

#[test]
fn every_dockable_dialog_changes_its_column_when_the_column_is_focused() {
    // The docking authority is `Component::docks_as_viewer`, not
    // `App::dock_target`: `dock_target` answers *which panel a box belongs
    // in*, and it maps several boxes the host must not intercept — a model
    // picker owns its own Shift-qualified keys. Sweeping every mapping would
    // dock boxes production never docks and would read the result as a focus
    // defect.
    let theme = theme();
    let mut app = App::new_for_testing("/tmp".to_string());
    let mut docked = 0usize;
    let mut unhosted: Vec<(&str, SidePanelId)> = Vec::new();
    let mut refused: Vec<(&str, SidePanelId)> = Vec::new();

    for fixture in fixtures::dialogs() {
        let component = (fixture.make)(&theme);
        if !component.docks_as_viewer() {
            continue;
        }
        app.focus_manager.push(component);
        let target = app.dock_target();
        app.focus_manager.pop_dialog(fixture.dialog_type);
        let Some(panel) = target else {
            continue;
        };

        let mut panels = Panels::new(Arc::clone(&theme));
        let widget = (fixture.make)(&theme);
        if !panels.host.adopt(panel, widget) {
            // A bespoke panel keeps its own component; there is no adopted
            // box whose column could show a focus difference.
            unhosted.push((fixture.surface, panel));
            continue;
        }
        for placement in PanelPlacement::ALL {
            panels.host.dock(panel, *placement);
            panels.host.set_visible_at(*placement, true);
            if !panels.host.focus_at(*placement) {
                refused.push((fixture.surface, panel));
                continue;
            }
            let focused = panels.capture_live(*placement, COLUMN);
            panels.host.blur();
            let unfocused = panels.capture_live(*placement, COLUMN);
            assert_eq!(
                focus_is_visible(&focused, &unfocused),
                Ok(()),
                "{} docks into {panel:?} at {placement:?} and shows no focus difference there",
                fixture.surface
            );
            docked += 1;
        }
    }

    assert!(
        docked >= 60,
        "the sweep only docked {docked} dialog placements, which is too few to be the \
         live fan-out"
    );

    unhosted.sort_by_key(|(_, panel)| panel.label());
    unhosted.dedup();
    assert_eq!(
        unhosted,
        vec![
            ("Agent", SidePanelId::Agents),
            ("Terminal", SidePanelId::Terminal)
        ],
        "the set of viewer dialogs whose target panel holds its own component changed; \
         a box that is no longer adopted has lost its column, and one that is adopted \
         here has gained a panel it did not have"
    );
    refused.sort_by_key(|(_, panel)| panel.label());
    refused.dedup();
    assert!(
        refused.is_empty(),
        "these viewer dialogs were adopted into a panel that then refused the \
         keyboard: {refused:?}"
    );
}

#[test]
fn the_focus_difference_check_is_not_vacuous() {
    // I3a's acceptance evidence: two renders of the same unfocused panel must
    // not read as a focus difference, and a focused render must. M002 widens
    // the difference beyond the `●` to the border and title styles, so this
    // stays the check on *whether* focus is visible rather than on how.
    let theme = theme();
    let area = Rect::new(0, 0, 40, 12);
    let without_marker = capture::hosted(
        &mut dialog(&theme),
        area,
        &theme,
        SidePanelId::Logs.label(),
        false,
    );
    let also_without_marker = capture::hosted(
        &mut dialog(&theme),
        area,
        &theme,
        SidePanelId::Logs.label(),
        false,
    );
    let verdict = focus_is_visible(&without_marker, &also_without_marker);
    assert!(
        verdict.is_err(),
        "two unfocused renders must not read as a focus difference"
    );

    let with_marker = capture::hosted(
        &mut dialog(&theme),
        area,
        &theme,
        SidePanelId::Logs.label(),
        true,
    );
    assert_eq!(
        focus_is_visible(&with_marker, &also_without_marker),
        Ok(()),
        "a focused render must read as a focus difference"
    );
}

fn dialog(theme: &Arc<Theme>) -> codegg::tui::components::dialogs::info::InfoDialog {
    codegg::tui::components::dialogs::info::InfoDialog::new(
        Arc::clone(theme),
        codegg::tui::components::dialogs::info::InfoType::Logs,
        vec!["one".to_string(), "two".to_string()],
    )
}
