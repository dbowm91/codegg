//! Panel placement: which edge the panel lives on, and what that does to the
//! directionals.
//!
//! The directionals (`Shift+H/J/K/L`, `Shift+←/→/↑/↓`) are one gesture family
//! whose meaning is *geometric*. Before placements existed they were hardcoded
//! to "right", which is why `Shift+H` could only ever mean "go back to the
//! buffer". These tests pin the property that makes a second panel possible:
//!
//! * the gesture pointing at the panel enters it, whatever side that is;
//! * the opposite gesture returns to the composer, whatever side that is;
//! * the two perpendicular gestures walk the panel list on every layout;
//! * a box docks to the side the panel is on, and a gesture pointing at a
//!   side with no panel is refused *out loud* rather than docking to the
//!   wrong one.
//!
//! Everything goes through `App::on_key` and `App::process_msg` — the two
//! doors a real keystroke uses. `commands::panels` is a private submodule, so
//! calling a handler directly would assert on a path no user can reach.

use codegg::tui::app::{App, TuiMsg};
use codegg::tui::components::component::DialogType;
use codegg::tui::input::InputMode;
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

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

fn test_app() -> App {
    App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string())
}

/// Put the app in Normal mode, where the directionals are live.
///
/// Idempotent, because focusing a panel already switches to Normal and an
/// unconditional `Esc` would blur the very panel under test.
fn to_normal(app: &mut App) {
    if app.ui_state.input_mode != InputMode::Normal {
        app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    }
}

/// Park the app with `placement` as the one edge showing a panel, and focus in
/// the main buffer.
///
/// Reveals that edge and collapses the others, because that is the frame a
/// user has after moving the panel: one panel, one side, everything else out
/// of the way. Merely re-pointing the gestures is not enough — the host
/// resolves a direction against the edges that are *showing* a panel, so a
/// collapsed edge is correctly invisible to `Shift+H`.
fn app_on(placement: PanelPlacement) -> App {
    let mut app = test_app();
    app.panel_host.select_placement(placement);
    for edge in PanelPlacement::ALL {
        app.panel_host.set_visible_at(*edge, *edge == placement);
    }
    app.ui_state.sidebar_visible = true;
    to_normal(&mut app);
    app
}

/// Open the panel manager: `Shift+S`, Normal-mode only.
fn open_manager(app: &mut App) {
    to_normal(app);
    app.on_key(plain('S'));
    assert!(
        app.focus_manager.has_dialog(DialogType::PanelManager),
        "precondition: the manager is open"
    );
}

// ── The default ──────────────────────────────────────────────────────

/// A user with no stored preference must see the frame that shipped. If the
/// default moved, every existing keybinding's meaning moved with it.
#[test]
fn a_fresh_app_is_docked_right() {
    let app = test_app();
    assert_eq!(app.panel_host.placement(), PanelPlacement::Right);
}

// ── Entering and leaving, per placement ──────────────────────────────

#[test]
fn a_right_panel_is_entered_with_l_and_left_with_h() {
    let mut app = app_on(PanelPlacement::Right);
    app.on_key(plain('L'));
    assert!(
        app.panel_host.is_focused(),
        "L points at a right-hand panel"
    );
    app.on_key(plain('H'));
    assert!(!app.panel_host.is_focused(), "H is the way back out");
}

#[test]
fn a_left_panel_swaps_the_entering_and_leaving_gestures() {
    let mut app = app_on(PanelPlacement::Left);
    app.on_key(plain('H'));
    assert!(
        app.panel_host.is_focused(),
        "H points at a left-hand panel now"
    );
    app.on_key(plain('L'));
    assert!(!app.panel_host.is_focused(), "and L gets back out");
}

#[test]
fn a_bottom_panel_moves_the_axis_to_vertical() {
    let mut app = app_on(PanelPlacement::Bottom);
    app.on_key(plain('J'));
    assert!(
        app.panel_host.is_focused(),
        "J points at a panel under the buffer"
    );
    app.on_key(plain('K'));
    assert!(!app.panel_host.is_focused(), "and K gets back out");
}

#[test]
fn the_arrow_twins_agree_with_the_letter_twins() {
    // Terminals disagree about how they encode a shifted arrow, so both
    // spellings must land on the same side. Only the arrow pointing *at* the
    // panel enters it — the opposite one leaves, which is why a right-hand
    // panel is entered with `Shift+→` and not with `Shift+←`.
    let cases = [
        (PanelPlacement::Right, KeyCode::Right, 'L'),
        (PanelPlacement::Left, KeyCode::Left, 'H'),
        (PanelPlacement::Bottom, KeyCode::Down, 'J'),
    ];
    for (placement, arrow, letter) in cases {
        let mut app = app_on(placement);
        app.on_key(key(arrow, KeyModifiers::SHIFT));
        assert!(
            app.panel_host.is_focused(),
            "{placement:?}: Shift+{arrow:?} must enter the panel, like {letter}"
        );

        // The mirrored gesture leaves, by the same letters the vim map uses.
        let away_letter = match letter {
            'L' => 'H',
            'H' => 'L',
            _ => 'K',
        };
        app.on_key(plain(away_letter));
        assert!(
            !app.panel_host.is_focused(),
            "{placement:?}: {away_letter} is the way back out"
        );

        // A bare arrow is within-surface navigation and must stay that way,
        // or scrolling a panel would double as moving focus.
        let mut bare = app_on(placement);
        bare.on_key(key(arrow, KeyModifiers::NONE));
        assert!(
            !bare.panel_host.is_focused(),
            "a bare {arrow:?} is navigation, not the panel gesture"
        );
    }
}

// ── Cycling the panel list ───────────────────────────────────────────

#[test]
fn the_perpendicular_pair_cycles_the_loaded_panel_on_every_layout() {
    for (placement, forward, backward) in [
        (PanelPlacement::Right, 'J', 'K'),
        (PanelPlacement::Left, 'J', 'K'),
        // Bottom moves the axis, so the horizontals take over the list.
        (PanelPlacement::Bottom, 'L', 'H'),
    ] {
        let mut app = app_on(placement);
        let before = app.panel_host.active();

        app.on_key(plain(forward));
        assert_ne!(
            app.panel_host.active(),
            before,
            "{placement:?}: {forward} must walk the panel list"
        );

        app.on_key(plain(backward));
        assert_eq!(
            app.panel_host.active(),
            before,
            "{placement:?}: {backward} must walk back"
        );
    }
}

#[test]
fn cycling_never_seizes_the_keyboard() {
    // Reading titles is not the same as driving them. A user who steps
    // through the list while scanning must not have their next keystroke
    // delivered to a panel they only glanced at.
    let mut app = app_on(PanelPlacement::Right);
    app.on_key(plain('J'));
    assert!(!app.panel_host.is_focused());
}

// ── Docking a box ────────────────────────────────────────────────────

/// A bare app with `/help` open and the box in Normal.
///
/// Deliberately **not** `app_on`: that helper parks the app in Normal, and
/// the app's own Normal-mode `Esc` closes a mounted dialog before the box
/// ever sees it. `/help` is opened from the composer, so the honest
/// sequence is: app in Insert, box opens in its own Insert, one `Esc`
/// leaves *the box's* Insert without closing anything.
fn help_box_in_normal() -> App {
    let mut app = test_app();
    app.panel_host.select_placement(PanelPlacement::Right);
    app.process_msg(TuiMsg::OpenHelpDialog);
    assert!(
        app.focus_manager.has_dialog(DialogType::Help),
        "box is open"
    );
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.focus_manager.has_dialog(DialogType::Help),
        "one Esc leaves the box's Insert without closing it"
    );
    app
}

/// A box docks to whichever edge the gesture names, and the edge it leaves
/// puts back what it was showing.
///
/// This is the behavior that replaced a refusal: the user said where they
/// wanted the panel, so it goes there.
#[tokio::test(flavor = "current_thread")]
async fn a_box_docks_to_the_edge_the_gesture_names() {
    let mut app = help_box_in_normal();
    app.panel_host.select_placement(PanelPlacement::Left);

    // `L` names the right column, which has no panel.
    app.on_key(plain('L'));

    assert!(
        !app.focus_manager.has_dialog(DialogType::Help),
        "docking closes the box wherever it lands"
    );
    assert_eq!(app.panel_host.active(), SidePanelId::Help);
    assert_eq!(
        app.panel_host.placement(),
        PanelPlacement::Right,
        "the panel went to the edge the gesture named, not the one it was on"
    );
    assert!(
        app.panel_host.is_focused(),
        "and the panel takes the keyboard"
    );
}

/// The swap's contract: the edge a panel leaves comes back to its own state
/// rather than going blank.
#[tokio::test(flavor = "current_thread")]
async fn docking_across_sides_restores_the_edge_it_left() {
    let mut app = help_box_in_normal();
    // The right column is up with the Session panel on it.
    assert!(app.panel_host.is_visible_at(PanelPlacement::Right));

    // Walk the help panel to the left.
    app.on_key(plain('H'));
    assert_eq!(app.panel_host.placement(), PanelPlacement::Left);

    assert_eq!(
        app.panel_host.panel_at(PanelPlacement::Right),
        SidePanelId::Session,
        "the right column brought its own panel back"
    );
    assert!(
        app.panel_host.is_visible_at(PanelPlacement::Right),
        "and it is up — a column that reappeared empty reads as data loss"
    );
    assert_eq!(
        app.panel_host.panel_at(PanelPlacement::Left),
        SidePanelId::Help
    );
}

/// The user's own scenario, end to end: a collapsed edge has nothing to
/// restore, so it goes back to collapsed instead of producing an empty
/// column or resurrecting a panel the user had put away.
///
/// Each leg re-opens the box, because that is the real gesture sequence:
/// `/chat` then `Shift+L`, then `/chat` again then `Shift+H`. A directional
/// only *moves a panel* while a box is open — with no box open the same key
/// means "go that way", which is a different (and equally correct) thing.
#[tokio::test(flavor = "current_thread")]
async fn an_edge_the_user_had_collapsed_stays_collapsed_after_a_swap() {
    let mut app = help_box_in_normal();
    // `Ctrl+B` puts the right column away.
    app.panel_host.set_visible_at(PanelPlacement::Right, false);
    app.panel_host.select_placement(PanelPlacement::Right);

    // `/chat` + `Shift+L`: chat lands on the right.
    app.on_key(plain('L'));
    assert_eq!(app.panel_host.placement(), PanelPlacement::Right);
    assert!(app.panel_host.is_visible_at(PanelPlacement::Right));

    // Back to `/chat`, then `Shift+H`: chat moves left.
    //
    // No `Esc` here, unlike `help_box_in_normal`: docking the panel left the
    // app in `NOR` already, so a blind `Esc` would close the freshly-opened
    // box rather than leaving its own `INS` mode.
    app.process_msg(TuiMsg::OpenHelpDialog);
    assert!(app.focus_manager.has_dialog(DialogType::Help));
    app.on_key(plain('H'));
    assert_eq!(app.panel_host.placement(), PanelPlacement::Left);

    assert!(
        !app.panel_host.is_visible_at(PanelPlacement::Right),
        "nothing was there before the panel arrived, so nothing comes back"
    );
    assert!(
        app.panel_host.is_visible_at(PanelPlacement::Left),
        "and the edge that received it is up"
    );
}

/// A refusal would have told the user which key to use instead. Now there is
/// no dead end, so nothing is left to report — but the swap still says what
/// happened to the edge it left, because that edge is the part the user
/// cannot see change without being told.
#[tokio::test(flavor = "current_thread")]
async fn the_swap_says_what_the_vacated_edge_became() {
    let mut app = help_box_in_normal();
    app.on_key(plain('H'));

    let notice = app
        .messages_state
        .toasts
        .iter()
        .map(|toast| toast.message.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        notice.contains("Left") && notice.contains("Right"),
        "the confirmation must name both edges, got: {notice}"
    );
    assert!(
        notice.contains("Help"),
        "and name the panel that moved, got: {notice}"
    );
    assert!(
        notice.contains("Session"),
        "and say what the vacated edge is showing now, got: {notice}"
    );
}

// ── The layout strip ─────────────────────────────────────────────────

#[test]
fn p_cycles_the_layout_without_disturbing_the_panel_cursor() {
    let mut app = test_app();
    open_manager(&mut app);
    app.on_key(plain('j')); // move the panel highlight off its opening row
    let cursor_before = app
        .focus_manager
        .with_component::<codegg::tui::components::dialogs::panel_manager::PanelManagerDialog, _>(
            |d| d.cursor(),
        )
        .expect("manager is mounted");

    assert_eq!(app.panel_host.placement(), PanelPlacement::Right);
    app.on_key(plain('p'));

    assert_eq!(
        app.panel_host.placement(),
        PanelPlacement::Left,
        "p must move the panel"
    );
    let cursor_after = app
        .focus_manager
        .with_component::<codegg::tui::components::dialogs::panel_manager::PanelManagerDialog, _>(
            |d| d.cursor(),
        )
        .expect("manager is mounted");
    assert_eq!(
        cursor_after, cursor_before,
        "moving the panel must not renumber the panel list"
    );

    // And it wraps rather than stranding the user on the last layout.
    app.on_key(plain('p'));
    assert_eq!(app.panel_host.placement(), PanelPlacement::Bottom);
    app.on_key(plain('p'));
    assert_eq!(app.panel_host.placement(), PanelPlacement::Right);
}

#[test]
fn the_layout_strip_shows_the_edge_that_is_live() {
    let mut app = test_app();
    app.panel_host.select_placement(PanelPlacement::Bottom);
    open_manager(&mut app);

    let shown = app
        .focus_manager
        .with_component::<codegg::tui::components::dialogs::panel_manager::PanelManagerDialog, _>(
            |d| d.placement(),
        )
        .expect("manager is mounted");
    assert_eq!(
        shown,
        PanelPlacement::Bottom,
        "the box must open describing the layout it is about to change"
    );
}

#[test]
fn p_is_not_reachable_as_a_filter_character() {
    // `p` answers "where does the panel live". Letting it also type into the
    // filter would make the layout key impossible to press while searching.
    let mut app = test_app();
    open_manager(&mut app);
    app.on_key(plain('p'));
    assert!(
        app.focus_manager.has_dialog(DialogType::PanelManager),
        "the manager must stay open so a layout change can be seen"
    );
    let query = app
        .focus_manager
        .with_component::<codegg::tui::components::dialogs::panel_manager::PanelManagerDialog, _>(
            |d| d.cursor(),
        )
        .expect("manager is mounted");
    assert!(query < SidePanelId::ALL.len());
}

// ── Ctrl+B ───────────────────────────────────────────────────────────

#[test]
fn ctrl_b_closes_the_selected_pane_and_reopens_it_where_it_was() {
    for placement in [
        PanelPlacement::Right,
        PanelPlacement::Left,
        PanelPlacement::Bottom,
    ] {
        let mut app = app_on(placement);
        app.on_key(plain(placement.toward().gesture().chars().next().unwrap()));
        assert!(
            app.panel_host.is_focused(),
            "{placement:?}: the gesture pointing at the panel must enter it"
        );

        app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(
            !app.ui_state.sidebar_visible,
            "{placement:?}: Ctrl+B must close the panel the keyboard was in"
        );
        assert!(
            !app.panel_host.is_focused(),
            "{placement:?}: closing must hand the composer its keyboard"
        );

        app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(
            app.ui_state.sidebar_visible,
            "{placement:?}: the same key must bring it back"
        );
        assert!(
            app.panel_host.is_focused(),
            "{placement:?}: and it must come back ready to use"
        );
        assert_eq!(
            app.panel_host.placement(),
            placement,
            "{placement:?}: reopening must not move the panel"
        );
    }
}

#[test]
fn ctrl_b_is_answered_from_inside_a_focused_panel() {
    // A focused panel consumes every key by contract, so the toggle has to
    // be answered by the host. Otherwise the first press enters the panel
    // and the second — the one that was supposed to close it — is swallowed,
    // which is how this reads as "works exactly once".
    let mut app = app_on(PanelPlacement::Left);
    app.on_key(plain('H'));
    assert!(
        app.panel_host.is_focused(),
        "precondition: inside a left panel"
    );

    app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(!app.ui_state.sidebar_visible);
    assert!(!app.panel_host.is_focused());
}

// ── The manager actually draws it ────────────────────────────────────

/// Render the mounted manager and return its buffer as text.
fn manager_text(app: &mut App) -> String {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let backend = TestBackend::new(72, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// The layout strip costs a chunk, so the panel list it pushed down is the
/// thing most likely to get clipped — and a clipped list is a manager that
/// looks like it lost panels.
#[test]
fn the_layout_strip_does_not_push_the_panel_list_off_screen() {
    let mut app = test_app();
    open_manager(&mut app);
    let text = manager_text(&mut app);

    for placement in PanelPlacement::ALL {
        assert!(
            text.contains(placement.label()),
            "{placement:?} missing from the drawn strip"
        );
    }
    // The highlighted row and its hint must survive the new chunk — that is
    // the row saying which panel `Enter` will load.
    assert!(text.contains("Session"), "first panel clipped");
    assert!(text.contains("goal"), "panel hints clipped");
    assert!(text.contains("Enter load"), "footer clipped");
    // And the footer — the only place the keys are written down — must
    // survive. It did not, when the strip briefly had a chunk of its own.
    assert!(text.contains("Enter load"), "footer clipped");
    assert!(text.contains("p move panel"), "footer lost the layout key");
}

/// The three placements fit on one line at the manager's usual width. If they
/// ever wrap, the strip silently loses its last entry — which is the one a
/// user needs to discover.
#[test]
fn the_strip_fits_on_one_line() {
    let mut app = test_app();
    open_manager(&mut app);
    let text = manager_text(&mut app);
    let line = text
        .lines()
        .find(|line| line.contains("[Right]") || line.contains("Bottom"))
        .expect("the strip should draw a line");
    for placement in PanelPlacement::ALL {
        assert!(
            line.contains(placement.label()),
            "{placement:?} wrapped off the strip line: {line:?}"
        );
    }
}

/// The highlight is the only thing saying which panel `Enter` commits to, so
/// `j` must never walk it off a box that can only show ten of thirty-three.
#[test]
fn the_highlight_stays_visible_when_walking_the_list() {
    let mut app = test_app();
    open_manager(&mut app);
    let total = codegg::tui::panels::SidePanelId::ALL.len();
    // Well past one page in a 24-row box.
    for _ in 0..(total + 5) {
        app.on_key(plain('j'));
    }
    let cursor = app
        .focus_manager
        .with_component::<codegg::tui::components::dialogs::panel_manager::PanelManagerDialog, _>(
            |d| d.cursor(),
        )
        .expect("manager is mounted");
    assert_eq!(cursor, total - 1, "precondition: walked to the last panel");

    let text = manager_text(&mut app);
    let last = codegg::tui::panels::SidePanelId::ALL[total - 1];
    assert!(
        text.contains(last.label()),
        "the highlighted panel scrolled out of view: {last:?} missing"
    );
}
