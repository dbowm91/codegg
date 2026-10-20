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
    ('s', InputAction::FocusSidebar, "sidebar focus"),
    ('p', InputAction::CycleModelBackward, "model cycle backward"),
    ('y', InputAction::StopTts, "stop TTS"),
    ('f', InputAction::ToggleFullscreen, "fullscreen"),
    ('n', InputAction::NextProjectTab, "next project tab"),
    ('b', InputAction::PreviousProjectTab, "previous project tab"),
    // ADR-0042: the composer claimed the letters `x`, `d` and `i`, so the
    // actions that owned them moved to chords that are not letters. Each one
    // is in **both** maps, so the capability does not depend on which map is
    // active.
    ('x', InputAction::ToggleAgentRun, "stop / resume the turn"),
    ('d', InputAction::OpenDiff, "open diff"),
    ('e', InputAction::FocusPrompt, "focus prompt"),
];

/// The bare-Ctrl action each chord must not be confused with. Legacy encoding
/// delivers `Ctrl+X` for `Ctrl+Shift+X`, so any chord sharing a letter with a
/// bare-Ctrl binding is a live footgun.
const BARE_CTRL_TWINS: &[(char, InputAction, &str)] = &[
    ('w', InputAction::CloseSession, "CloseSession"),
    ('y', InputAction::ToggleTts, "ToggleTts"),
    ('f', InputAction::Search, "Search"),
    ('k', InputAction::ClearSession, "ClearSession"),
    ('s', InputAction::StashPrompt, "StashPrompt"),
    ('b', InputAction::ToggleSidebar, "ToggleSidebar"),
    ('d', InputAction::PageDown, "PageDown"),
    ('e', InputAction::ExternalEditor, "ExternalEditor"),
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
    // `Ctrl+Alt+W` was retired in favour of `Shift+S`, which is a
    // Normal-mode letter rather than a chord. No map may still bind it, or
    // a user with the old habit gets a key that silently does nothing —
    // and the letter `w` is next to `Ctrl+Alt+W`'s old neighbour, so a
    // stray fallback to `CloseSession` would be actively dangerous.
    for vim_mode in [false, true] {
        let bindings = build_bindings(None, vim_mode);
        assert!(
            !bindings.contains_key(&(
                KeyModifiers::CONTROL | KeyModifiers::ALT,
                KeyCode::Char('w')
            )),
            "the retired Ctrl+Alt+W chord is still bound in the {} map",
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
fn ctrl_n_remains_unbound_to_new_session() {
    // Ctrl+N arms the panel-resize frame gesture in the app layer. It is
    // intentionally absent from the input-action map; NewSession is bare `n`.
    let ctrl_n = (KeyModifiers::CONTROL, KeyCode::Char('n'));
    assert_eq!(default_bindings().get(&ctrl_n), None);
    assert_eq!(press(ctrl_n.1, ctrl_n.0, InputMode::Insert), None);
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
    assert!(
        !matches!(
            ctrl_alt_w,
            Some(InputAction::CloseProjectTab) | Some(InputAction::CloseSession)
        ),
        "the retired Ctrl+Alt+W chord must not resolve to a destructive action"
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

// ── Pane directions ───────────────────────────────────────────────────
//
// `Shift+←/→/↑/↓` and the vim `Shift+H/J/K/L` twins are one gesture family.
// Every one of them resolves to a `Direction*` action and is then handed to a
// single contextual dispatcher, which docks an open box or selects a pane
// depending on what is on screen.
//
// These are the only Shift-qualified bindings in the shipped maps, and they
// carry a constraint the chords above do not: they must exist in Normal mode
// and must NOT fire in Insert mode, where the same key is ordinary text or
// caret movement. A binding that leaked across the mode boundary would make
// `Shift+Right` unreachable for typing and `H` untypable while composing.

#[test]
fn directional_arrows_are_bound_in_every_map() {
    for vim_mode in [false, true] {
        let map = build_bindings(None, vim_mode);
        let label = if vim_mode { "vim" } else { "default" };
        for (code, expected) in [
            (KeyCode::Left, InputAction::DirectionLeft),
            (KeyCode::Right, InputAction::DirectionRight),
            (KeyCode::Up, InputAction::DirectionUp),
            (KeyCode::Down, InputAction::DirectionDown),
        ] {
            assert_eq!(
                map.get(&(KeyModifiers::SHIFT, code)).cloned(),
                Some(expected),
                "Shift+{code:?} missing from the {label} map"
            );
        }
    }
}

#[test]
fn directions_fire_in_normal_mode_only() {
    for (code, expected) in [
        (KeyCode::Left, InputAction::DirectionLeft),
        (KeyCode::Right, InputAction::DirectionRight),
        (KeyCode::Up, InputAction::DirectionUp),
        (KeyCode::Down, InputAction::DirectionDown),
    ] {
        assert_eq!(
            press(code, KeyModifiers::SHIFT, InputMode::Normal),
            Some(expected),
            "Shift+{code:?} must be a pane direction in Normal mode"
        );
        assert_eq!(
            press(code, KeyModifiers::SHIFT, InputMode::Insert),
            None,
            "Shift+{code:?} is caret movement while composing; it must not steal the key"
        );
    }
}

#[test]
fn shift_direction_letters_resolve_under_both_encodings() {
    // Terminals disagree about whether a shifted letter carries SHIFT. If
    // only one form were bound, the gesture would work on one machine and
    // silently do nothing on another.
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        for (letter, expected) in [
            ('H', InputAction::DirectionLeft),
            ('J', InputAction::DirectionDown),
            ('K', InputAction::DirectionUp),
            ('L', InputAction::DirectionRight),
        ] {
            assert_eq!(
                press(KeyCode::Char(letter), modifiers, InputMode::Normal),
                Some(expected),
                "{letter} with {modifiers:?} must resolve as a pane direction"
            );
        }
    }
}

#[test]
fn uppercase_direction_letters_are_ordinary_text_in_insert_mode() {
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        for letter in ['H', 'J', 'K', 'L'] {
            assert_eq!(
                press(KeyCode::Char(letter), modifiers, InputMode::Insert),
                Some(InputAction::Char(letter)),
                "an uppercase {letter} while composing must type a {letter}, not move a pane"
            );
        }
    }
}

#[test]
fn bare_arrows_keep_their_unmodified_meaning() {
    // The user's contract was that pane switching must not disturb the
    // existing arrow behaviour, so an unmodified arrow may never become a
    // direction. These stay transcript scrolling and caret movement.
    for code in [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right] {
        let action = press(code, KeyModifiers::NONE, InputMode::Normal);
        assert!(
            !is_direction(&action),
            "an unmodified {code:?} must not have become a pane direction, got {action:?}"
        );
    }
}

#[test]
fn the_vertical_pair_is_now_bound_rather_than_reserved() {
    // This pair used to be deliberately left unbound, reserved for a layout
    // with stacked panels, on the grounds that binding an inert key would
    // advertise a capability the UI did not have. They are bound now because
    // they are no longer inert: `↑`/`↓` cycle which panel type is loaded.
    assert_eq!(
        press(KeyCode::Up, KeyModifiers::SHIFT, InputMode::Normal),
        Some(InputAction::DirectionUp),
        "Shift+Up must cycle the loaded panel list"
    );
    assert_eq!(
        press(KeyCode::Down, KeyModifiers::SHIFT, InputMode::Normal),
        Some(InputAction::DirectionDown),
        "Shift+Down must cycle the loaded panel list"
    );
    assert_eq!(
        press(KeyCode::Char('K'), KeyModifiers::SHIFT, InputMode::Normal),
        Some(InputAction::DirectionUp)
    );
    assert_eq!(
        press(KeyCode::Char('J'), KeyModifiers::NONE, InputMode::Normal),
        Some(InputAction::DirectionDown)
    );
}

#[test]
fn bare_lowercase_letters_are_not_the_directional_gesture() {
    // A terminal reports `Shift+L` as `Char('L')`, with or without SHIFT. It
    // never reports it as `Char('l')`, so binding lowercase would claim the
    // plain within-surface vim verbs for a gesture the user did not make.
    for letter in ['h', 'j', 'k', 'l'] {
        let action = press(KeyCode::Char(letter), KeyModifiers::NONE, InputMode::Normal);
        assert!(
            !is_direction(&action),
            "a bare `{letter}` must stay a within-surface verb, got {action:?}"
        );
    }
}

fn is_direction(action: &Option<InputAction>) -> bool {
    matches!(
        action,
        &Some(InputAction::DirectionLeft)
            | &Some(InputAction::DirectionRight)
            | &Some(InputAction::DirectionUp)
            | &Some(InputAction::DirectionDown)
    )
}

// ── Subagent tabs ─────────────────────────────────────────────────────
//
// `Shift+N` / `Shift+P` / `Shift+X` are the second family of Shift-qualified
// letters after the pane directions, and they carry the same two constraints:
// they must be bound under **both** legacy encodings, and they must stay
// ordinary text while composing. The second is not a nicety here — `N` and `X`
// are among the most common capitals in English prose, and `X` in particular is
// what a user types when naming a variable.

#[test]
fn subagent_tab_letters_resolve_under_both_encodings() {
    // Terminals disagree about whether a shifted letter carries SHIFT. Binding
    // only one form would make the gesture work on one machine and silently do
    // nothing on another.
    for vim_mode in [false, true] {
        let map = build_bindings(None, vim_mode);
        let label = if vim_mode { "vim" } else { "default" };
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
            for (letter, expected) in [
                ('N', InputAction::NextSubagentTab),
                ('P', InputAction::PreviousSubagentTab),
                ('X', InputAction::CloseSubagentTab),
            ] {
                assert_eq!(
                    map.get(&(modifiers, KeyCode::Char(letter))).cloned(),
                    Some(expected.clone()),
                    "{letter} with {modifiers:?} missing from the {label} map"
                );
                assert_eq!(
                    press(KeyCode::Char(letter), modifiers, InputMode::Normal),
                    Some(expected),
                    "{letter} with {modifiers:?} must cycle tabs in Normal mode"
                );
            }
        }
    }
}

#[test]
fn subagent_tab_letters_are_ordinary_text_in_insert_mode() {
    // If any of these fired while composing it would stop the agent mid-word.
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        for letter in ['N', 'P', 'X'] {
            assert_eq!(
                press(KeyCode::Char(letter), modifiers, InputMode::Insert),
                Some(InputAction::Char(letter)),
                "an uppercase {letter} while composing must type a {letter}, not drive a tab"
            );
        }
    }
}

#[test]
fn subagent_tab_letters_are_distinct_from_the_project_tab_chords() {
    // Project tabs (header) and subagent tabs (main buffer) are independent
    // axes. Reusing `Ctrl+Alt+N`/`Ctrl+Alt+B` for both would have collapsed
    // two navigations onto one gesture, so the letters must stay disjoint.
    for (letter, chord_action) in [
        ('n', InputAction::NextProjectTab),
        ('b', InputAction::PreviousProjectTab),
    ] {
        let chord = press(
            KeyCode::Char(letter),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            InputMode::Normal,
        );
        assert_eq!(
            chord,
            Some(chord_action),
            "Ctrl+Alt+{letter} must keep driving project tabs"
        );
        assert_ne!(chord, Some(InputAction::NextSubagentTab));
        assert_ne!(chord, Some(InputAction::PreviousSubagentTab));
    }

    // And the Shift spellings must not have shadowed an existing bare letter.
    for letter in ['n', 'p', 'x'] {
        assert_ne!(
            press(KeyCode::Char(letter), KeyModifiers::NONE, InputMode::Normal),
            Some(InputAction::NextSubagentTab),
            "lowercase {letter} must not open the tasks tab family"
        );
    }
}

#[test]
fn no_subagent_tab_binding_is_reachable_as_a_control_shift_chord() {
    // Covered structurally by `no_binding_map_contains_a_control_shift_entry`,
    // but asserted here by name because this family is the one most likely to
    // grow a chord later.
    for bindings in [default_bindings(), build_bindings(None, true)] {
        for (modifiers, code) in bindings.keys() {
            let targets_a_tab = matches!(
                bindings.get(&(*modifiers, *code)),
                Some(InputAction::NextSubagentTab)
                    | Some(InputAction::PreviousSubagentTab)
                    | Some(InputAction::CloseSubagentTab)
            );
            if targets_a_tab {
                assert!(
                    !(modifiers.contains(KeyModifiers::SHIFT)
                        && modifiers.contains(KeyModifiers::CONTROL)),
                    "{code:?} drives a subagent tab with CONTROL|SHIFT, which legacy \
                     xterm encoding cannot deliver"
                );
            }
        }
    }
}
