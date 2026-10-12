//! Multi-modifier chord reachability.
//!
//! The TUI never pushes keyboard-enhancement flags (`src/tui/terminal.rs`
//! enables raw mode, bracketed paste, and mouse capture only), so the terminal
//! is in **legacy xterm key encoding**. That encoding has one property this
//! file exists to prevent regressing: `Ctrl+Shift+X` and `Ctrl+X` are the same
//! byte, so crossterm reports `Char('x')` with CONTROL only. A
//! `(CONTROL|SHIFT, …)` binding is therefore unreachable, and the keystroke
//! silently falls through to whichever action owns the bare-Ctrl twin.
//!
//! That is not hypothetical. The side-panel switcher was bound to
//! `Ctrl+Shift+W`; in legacy encoding it is byte `0x17`, identical to
//! `Ctrl+W`, so pressing it ran `CloseSession`.
//!
//! The contract these tests enforce:
//!
//! 1. Every advertised multi-modifier chord resolves from the key the
//!    terminal actually delivers (`Ctrl+Alt+<lower>`).
//! 2. No `(CONTROL|SHIFT, …)` entry exists in any built-in binding map.
//! 3. Each chord stays disjoint from its own bare-Ctrl and bare-Alt twins.

use codegg::tui::input::{
    build_bindings, default_bindings, handle_event_with_bindings_moded, InputAction, InputMode,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

fn press(code: KeyCode, modifiers: KeyModifiers, mode: InputMode) -> Option<InputAction> {
    handle_event_with_bindings_moded(Event::Key(KeyEvent::new(code, modifiers)), None, mode)
}

/// Chords that must be deliverable in legacy encoding, with the action each
/// one owns. Every entry here must be `{CONTROL, ALT}` + lowercase letter.
const LEGACY_SAFE_CHORDS: &[(char, InputAction, &str)] = &[
    ('w', InputAction::TogglePanelSelector, "side-panel switcher"),
    ('s', InputAction::FocusSidebar, "sidebar focus"),
    ('p', InputAction::CycleModelBackward, "model cycle backward"),
    ('y', InputAction::StopTts, "stop TTS"),
    ('f', InputAction::ToggleFullscreen, "fullscreen"),
    ('n', InputAction::NextProjectTab, "next project tab"),
    ('b', InputAction::PreviousProjectTab, "previous project tab"),
];

/// The bare-Ctrl action each chord must not be confused with. Legacy encoding
/// delivers `Ctrl+X` for `Ctrl+Shift+X`, so any chord sharing a letter with a
/// bare-Ctrl binding is a live footgun.
const BARE_CTRL_TWINS: &[(char, InputAction, &str)] = &[
    ('w', InputAction::CloseSession, "CloseSession"),
    ('y', InputAction::ToggleTts, "ToggleTts"),
    ('f', InputAction::Search, "Search"),
    ('k', InputAction::ClearSession, "ClearSession"),
    ('n', InputAction::NewSession, "NewSession"),
    ('s', InputAction::StashPrompt, "StashPrompt"),
    ('b', InputAction::ToggleSidebar, "ToggleSidebar"),
];

#[test]
fn every_advertised_chord_resolves_in_insert_mode() {
    for (letter, expected, what) in LEGACY_SAFE_CHORDS {
        let got = press(
            KeyCode::Char(*letter),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            InputMode::Insert,
        );
        assert_eq!(
            got,
            Some(expected.clone()),
            "Ctrl+Alt+{letter} ({what}) must resolve in Insert mode"
        );
    }
}

#[test]
fn every_advertised_chord_resolves_in_normal_mode() {
    for (letter, expected, what) in LEGACY_SAFE_CHORDS {
        let got = press(
            KeyCode::Char(*letter),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            InputMode::Normal,
        );
        assert_eq!(
            got,
            Some(expected.clone()),
            "Ctrl+Alt+{letter} ({what}) must resolve in Normal mode"
        );
    }
}

#[test]
fn the_panel_switcher_is_bound_in_every_map() {
    // The switcher is the one chord this work exists for, and it must be
    // reachable regardless of which map is active.
    for vim_mode in [false, true] {
        let bindings = build_bindings(None, vim_mode);
        assert_eq!(
            bindings
                .get(&(
                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                    KeyCode::Char('w')
                ))
                .cloned(),
            Some(InputAction::TogglePanelSelector),
            "Ctrl+Alt+W missing from the {} map",
            if vim_mode { "vim" } else { "default" }
        );
    }
}

#[test]
fn every_advertised_chord_exists_in_the_default_map() {
    // Key handling above can also pass because a *different* map is consulted;
    // assert the entry itself is present in the literal that ships.
    let default_map = default_bindings();
    for (letter, expected, what) in LEGACY_SAFE_CHORDS {
        let chord = (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char(*letter),
        );
        assert_eq!(
            default_map.get(&chord).cloned(),
            Some(expected.clone()),
            "Ctrl+Alt+{letter} ({what}) must exist in the default map"
        );
    }
}

#[test]
fn chords_are_disjoint_from_their_bare_ctrl_twin() {
    // This is the regression that shipped: the chord's letter is owned by a
    // different action under bare CONTROL, so if the chord ever degrades back
    // to CONTROL|SHIFT the user triggers the bare action instead.
    for (letter, chord_action, what) in LEGACY_SAFE_CHORDS {
        let Some((_, twin_action, twin_name)) =
            BARE_CTRL_TWINS.iter().find(|(l, _, _)| l == letter)
        else {
            continue;
        };
        let bare = press(
            KeyCode::Char(*letter),
            KeyModifiers::CONTROL,
            InputMode::Insert,
        );
        assert_eq!(
            bare,
            Some(twin_action.clone()),
            "Ctrl+{letter} must still be {twin_name}"
        );
        assert_ne!(
            chord_action, twin_action,
            "Ctrl+Alt+{letter} ({what}) must not alias its bare-Ctrl twin {twin_name}"
        );
    }
}

#[test]
fn chords_are_disjoint_from_their_bare_alt_twin() {
    // `Alt+W` is CloseProjectTab, so `Ctrl+Alt+W` must still be distinct.
    let alt_w = press(KeyCode::Char('w'), KeyModifiers::ALT, InputMode::Insert);
    assert_eq!(
        alt_w,
        Some(InputAction::CloseProjectTab),
        "Alt+W must still close the project tab"
    );
    let ctrl_alt_w = press(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
        InputMode::Insert,
    );
    assert_eq!(
        ctrl_alt_w,
        Some(InputAction::TogglePanelSelector),
        "Ctrl+Alt+W must open the panel selector, not close the tab"
    );
}

#[test]
fn no_binding_map_contains_a_control_shift_entry() {
    // The root cause, asserted structurally rather than per-action: in legacy
    // encoding a `(CONTROL|SHIFT, …)` key can never arrive as written.
    for (name, bindings) in [
        ("default", default_bindings()),
        ("vim", build_bindings(None, true)),
    ] {
        for (modifiers, code) in bindings.keys() {
            assert!(
                !modifiers.contains(KeyModifiers::SHIFT)
                    || !modifiers.contains(KeyModifiers::CONTROL),
                "{name} map binds {code:?} with CONTROL|SHIFT, which legacy xterm \
                 encoding cannot deliver — use CONTROL|ALT instead"
            );
        }
    }
}

#[test]
fn panel_switcher_never_falls_through_to_close_session() {
    // The exact user-visible failure: pressing the switcher chord closed the
    // session instead. Assert the switcher wins on its own chord.
    for mode in [InputMode::Insert, InputMode::Normal] {
        assert_ne!(
            press(
                KeyCode::Char('w'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
                mode
            ),
            Some(InputAction::CloseSession),
            "the switcher chord must not resolve to CloseSession in {mode:?}"
        );
    }
}

// ── Panel focus moves ─────────────────────────────────────────────────
//
// These are the only Shift-qualified bindings in the shipped maps, and they
// carry a constraint the chords above do not: they must exist in Normal mode
// and must NOT fire in Insert mode, where the same key is ordinary text or
// caret movement. A binding that leaked across the mode boundary would make
// `Shift+Right` unreachable for typing and `H` untypable while composing.

#[test]
fn shift_arrow_focus_moves_are_bound_in_every_map() {
    for vim_mode in [false, true] {
        let map = build_bindings(None, vim_mode);
        let label = if vim_mode { "vim" } else { "default" };
        assert_eq!(
            map.get(&(KeyModifiers::SHIFT, KeyCode::Right)).cloned(),
            Some(InputAction::FocusSidePanel),
            "Shift+Right missing from the {label} map"
        );
        assert_eq!(
            map.get(&(KeyModifiers::SHIFT, KeyCode::Left)).cloned(),
            Some(InputAction::FocusMainBuffer),
            "Shift+Left missing from the {label} map"
        );
    }
}

#[test]
fn focus_moves_fire_in_normal_mode_only() {
    assert_eq!(
        press(KeyCode::Right, KeyModifiers::SHIFT, InputMode::Normal),
        Some(InputAction::FocusSidePanel),
        "Shift+Right must move focus in Normal mode"
    );
    assert_eq!(
        press(KeyCode::Left, KeyModifiers::SHIFT, InputMode::Normal),
        Some(InputAction::FocusMainBuffer),
        "Shift+Left must return focus in Normal mode"
    );
    assert_eq!(
        press(KeyCode::Right, KeyModifiers::SHIFT, InputMode::Insert),
        None,
        "Shift+Right is caret movement while composing; it must not steal the key"
    );
    assert_eq!(
        press(KeyCode::Left, KeyModifiers::SHIFT, InputMode::Insert),
        None,
        "Shift+Left must remain caret movement while composing"
    );
}

#[test]
fn shift_letters_resolve_under_both_encodings() {
    // Terminals disagree about whether a shifted letter carries SHIFT. If
    // only one form were bound, the action would work on one machine and
    // silently do nothing on another.
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        assert_eq!(
            press(KeyCode::Char('L'), modifiers, InputMode::Normal),
            Some(InputAction::DockPanel),
            "L with {modifiers:?} must dock the open box into the side panel"
        );
        assert_eq!(
            press(KeyCode::Char('H'), modifiers, InputMode::Normal),
            Some(InputAction::FocusMainBuffer),
            "H with {modifiers:?} must focus the main buffer"
        );
    }
    // Moving focus into the side column keeps its arrow binding. `L` was
    // repointed at docking, and arrows are the one chord no terminal
    // mis-encodes, so `FocusSidePanel` needs no letter twin.
    assert_eq!(
        press(KeyCode::Right, KeyModifiers::SHIFT, InputMode::Normal),
        Some(InputAction::FocusSidePanel),
        "Shift+Right must still focus the side column"
    );
}

#[test]
fn uppercase_focus_letters_are_ordinary_text_in_insert_mode() {
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        assert_eq!(
            press(KeyCode::Char('L'), modifiers, InputMode::Insert),
            Some(InputAction::Char('L')),
            "an uppercase L while composing must type an L, not dock a box"
        );
        assert_eq!(
            press(KeyCode::Char('H'), modifiers, InputMode::Insert),
            Some(InputAction::Char('H')),
            "an uppercase H while composing must type an H, not move focus"
        );
    }
}

#[test]
fn bare_arrows_keep_their_unmodified_meaning() {
    // The user's contract was that focus switching must not disturb the
    // existing arrow behaviour, so an unmodified arrow may never move focus.
    for code in [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right] {
        let action = press(code, KeyModifiers::NONE, InputMode::Normal);
        assert!(
            !matches!(
                action,
                Some(InputAction::FocusSidePanel) | Some(InputAction::FocusMainBuffer)
            ),
            "an unmodified {code:?} must not have become a focus move, got {action:?}"
        );
    }
}

#[test]
fn the_vertical_focus_pair_is_deliberately_unbound() {
    // Shift+Up/Down and Shift+J/K are reserved for a layout with stacked
    // panels. Binding them to a no-op today would advertise a capability
    // the UI does not have, so they must stay unclaimed in Normal mode.
    for code in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Char('J'),
        KeyCode::Char('K'),
    ] {
        let action = press(code, KeyModifiers::SHIFT, InputMode::Normal);
        assert!(
            !matches!(
                action,
                Some(InputAction::FocusSidePanel) | Some(InputAction::FocusMainBuffer)
            ),
            "Shift+{code:?} is reserved and must not act as a focus move, got {action:?}"
        );
    }
}
