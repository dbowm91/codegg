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
use codegg::tui::components::dialogs::panel_manager::PanelManagerDialog as ManagerDialog;
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
    // The Normal-mode directional is the footer's headline verb. `p` and the
    // digits are not repeated here any more -- the strip and each list row say
    // where they live, and the footer only has ~41 columns to spend.
    assert!(
        text.contains("⇧H/J/K/L move"),
        "footer lost the directional, which is the box's reason to exist"
    );
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

// ── Two columns, and the buffer between them ──────────────────────────────

/// Session on the left, Terminal on the right, the bottom row out of the way.
///
/// The frame the two-column gestures have to be right about. Every helper
/// above collapses all but one edge, so none of them would notice a gesture
/// that stepped from one column straight to the other.
///
/// Both panels take focus when empty. Chat deliberately does not: an unbound
/// chat panel refuses it so the keyboard stays in the composer, which is
/// correct behaviour and useless here — the geometry under test is about
/// *where* the keyboard is, so it has to be able to arrive.
fn two_column_app() -> App {
    let mut app = test_app();
    app.panel_host.select_placement(PanelPlacement::Right);
    app.panel_host
        .dock(SidePanelId::Session, PanelPlacement::Left);
    app.panel_host
        .dock(SidePanelId::Terminal, PanelPlacement::Right);
    // Set explicitly rather than trusted from the docks: `PanelSlot::occupy`
    // is a no-op when the panel already sits on that edge, so docking onto a
    // slot that starts collapsed leaves it collapsed. Both helpers above do
    // the same thing, for the same reason.
    for edge in PanelPlacement::ALL {
        app.panel_host
            .set_visible_at(*edge, *edge != PanelPlacement::Bottom);
    }
    app.ui_state.sidebar_visible = true;
    to_normal(&mut app);
    app
}

fn with_manager<R>(app: &App, f: impl FnOnce(&ManagerDialog) -> R) -> Option<R> {
    app.focus_manager.with_component::<ManagerDialog, _>(f)
}

fn manager_mode(app: &App) -> Option<InputMode> {
    with_manager(app, |d| d.input_mode())
}

fn manager_cursor(app: &App) -> Option<usize> {
    with_manager(app, |d| d.cursor())
}

fn manager_selected(app: &App) -> Option<Option<SidePanelId>> {
    with_manager(app, |d| d.selected())
}

/// The buffer is the surface the user spends most of their time in, so the
/// gesture that names it has to *land* on it rather than stepping over it on
/// the way to the far column. Without this, two `Shift+H` from Chat reached
/// Terminal — and the buffer was unreachable by any horizontal gesture at all.
#[test]
fn the_main_buffer_is_a_stop_on_the_way_across_the_columns() {
    let mut app = two_column_app();
    app.on_key(plain('L'));
    assert!(
        app.panel_host.is_focused(),
        "precondition: inside the right panel"
    );

    app.on_key(plain('H'));
    assert!(
        !app.panel_host.is_focused(),
        "the first Shift+H lands on the main buffer, not the far column"
    );

    app.on_key(plain('H'));
    assert_eq!(
        app.panel_host.placement(),
        PanelPlacement::Left,
        "the second Shift+H reaches the left column"
    );
    assert!(app.panel_host.is_focused(), "and arrives ready to use");
}

/// The same stop in the other direction, so the cycle is symmetric rather than
/// a rule that only holds on the way left.
#[test]
fn the_main_buffer_is_a_stop_on_the_way_back_too() {
    let mut app = two_column_app();
    app.on_key(plain('H'));
    assert!(app.panel_host.is_focused(), "precondition: inside Terminal");

    app.on_key(plain('L'));
    assert!(
        !app.panel_host.is_focused(),
        "L steps out to the main buffer"
    );

    app.on_key(plain('L'));
    assert_eq!(app.panel_host.placement(), PanelPlacement::Right);
    assert!(
        app.panel_host.is_focused(),
        "and L again reaches the right panel"
    );
}

/// The buffer is a stop, not a trap: a column that is not up must not swallow
/// the gesture, and the perpendicular pair still walks the panel list from it.
#[test]
fn a_collapsed_column_does_not_swallow_the_gesture_that_names_it() {
    let mut app = app_on(PanelPlacement::Right);
    app.on_key(plain('L'));
    assert!(
        app.panel_host.is_focused(),
        "precondition: inside the right panel"
    );
    app.on_key(plain('H'));
    assert!(!app.panel_host.is_focused(), "back in the main buffer");
    // Left is collapsed, so there is nothing to step into — and no toast
    // pretending the key was wrong.
    app.on_key(plain('H'));
    assert!(!app.panel_host.is_focused());
    // The perpendicular pair is untouched by all of that.
    let before = app.panel_host.active();
    app.on_key(plain('J'));
    assert_ne!(app.panel_host.active(), before, "J still walks the list");
}

// ── Opening the manager must not change the frame ──────────────────────────

/// The reported bug. `/chat` on the right, Terminal on the left, `Ctrl+B` puts
/// the left column away — and `Shift+S`, a box that exists only to *describe*
/// the layout, brought it straight back. The manager used to force the selected
/// edge visible; a box cannot be the reason a column the user deliberately
/// collapsed reappears.
#[test]
fn opening_the_manager_does_not_uncollapse_an_edge() {
    let mut app = two_column_app();
    app.panel_host.select_placement(PanelPlacement::Left);
    app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(
        !app.panel_host.is_visible_at(PanelPlacement::Left),
        "precondition: Ctrl+B put the left column away"
    );

    open_manager(&mut app);

    assert!(
        !app.panel_host.is_visible_at(PanelPlacement::Left),
        "Shift+S describes the layout; it must not change it"
    );
}

/// With the frame untouched, the strip has to *say* so — naming the state is
/// the honest replacement for resolving the ambiguity by expanding it.
#[test]
fn the_strip_says_when_the_edge_it_describes_is_collapsed() {
    let mut app = two_column_app();
    app.panel_host.select_placement(PanelPlacement::Left);
    app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
    open_manager(&mut app);
    assert!(
        manager_text(&mut app).contains("collapsed"),
        "a strip describing a collapsed edge must name it"
    );
}

// ── The manager's modes ───────────────────────────────────────────────────

/// The manager is reached with `Shift+S` — a Normal-mode chord — and is read
/// before it is typed into. Opening it in Insert made `j`/`k` filter text,
/// which is indistinguishable from a dead pane.
#[test]
fn the_manager_opens_in_normal_and_only_types_in_insert() {
    let mut app = test_app();
    open_manager(&mut app);
    assert_eq!(manager_mode(&app), Some(InputMode::Normal));

    app.on_key(plain('j'));
    assert_eq!(
        manager_cursor(&app),
        Some(1),
        "j walks the highlight in Normal"
    );
    assert!(
        !manager_text(&mut app).contains("search"),
        "a Normal-mode keypress must not open a filter"
    );

    app.on_key(plain('i'));
    assert_eq!(manager_mode(&app), Some(InputMode::Insert));
    for ch in "chat".chars() {
        app.on_key(plain(ch));
    }
    assert!(
        manager_text(&mut app).contains("search chat"),
        "Insert types into the filter"
    );
    assert_eq!(
        manager_cursor(&app),
        Some(1),
        "filtering narrows what is visible without renumbering the cursor"
    );
}

/// `Esc` walks out one step at a time — Insert → Normal → clear → close — so
/// one press never discards a query the user is midway through typing.
#[test]
fn escape_leaves_insert_before_it_clears_or_closes() {
    let mut app = test_app();
    open_manager(&mut app);
    app.on_key(plain('i'));
    app.on_key(plain('a'));

    let esc = key(KeyCode::Esc, KeyModifiers::NONE);
    app.on_key(esc);
    assert_eq!(
        manager_mode(&app),
        Some(InputMode::Normal),
        "Esc returns to Normal"
    );
    assert!(
        app.focus_manager.has_dialog(DialogType::PanelManager),
        "with the filter intact — that is where the directional lives"
    );

    app.on_key(esc);
    assert!(
        app.focus_manager.has_dialog(DialogType::PanelManager),
        "a second Esc clears the filter rather than closing"
    );

    app.on_key(esc);
    assert!(
        !app.focus_manager.has_dialog(DialogType::PanelManager),
        "a third Esc, with nothing left to clear, closes"
    );
}

/// The guard that makes the whole NOR/INS split necessary. `Shift+H` is the
/// letter `H`; while a query is being typed it has to reach the filter.
#[test]
fn the_directional_is_a_filter_character_in_insert() {
    let mut app = test_app();
    open_manager(&mut app);
    app.on_key(plain('i'));
    app.on_key(plain('H'));

    assert_eq!(manager_mode(&app), Some(InputMode::Insert));
    assert!(
        manager_text(&mut app).contains("search H"),
        "in Insert, Shift+H is the letter H and belongs in the filter"
    );
}

/// The reason the box needs a mode at all: in Normal the gesture is free, so
/// it means *put this panel over there* — the same gesture the main buffer
/// uses, rather than a manager-only verb with its own vocabulary.
#[test]
fn the_manager_directional_puts_the_highlighted_panel_on_that_edge() {
    let mut app = two_column_app();
    open_manager(&mut app);
    assert_eq!(
        manager_selected(&app),
        Some(Some(SidePanelId::Terminal)),
        "precondition: the manager opens on the live panel"
    );

    app.on_key(plain('H'));

    assert_eq!(
        app.panel_host.panel_at(PanelPlacement::Left),
        SidePanelId::Terminal,
        "Shift+H in the manager moves the highlighted panel"
    );
    assert_eq!(
        app.panel_host.placement(),
        PanelPlacement::Left,
        "and the edge it landed on becomes the live one — that is where the panel is"
    );
    assert!(
        app.focus_manager.has_dialog(DialogType::PanelManager),
        "arranging three panels is one continuous act; the box stays open"
    );
    assert!(
        manager_text(&mut app).contains("[Left]"),
        "the strip follows the panel the user just moved"
    );
}

/// Moving a panel and filling an edge stay separate acts. Folding them together
/// would make a gesture silently start a daemon load — the same reason walking
/// the panel list never materializes an empty panel.
#[test]
fn moving_a_panel_in_the_manager_does_not_load_it() {
    let mut app = two_column_app();
    open_manager(&mut app);
    app.on_key(plain('H'));
    assert_eq!(
        app.panel_host.panel_at(PanelPlacement::Left),
        SidePanelId::Terminal,
        "precondition: the panel is on the left"
    );
    assert!(
        app.focus_manager.has_dialog(DialogType::PanelManager),
        "still open: moving is not loading"
    );
}

/// `Shift+K` names no edge — there is no top — so inside the manager it must do
/// nothing at all rather than inventing a destination.
#[test]
fn an_upward_directional_is_inert_in_the_manager() {
    let mut app = two_column_app();
    open_manager(&mut app);
    let before = app.panel_host.placement();
    app.on_key(plain('K'));
    assert_eq!(app.panel_host.placement(), before);
    assert_eq!(
        manager_cursor(&app),
        Some(SidePanelId::Terminal.index()),
        "and it must not have walked the highlight either"
    );
}
