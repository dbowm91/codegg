//! Panel reachability and placeholder honesty.
//!
//! The manager (`⇧S`, typed `/panels`) is the one place a panel is chosen by
//! name. Selecting a row there is a single act, so what the column says next
//! has to be the truth about that act: the panel fills, a load started and the
//! panel says so, or the panel says why it cannot act.
//!
//! These tests drive the real path — `App::on_key` into the manager, `Enter` to
//! commit, then a real frame — and read the **rendered column**, not internal
//! state, because the defect being pinned here is a sentence on screen.
//!
//! The suite that already exists (`tui_panel_docking.rs`) covers the docking
//! *gesture* for seven boxes. It cannot cover this: nothing there walks
//! `SidePanelId::ALL`, so a panel that is listed but can never act passes
//! today.

use codegg::tui::app::{App, TuiMsg};
use codegg::tui::components::component::DialogType;
use codegg::tui::input::InputMode;
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::layout::Rect;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn plain(ch: char) -> KeyEvent {
    key(KeyCode::Char(ch), KeyModifiers::NONE)
}

/// A fresh app on a frame wide enough that the column is readable.
///
/// The toast overlay is pinned to `x = width - 60`, so at this width it sits
/// over the main buffer. That is what makes the **left** edge usable as the
/// column under test: the panel and the toast cannot overlap, so a toast can
/// no longer hide panel text and quietly turn an assertion into a no-op.
fn test_app() -> App {
    App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string())
}

/// Park the panel under test on the **left** column.
///
/// Not cosmetic — it is what keeps [`panel_column`] measuring the panel, and
/// it collapses the default right column so exactly one `HostPanel` competes
/// for the assertion.
fn park_on_the_left(app: &mut App, id: SidePanelId) {
    app.panel_host.dock(id, PanelPlacement::Left);
    // `dock` *swaps* two slots and restores whatever each held, and every slot
    // starts life holding `Session`, so a bare `dock` leaves the left slot's
    // visibility as a side effect of the swap. State it outright.
    app.panel_host.set_visible_at(PanelPlacement::Left, true);
    app.panel_host.set_visible_at(PanelPlacement::Right, false);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
}

/// Commit `id` from the manager, the way a user does.
///
/// The manager opens its cursor on whichever panel is already active, so the
/// walk clamps to the top of the registry first and then steps down to the
/// row under test — otherwise this would silently drift as soon as the default
/// selection changed.
fn commit_from_manager(app: &mut App, id: SidePanelId) {
    app.ui_state.input_mode = InputMode::Normal;
    app.on_key(key(KeyCode::Char('S'), KeyModifiers::SHIFT));
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::PanelManager,
        "the manager did not open, so this test would be asserting on the composer"
    );
    for _ in 0..=SidePanelId::ALL.len() {
        app.on_key(plain('k'));
    }
    for _ in 0..id.index() {
        app.on_key(plain('j'));
    }
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
}

/// The rendered panel column as text.
///
/// Two things this deliberately does **not** trust, both learned the hard way:
///
/// * `sidebar_area` — it is only set when the occupied edge happens to be
///   showing `Session`, so for any other panel it is `None` and every
///   `!text.contains(…)` below would silently pass on an empty string.
///   `panel_areas` is rebuilt each frame alongside it, so it cannot name a
///   column that is not painted.
/// * the toast overlay — it paints a fixed block at `x = width - 60`, which
///   lands on the default right column and hid the very text under test.
///   [`park_on_the_left`] moves the column clear of it.
fn panel_column(app: &mut App) -> String {
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    let Some((_, rect)) = app
        .panel_areas
        .iter()
        .find(|(placement, _)| *placement == app.panel_host.placement())
        .copied()
    else {
        return String::new();
    };
    // The panel column and the toast block must not intersect, or this read is
    // measuring the overlay instead of the panel.
    let toast = Rect {
        x: 120 - 60,
        y: 2,
        width: 60,
        height: 10,
    };
    assert!(
        rect.x + rect.width <= toast.x || toast.x + toast.width <= rect.x,
        "the toast overlay covers the column under test ({rect:?} vs {toast:?}) — \
         move the panel to an edge the overlay cannot reach"
    );
    let buf = terminal.backend().buffer();
    (rect.y..rect.y + rect.height)
        .map(|y| {
            (rect.x..rect.x + rect.width)
                .map(|x| {
                    buf.cell((x, y))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Commit `id` on the left edge and read its column, insisting it rendered.
///
/// Every assertion in this file is a `!text.contains(…)`, so an empty string
/// turns all of them into no-ops. That is not hypothetical: an earlier revision
/// of this helper read `sidebar_area`, and three of these four tests "passed"
/// against nothing.
fn committed_column(app: &mut App, id: SidePanelId) -> String {
    park_on_the_left(app, id);
    commit_from_manager(app, id);
    let text = panel_column(app);
    assert!(
        !text.trim().is_empty(),
        "{id:?} committed but no panel column was painted, so nothing below is being checked"
    );
    text
}

/// Every panel in the registry, in manager order, committed one at a time.
fn committed_panels() -> Vec<SidePanelId> {
    SidePanelId::ALL.to_vec()
}

/// The panel must never tell the user to leave it in order to fill it.
///
/// The column is already the panel: there is no box to move into it, and
/// `Shift+←` names a different edge on every placement. Naming a `/` command
/// makes the user believe the panel could have done the work itself.
///
/// The per-panel "does not name *its* loader" check lives with the placeholder
/// itself (`panels::hosted`), which can assert it against every state. What is
/// new here is the sweep: that *no* panel in the registry does it.
#[test]
fn committing_a_panel_never_asks_the_user_to_go_and_come_back() {
    for id in committed_panels() {
        let mut app = test_app();
        let text = committed_column(&mut app, id);

        assert!(
            !text.contains("Shift+←"),
            "{id:?} told the user to move a box that is already in place, got {text:?}"
        );
        assert!(
            !text.contains("to open it"),
            "{id:?} told the user to go run a command from the input box, got {text:?}"
        );
    }
}

/// After an explicit commit the column may not claim that nothing was loaded.
///
/// This is the honesty sweep. A panel that filled draws its box; a panel whose
/// load started shows progress; a panel that cannot act states why. "No X
/// loaded" was none of those — it was the manager failing to admit it had done
/// nothing, and it was on screen for every panel in the registry.
#[test]
fn committing_a_panel_never_leaves_it_claiming_nothing_is_loaded() {
    for id in committed_panels() {
        let mut app = test_app();
        let text = committed_column(&mut app, id);
        let claim = format!("No {} loaded", id.label());

        assert!(
            !text.contains(&claim),
            "{id:?} was committed and still claims {claim:?}, got {text:?}"
        );
    }
}

/// Every panel in the registry must be able to *act*.
///
/// This is the reachability guard. A row can reach `SidePanelId::ALL` by being
/// added to the enum, and nothing else stops it: a bespoke-only row, a panel
/// whose constructor is `#[cfg(test)]`, or a panel whose arm only toasts all
/// reach the manager identically and can only ever render a placeholder. The
/// sweep walks the whole registry through the real gesture, so adding one is
/// no longer enough — committing it has to produce something.
///
/// Bespoke panels are skipped because they are not hosted and never show a
/// placeholder; they own their own live view by definition.
#[test]
fn committing_a_panel_never_leaves_it_in_the_default_empty_state() {
    for id in committed_panels() {
        if id.is_bespoke() {
            continue;
        }
        let mut app = test_app();
        let text = committed_column(&mut app, id);

        assert!(
            !text.contains("Nothing loaded."),
            "{id:?} is listed in the manager, was committed, and still has nothing to say — \
             either no box can fill it or its `open_panel_box` arm never declared why not"
        );
    }
}

/// A box that has no panel to receive it must say so and stay open.
///
/// `Dialog::Agent` is the agent **editor**; the `Agents` panel is the management
/// **rows** list. `SidePanelHost::adopt` answers `false` for it and consumes the
/// component it was handed, so docking used to unmount the editor, throw it
/// away unread, and focus a panel showing something else. The check is that the
/// box is still there afterwards — a refusal that unmounts first would pass
/// every state assertion and still lose the user's work.
#[test]
fn a_box_with_no_panel_to_receive_it_is_refused_before_it_is_unmounted() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.process_msg(TuiMsg::OpenAgentDialog);
    assert!(
        app.focus_manager.active_dialog_type() == DialogType::Agent,
        "precondition: the agent editor box is mounted"
    );

    app.on_key(key(KeyCode::Char('L'), KeyModifiers::SHIFT));

    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::Agent,
        "the editor box was unmounted for a panel that cannot hold it"
    );
}

/// A load in flight must not be described as an empty panel.
#[test]
fn a_panel_whose_load_is_in_flight_does_not_read_as_an_empty_panel() {
    let mut app = test_app();
    // Wired so `open_panel_box` takes the "start the load" branch rather than
    // the "no core client" one.
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    app.tui_cmd_tx = Some(tx);

    let text = committed_column(&mut app, SidePanelId::Doctor);

    assert!(
        !text.contains("No Doctor loaded"),
        "doctor's load is running; the column must show progress, got {text:?}"
    );
}
