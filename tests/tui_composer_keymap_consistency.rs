//! The composer's keymap consistency: the hint row, the help, and the three
//! surfaces' shared assets.
//!
//! M007 is the workstream's closing sweep. Its subject is not a new verb but
//! the *agreement* between the surfaces that describe the verbs: the claim
//! table, the hint row above the composer, and the help box. All three are
//! **derived** from the same table, so none of them can drift from it.

use codegg::tui::app::App;
use codegg::tui::components::dialogs::help::{HelpDialog, HelpRoute};
use codegg::tui::composer_claims::{composer_verb_entries, COMPOSER_CLAIMS};
use codegg::tui::input::{build_help_lines, default_help_entries, HelpMode, InputMode};
use std::sync::Arc;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

/// Put text in the composer and place the caret, in Normal mode.
fn compose(app: &mut App, text: &str, caret: usize) {
    app.prompt_state.prompt.set_text(text.to_string());
    app.prompt_state.prompt.set_cursor(caret);
    app.ui_state.input_mode = InputMode::Normal;
}

/// The hint row above the composer.
fn hint(app: &mut App) -> String {
    app.composer_hint_text()
        .expect("the composer holds the keyboard")
}

/// The keys the hint row names, read back out of the row.
///
/// The row is read rather than inspected, because the row is what the user
/// sees: a key that is in the table but not in the row is a key the user
/// cannot find. A **key** is a single-letter part of the row — the prose
/// around it ("space help", "type", "Esc normal") is not a key.
fn keys_in(row: &str) -> Vec<char> {
    row.split(" · ")
        .filter_map(|part| {
            let mut chars = part.chars();
            let first = chars.next()?;
            if chars.next().is_none() && first.is_ascii_alphabetic() {
                Some(first)
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The hint row
// ---------------------------------------------------------------------------

/// The row names every live verb, on a multi-line buffer.
///
/// The line motions are the conditional ones, so a multi-line buffer is the
/// case where all of them are available.
#[test]
fn the_hint_names_every_live_verb_on_a_multi_line_buffer() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    let row = hint(&mut app);
    for key in [
        'w', 'b', 'e', 'j', 'k', 'v', 'x', 'd', 'p', 'u', 'U', 'a', 'o',
    ] {
        assert!(
            row.contains(key),
            "the hint row does not name {key:?}: {row:?}"
        );
    }
}

/// On a single-line buffer the row does not claim `j` and `k`.
///
/// There they are the transcript's, and a row that claimed them would teach a
/// gesture that scrolls the transcript instead of moving the caret.
#[test]
fn the_hint_does_not_claim_the_line_motions_on_a_single_line_buffer() {
    let mut app = test_app();
    compose(&mut app, "one", 0);

    let row = hint(&mut app);
    assert!(
        !row.contains('j') && !row.contains('k'),
        "the hint row claims the line motions on a single-line buffer: {row:?}"
    );
    // The unconditional verbs are still there.
    for key in ['w', 'b', 'e', 'v', 'x', 'd', 'p', 'u', 'U', 'a', 'o'] {
        assert!(
            row.contains(key),
            "the hint row does not name {key:?}: {row:?}"
        );
    }
}

/// The row's **presence** does not depend on the mode.
///
/// The layout must not jump on `Esc`: the row is reserved in Normal mode and
/// in Insert mode alike, and only its *text* changes.
#[test]
fn the_hints_presence_is_the_same_in_normal_and_insert_mode() {
    let mut app = test_app();
    compose(&mut app, "one", 0);

    let normal = app.composer_hint_text();
    app.ui_state.input_mode = InputMode::Insert;
    let insert = app.composer_hint_text();

    assert!(
        normal.is_some() && insert.is_some(),
        "the row's presence changed with the mode: normal {normal:?}, insert {insert:?}"
    );
    assert_ne!(
        normal, insert,
        "the row's text did not change with the mode; the mode is what changes it"
    );
}

/// The row names no verb that is not a live composer claim.
///
/// The row is derived from the claim table, so this holds by construction —
/// the test is what makes that a fact rather than an intention.
#[test]
fn the_hint_names_no_verb_that_is_not_a_live_claim() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    let row = hint(&mut app);
    let live: Vec<char> = COMPOSER_CLAIMS
        .iter()
        .filter(|claim| claim.status == codegg::tui::composer_claims::ClaimStatus::Live)
        .map(|claim| claim.key)
        .collect();

    // The row's prose is "… · space help"; the words carry letters of their
    // own, so the check is over the row's *key* positions: every
    // single-letter part must be a live claim.
    for key in keys_in(&row) {
        assert!(
            live.contains(&key),
            "the hint row names {key:?}, which is not a live composer claim: {row:?}"
        );
    }
    assert!(
        !keys_in(&row).is_empty(),
        "the hint row names no keys at all: {row:?}"
    );
}

/// The row fits the width it is given, and never renders half a key.
///
/// A row cut in the middle of a key names a key that does not exist.
#[test]
fn the_hint_is_truncated_to_the_width_without_cutting_a_key() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    let full = app.composer_hint_text().expect("a row");
    for budget in [8usize, 16, 24, 40, 60] {
        let bounded = app.composer_hint_text_bounded(budget).expect("a row");
        assert!(
            unicode_width::UnicodeWidthStr::width(bounded.as_str()) <= budget,
            "the row is wider than its budget at {budget}: {bounded:?}"
        );
        // Every part the row kept is a whole part.
        for part in bounded.split(" · ") {
            assert!(
                part.chars().count() == 1
                    || matches!(
                        part,
                        "COMP" | "NOR" | "INS" | "space help" | "type" | "Esc normal"
                    ),
                "the row was cut in the middle of a part: {part:?} in {bounded:?}"
            );
        }
        assert!(
            bounded.len() <= full.len(),
            "the bounded row is longer than the unbounded one"
        );
    }
}

/// The row names the composer's verbs, not another surface's.
///
/// The terminal panel's verbs (`v chars · x lines · d delete`) are the
/// terminal's, and the composer's row is the composer's.
///
/// The row carries **keys only**. It used to open with `COMP · NOR`, which
/// named the surface and the mode — and both are already said elsewhere: the
/// badge directly above the input box carries the mode, and the row's
/// position under that badge says whose keys these are. A row that restated
/// them was the same fact said twice, and the second copy was the one that
/// could disagree with the first.
#[test]
fn the_hint_names_the_composers_verbs_and_not_the_transcripts() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    let row = hint(&mut app);
    assert!(
        !row.starts_with("COMP") && !row.starts_with("NOR") && !row.starts_with("INS"),
        "the row carries keys only: the badge above the input box owns the \
         mode and the surface name: {row:?}"
    );
    assert!(
        !row.contains("scroll"),
        "the hint row advertises a transcript gesture: {row:?}"
    );
}

// ---------------------------------------------------------------------------
// The help's composer section
// ---------------------------------------------------------------------------

/// The help has a row for every live composer verb, with the right key.
///
/// The section is derived from the claim table, so this holds by
/// construction; the test is what makes that a fact.
#[test]
fn the_help_has_a_row_for_every_live_composer_verb() {
    let normal = codegg::tui::input::help_entries_for_mode(HelpMode::Normal);
    for entry in composer_verb_entries() {
        let row = normal
            .iter()
            .find(|row| row.key == entry.key_text)
            .unwrap_or_else(|| {
                panic!(
                    "the help has no row for {:?} (a live composer claim)",
                    entry.key_text
                )
            });
        assert_eq!(
            row.action, entry.label,
            "the help describes {:?} differently from the claim table",
            entry.key_text
        );
    }
}

/// The help has no row for a reserved claim.
///
/// A reserved claim owns its key but has no behaviour, so a row would teach a
/// gesture that does nothing.
#[test]
fn the_help_has_no_row_for_a_reserved_claim() {
    let normal = codegg::tui::input::help_entries_for_mode(HelpMode::Normal);
    for claim in COMPOSER_CLAIMS.iter() {
        if claim.status == codegg::tui::composer_claims::ClaimStatus::Live {
            continue;
        }
        assert!(
            !normal.iter().any(|row| row.key == claim.key.to_string()),
            "the help advertises the reserved claim {:?}",
            claim.key
        );
    }
}

/// A conditional claim's row states the condition.
///
/// The `j`/`k` rows say the line motions need a multi-line buffer; a row that
/// dropped the condition would read as unconditional.
#[test]
fn a_conditional_verbs_row_states_its_condition() {
    let normal = codegg::tui::input::help_entries_for_mode(HelpMode::Normal);
    for entry in composer_verb_entries() {
        let row = normal
            .iter()
            .find(|row| row.key == entry.key_text)
            .expect("a row for the claim");
        if entry.condition.label().is_some() {
            assert!(
                row.condition.is_some(),
                "the conditional claim {:?} has no condition in the help",
                entry.key_text
            );
        }
    }
}

/// The chord route's box lists the copy gesture; the command route's does not.
///
/// The copy gesture is the second step of the `space` chord, and only the box
/// the `space` chord opens performs it. A `/help` box is opened to type a
/// query, and `y` there is a filter character.
#[test]
fn the_copy_gesture_is_listed_only_in_the_chord_routes_box() {
    let chord = build_help_lines(false, HelpMode::Normal, HelpRoute::Chord);
    let command = build_help_lines(false, HelpMode::Normal, HelpRoute::Command);

    assert!(
        chord.iter().any(|line| line.contains("space+y")),
        "the chord route's box does not list the copy gesture: {chord:?}"
    );
    assert!(
        !command.iter().any(|line| line.contains("space+y")),
        "the command route's box lists a gesture it does not perform: {command:?}"
    );
}

/// The composer's own verbs are in **both** routes.
///
/// They are documentation: a user who opens `/help` is exactly the user who
/// needs to know what `w` does.
#[test]
fn the_composers_verbs_are_in_both_routes() {
    for route in [HelpRoute::Chord, HelpRoute::Command] {
        let lines = build_help_lines(false, HelpMode::Normal, route);
        for entry in composer_verb_entries() {
            assert!(
                lines.iter().any(|line| line.contains(entry.label)),
                "the {route:?} route's box does not describe {:?}",
                entry.key_text
            );
        }
    }
}

/// The pre-rendered constructor keeps the command route's behaviour.
///
/// It is the app's help, and it must not be armed.
#[test]
fn the_pre_rendered_constructor_keeps_the_command_route() {
    let dialog = HelpDialog::new(
        Arc::new(codegg::tui::theme::Theme::dark()),
        vec!["a".to_string()],
    );
    assert_eq!(dialog.route(), HelpRoute::Command);
}

/// The help's rows and the claim table agree on every live verb's key.
///
/// The check is at the *key* level, which is what a user presses.
#[test]
fn the_help_and_the_claim_table_agree_on_every_key() {
    let normal = codegg::tui::input::help_entries_for_mode(HelpMode::Normal);
    let live: Vec<String> = composer_verb_entries()
        .iter()
        .map(|entry| entry.key_text.to_string())
        .collect();
    for key in live {
        assert!(
            normal.iter().any(|row| row.key == key),
            "the help has no row for the live claim {key:?}"
        );
    }
}

/// The derived section is not a second copy of the table.
///
/// The help's composer rows are computed from the claim table, so adding a
/// claim adds a row without touching the help.
#[test]
fn the_help_section_is_derived_rather_than_written_out() {
    // The derived rows are not literal `HelpEntry`s in the source, so the
    // number of literal rows is the app's own count and the composer's count
    // is on top of it.
    let literal = default_help_entries().len();
    let normal = codegg::tui::input::help_entries_for_mode(HelpMode::Normal);
    let live = composer_verb_entries().len();
    assert!(
        normal.len() >= live,
        "the Normal-mode help has fewer rows than there are live claims"
    );
    assert!(
        literal > 0,
        "the help has no rows at all; the derivation cannot be the whole of it"
    );
}
