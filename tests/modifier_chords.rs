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
