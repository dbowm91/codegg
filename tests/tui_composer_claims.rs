//! The composer's Normal-mode claim table.
//!
//! ADR-0042 gives the main composer a closed set of Normal-mode letters, and
//! the app's global binding map is consulted only for letters the composer
//! does **not** claim. The rule that makes the table worth having is the
//! fallthrough rule:
//!
//! > A claimed letter is consumed by the composer even when the composer's
//! > verb is currently a no-op. It must not silently become an unrelated app
//! > action.
//!
//! Every verb in the table is reserved today, so every claim is currently a
//! no-op that must still swallow its key — with one exception. `i` is the
//! app's only route back to Insert mode, so its verb is **live**: it is the
//! transition the app already had, moved into the table so the table is the
//! one place that says who owns the key. "Nothing happened" is therefore not
//! always a usable assertion on its own: a key that was simply ignored looks
//! exactly like a key that was claimed. These tests use a **bound key** as
//! the oracle — the test binds an app-level action to the claimed letter, so
//! the action firing is visible. A claimed letter keeps the action from
//! firing; an unclaimed letter does not.
//!
//! The two halves of that oracle matter:
//!
//! * the claim is consulted **before** the binding map, so a claimed letter
//!   beats a binding for the same key;
//! * the claim is live only in the main buffer's Normal mode, so the same
//!   key reaches the map in Insert mode.

use codegg::tui::app::state::WorkspaceDashboardState;
use codegg::tui::app::{App, Dialog, SessionStatus, TuiCommand};
use codegg::tui::components::component::DialogType;
use codegg::tui::components::dialogs::help::{HelpDialog, HelpRoute};
use codegg::tui::composer_claims::{self, COMPOSER_CLAIMS};
use codegg::tui::input::{build_bindings, InputAction, InputMode};
use codegg::tui::route::Route;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeSet;
use tokio::sync::mpsc::error::TryRecvError;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent::new(code, modifiers));
}

fn normal_mode(app: &mut App) {
    app.ui_state.input_mode = InputMode::Normal;
}

/// Bind an app-level action to a key, so the test can see whether the key
/// reached the app or was consumed by the composer.
fn bind(app: &mut App, modifiers: KeyModifiers, code: KeyCode, action: InputAction) {
    app.ui_state.bindings.insert((modifiers, code), action);
}

fn help_is_open(app: &App) -> bool {
    matches!(app.ui_state.dialog, Dialog::Help(_))
}

/// The composer's text.
fn prompt_text(app: &App) -> String {
    app.prompt_state.prompt.get_text()
}

// ---------------------------------------------------------------------------
// The table itself
// ---------------------------------------------------------------------------

/// The table is the guard's input, so it must be a table and not a pile of
/// match arms. It is also the composer's whole key surface, so an empty one
/// would mean the milestone policed nothing.
#[test]
fn the_claim_table_is_not_empty_and_has_no_duplicate_letters() {
    assert!(
        !COMPOSER_CLAIMS.is_empty(),
        "the claim table is empty; the milestone would police nothing"
    );
    let mut seen = BTreeSet::new();
    for claim in COMPOSER_CLAIMS {
        assert!(
            seen.insert(claim.key),
            "the letter {:?} is claimed twice",
            claim.key
        );
        assert!(
            claim.key.is_ascii_alphabetic(),
            "the claim {:?} is not a letter",
            claim.key
        );
        assert_eq!(
            claim.verb.helix_key(),
            claim.key,
            "the claim for {:?} names a verb whose Helix key is {:?}",
            claim.key,
            claim.verb.helix_key()
        );
    }
}

/// The composer claims neither `space` nor `y`.
///
/// `space` keeps opening the help menu (ADR-0042's amendment), and the
/// Helix copy gesture is the second step of that chord — a verb of the help
/// box, not a composer letter. One gesture, one owner.
#[test]
fn the_composer_claims_neither_space_nor_y() {
    for key in [' ', 'y'] {
        assert!(
            !COMPOSER_CLAIMS.iter().any(|claim| claim.key == key),
            "the composer must not claim {key:?}"
        );
    }
}

/// `g` keeps `GoToTop`, and `h`/`l` keep moving the composer caret in **every**
/// map, so none of the three is claimed.
#[test]
fn the_composer_claims_neither_g_h_nor_l() {
    for key in ['g', 'h', 'l'] {
        assert!(
            !COMPOSER_CLAIMS.iter().any(|claim| claim.key == key),
            "the composer must not claim {key:?}"
        );
    }
}

/// No claimed letter can reach an app-level action.
///
/// This is the fallthrough rule stated over the *built-in* maps, which is
/// what `scripts/check_tui_composer_key_authority.py` checks statically.
/// The one exception is ADR-0042's conditional pair: `j`/`k` are claimed only
/// when the composer buffer has more than one line, and fall through to
/// transcript scroll otherwise.
#[test]
fn no_claimed_letter_can_reach_an_app_level_action() {
    for vim_mode in [false, true] {
        let map = build_bindings(None, vim_mode);
        for claim in COMPOSER_CLAIMS {
            for modifiers in claim.claimed_keys_modifiers() {
                let key = (modifiers, KeyCode::Char(claim.key));
                if let Some(action) = map.get(&key) {
                    assert!(
                        claim.is_conditional(),
                        "the claimed letter {:?} still reaches {action:?} in the {} map",
                        claim.key,
                        if vim_mode { "vim" } else { "default" }
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The claim beats the binding map
// ---------------------------------------------------------------------------

/// A claimed letter is consumed by the composer, so a binding for the same
/// key does not fire.
#[test]
fn a_claimed_letter_is_consumed_before_the_binding_map() {
    let mut app = test_app();
    bind(
        &mut app,
        KeyModifiers::NONE,
        KeyCode::Char('d'),
        InputAction::Help,
    );
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    assert!(
        !help_is_open(&app),
        "the claimed letter `d` reached the app's binding map"
    );
}

/// The control for the test above: an **unclaimed** letter still reaches the
/// binding map, so the assertion is not vacuous.
#[test]
fn an_unclaimed_letter_still_reaches_the_binding_map() {
    let mut app = test_app();
    bind(
        &mut app,
        KeyModifiers::NONE,
        KeyCode::Char('q'),
        InputAction::Help,
    );
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);

    assert!(
        help_is_open(&app),
        "an unclaimed letter must still reach the app's binding map"
    );
}

/// The claim is live in Normal mode only. In Insert mode the same key
/// reaches the map, which is what keeps the composer's text an ordinary
/// input surface.
#[test]
fn the_claim_is_not_live_in_insert_mode() {
    let mut app = test_app();
    bind(
        &mut app,
        KeyModifiers::SHIFT,
        KeyCode::Char('U'),
        InputAction::Help,
    );
    app.ui_state.input_mode = InputMode::Insert;

    press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT);

    assert!(
        help_is_open(&app),
        "in Insert mode the composer must not claim the key"
    );
}

/// A claimed capital letter answers to both encodings.
///
/// The claim is on `U`, and terminals disagree about whether a shifted key
/// carries the `SHIFT` modifier, so the claim must match `Char('U')` with
/// `SHIFT` **and** `Char('U')` with `NONE`. The test binds the shifted
/// spelling to an app action: in Normal mode the claim swallows it in both
/// encodings, and in Insert mode it reaches the map.
#[test]
fn a_claimed_capital_letter_is_reachable_in_both_encodings() {
    let mut app = test_app();
    bind(
        &mut app,
        KeyModifiers::SHIFT,
        KeyCode::Char('U'),
        InputAction::Help,
    );
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('U'), KeyModifiers::NONE);
    assert!(
        !help_is_open(&app),
        "the unshifted encoding of a claimed capital must be claimed"
    );

    press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT);
    assert!(
        !help_is_open(&app),
        "the shifted encoding of a claimed capital must be claimed"
    );

    // The control: in Insert mode the shifted spelling reaches the map, so
    // the two assertions above are not vacuous.
    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT);
    assert!(
        help_is_open(&app),
        "the shifted encoding must reach the map when the claim is not live"
    );
}

/// A claimed lowercase letter is claimed in its bare encoding only: the
/// shifted spelling of a lowercase letter is a *different* character, and it
/// is not the composer's.
#[test]
fn a_claimed_lowercase_letter_is_not_claimed_in_its_shifted_spelling() {
    let mut app = test_app();
    bind(
        &mut app,
        KeyModifiers::SHIFT,
        KeyCode::Char('D'),
        InputAction::Help,
    );
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('D'), KeyModifiers::SHIFT);

    assert!(
        help_is_open(&app),
        "`D` is not the composer's letter; only `d` is"
    );
}

// ---------------------------------------------------------------------------
// The re-spellings
// ---------------------------------------------------------------------------

/// `d` no longer opens the diff from the main buffer.
///
/// ADR-0042's negative test, and the one the terminal panel's dispatch
/// comment records: an unclaimed `d` would silently become `OpenDiff`.
#[test]
fn d_does_not_open_the_diff_in_the_main_buffer() {
    let mut app = test_app();
    app.ensure_tui_cmd_channel();
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    let queued = app
        .tui_cmd_rx
        .as_mut()
        .expect("the command channel is installed")
        .try_recv();
    assert!(
        matches!(queued, Err(TryRecvError::Empty)),
        "the claimed letter `d` reached the app: {queued:?}"
    );
}

/// `x` no longer stops the turn from the main buffer.
#[test]
fn x_does_not_stop_the_turn_in_the_main_buffer() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);

    assert!(
        app.prompt_state.stop_armed_at.is_none(),
        "the claimed letter `x` reached the app's stop/resume action"
    );
}

/// `space` still opens help — the ADR's amendment is the whole reason the
/// claim set has no `space` and no `y`.
///
/// This is the app-level assertion, not the map-level one: the claim step
/// sits in front of the binding map, so "the map still binds space" is not
/// by itself proof that "space still opens help".
#[test]
fn space_still_opens_help_in_the_main_buffer() {
    for vim_mode in [false, true] {
        let label = if vim_mode { "vim" } else { "default" };
        let mut app = test_app();
        app.ui_state.bindings = build_bindings(None, vim_mode);
        normal_mode(&mut app);

        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);

        assert!(
            help_is_open(&app),
            "space must still open help in the {label} map"
        );
    }
}

/// `i` no longer focuses the prompt from the main buffer.
#[test]
fn i_does_not_focus_the_prompt_in_the_main_buffer() {
    let mut app = test_app();
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);

    assert!(
        !app.ui_state.command_mode,
        "the claimed letter `i` reached the app's focus-prompt action"
    );
    assert_eq!(
        prompt_text(&app),
        "",
        "the claimed letter `i` reached the composer"
    );
}

/// `i` is the composer's, and its verb is **live**: the transition to Insert
/// mode.
///
/// The verb is the app's *existing* gesture — the one that used to live in
/// the `None` arm of the key chain — moved into the claim table. It has to
/// be live rather than reserved: `i` is the only route back to Insert mode,
/// so a reserved `i` would swallow the key and leave the user in Normal mode
/// with no way to type at all.
#[test]
fn i_runs_the_insert_transition_in_the_main_buffer() {
    for vim_mode in [false, true] {
        let label = if vim_mode { "vim" } else { "default" };
        let mut app = test_app();
        app.ui_state.bindings = build_bindings(None, vim_mode);
        normal_mode(&mut app);

        press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);

        assert_eq!(
            app.ui_state.input_mode,
            InputMode::Insert,
            "`i` must enter Insert mode in the {label} map"
        );
        assert_eq!(
            prompt_text(&app),
            "",
            "the transition must not type anything"
        );
    }
}

/// The claim is consulted before the binding map, so the live `i` verb beats
/// a binding for the same key.
///
/// This is the same precedence the reserved claims have — the claim table
/// owns the letter — but it is observable here because the verb has an
/// effect: the custom binding must not fire.
#[test]
fn the_live_i_verb_beats_a_binding_for_the_same_key() {
    let mut app = test_app();
    bind(
        &mut app,
        KeyModifiers::NONE,
        KeyCode::Char('i'),
        InputAction::Help,
    );
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);

    assert!(
        !help_is_open(&app),
        "the claimed letter `i` reached the app's binding map"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "the claimed letter `i` must still run its verb"
    );
}

/// The re-spelled chords still reach their actions, in **both** maps.
///
/// The capability is not removed by the re-spelling; it moves.
#[test]
fn the_re_spelled_chords_reach_their_actions_in_both_maps() {
    for vim_mode in [false, true] {
        let label = if vim_mode { "vim" } else { "default" };

        // Stop / resume the turn.
        let mut app = test_app();
        app.ui_state.bindings = build_bindings(None, vim_mode);
        app.session_state.session_status = SessionStatus::Working;
        normal_mode(&mut app);
        press(
            &mut app,
            KeyCode::Char('x'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert!(
            app.prompt_state.stop_armed_at.is_some(),
            "Ctrl+Alt+X must stop the turn in the {label} map"
        );

        // Open the diff.
        let mut app = test_app();
        app.ui_state.bindings = build_bindings(None, vim_mode);
        app.ensure_tui_cmd_channel();
        normal_mode(&mut app);
        press(
            &mut app,
            KeyCode::Char('d'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        let queued = app
            .tui_cmd_rx
            .as_mut()
            .expect("the command channel is installed")
            .try_recv();
        assert!(
            matches!(queued, Ok(TuiCommand::OpenDiffDialog { .. })),
            "Ctrl+Alt+D must open the diff in the {label} map, got {queued:?}"
        );

        // Focus the prompt.
        let mut app = test_app();
        app.ui_state.bindings = build_bindings(None, vim_mode);
        normal_mode(&mut app);
        press(
            &mut app,
            KeyCode::Char('e'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert!(
            app.ui_state.command_mode,
            "Ctrl+Alt+E must focus the prompt in the {label} map"
        );
    }
}

/// The old letters no longer reach the re-spelled actions in **either** map.
#[test]
fn the_old_letters_no_longer_reach_the_re_spelled_actions() {
    for vim_mode in [false, true] {
        let label = if vim_mode { "vim" } else { "default" };
        let map = build_bindings(None, vim_mode);
        for (code, action) in [
            (KeyCode::Char('x'), InputAction::ToggleAgentRun),
            (KeyCode::Char('d'), InputAction::OpenDiff),
            (KeyCode::Char('i'), InputAction::FocusPrompt),
        ] {
            assert_eq!(
                map.get(&(KeyModifiers::NONE, code)),
                None,
                "the {label} map still binds the old letter {code:?}"
            );
            // The old spelling is gone, not merely outranked: the action has
            // no bare-character spelling that is a **claimed** letter. A bare
            // spelling of a letter the composer does not claim is a different
            // thing — the default map's `/` for focus-prompt is one, and it
            // has to keep working (see the next test).
            assert!(
                !map.iter().any(|((modifiers, key), bound)| {
                    *bound == action
                        && *modifiers == KeyModifiers::NONE
                        && matches!(key, KeyCode::Char(c) if COMPOSER_CLAIMS.iter().any(|claim| claim.key == *c))
                }),
                "the {label} map still gives {action:?} a bare spelling of a claimed letter"
            );
            // And the action is reachable by a chord: a bare letter alone
            // would not be a re-spelling.
            assert!(
                map.iter().any(|((modifiers, key), bound)| {
                    *bound == action
                        && *modifiers != KeyModifiers::NONE
                        && matches!(key, KeyCode::Char(_))
                }),
                "the {label} map has no chord for {action:?}"
            );
        }
    }
}

/// The default map's `/` still opens the command palette.
///
/// `/` is **not** a claimed letter, so the plan's invariant — "a letter the
/// composer does not claim must resolve to the same app action it resolves to
/// today" — requires it to keep working. It is also the gesture that enters
/// command mode, and command mode is what makes the rest of a typed slash
/// command land as text: it is the only way back into the panel manager from
/// a focused panel, which owns every key by contract.
#[test]
fn the_default_maps_slash_still_opens_the_command_palette() {
    let mut app = test_app();
    app.ui_state.bindings = build_bindings(None, false);
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);

    assert!(
        app.ui_state.command_mode,
        "`/` must still open the command palette in the default map"
    );
    assert_eq!(
        prompt_text(&app),
        "/",
        "the command palette's leading slash belongs in the composer"
    );
}

/// The vim map's `i` does not open the command palette any more — the
/// composer's claim took the letter, and the action moved to `Ctrl+Alt+E`.
#[test]
fn the_vim_maps_i_no_longer_opens_the_command_palette() {
    let mut app = test_app();
    app.ui_state.bindings = build_bindings(None, true);
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);

    assert!(
        !app.ui_state.command_mode,
        "the claimed letter `i` reached focus-prompt in the vim map"
    );
}

// ---------------------------------------------------------------------------
// The conditional pair
// ---------------------------------------------------------------------------

/// `k` still scrolls the transcript when the composer buffer is a single
/// line: the claim is conditional, and the condition does not hold.
#[test]
fn k_still_scrolls_the_transcript_on_a_single_line_composer() {
    let mut app = test_app();
    app.prompt_state.prompt.set_text("one line".to_string());
    app.messages_state.messages.scroll = 5;
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);

    assert_eq!(
        app.messages_state.messages.scroll, 4,
        "`k` must still scroll the transcript on a single-line composer buffer"
    );
}

/// `k` is the composer's when the buffer has more than one line, so the
/// transcript does not scroll.
#[test]
fn k_is_claimed_on_a_multi_line_composer() {
    let mut app = test_app();
    app.prompt_state.prompt.set_text("one\ntwo".to_string());
    app.messages_state.messages.scroll = 5;
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);

    assert_eq!(
        app.messages_state.messages.scroll, 5,
        "`k` must be the composer's on a multi-line composer buffer"
    );
}

/// `j` behaves the same way as `k`: the pair is one conditional claim.
#[test]
fn j_follows_the_same_conditional_claim_as_k() {
    let mut app = test_app();
    app.prompt_state.prompt.set_text("one line".to_string());
    app.messages_state.messages.scroll = 5;
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(
        app.messages_state.messages.scroll, 5,
        "`j` must not scroll the transcript on a single-line composer buffer"
    );

    let mut app = test_app();
    app.prompt_state.prompt.set_text("one\ntwo".to_string());
    app.messages_state.messages.scroll = 5;
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(
        app.messages_state.messages.scroll, 5,
        "`j` must be the composer's on a multi-line composer buffer"
    );
}

// ---------------------------------------------------------------------------
// Insert mode still types
// ---------------------------------------------------------------------------

/// Every letter in the claim table is ordinary text in Insert mode.
#[test]
fn every_claim_table_letter_is_typed_as_text_in_insert_mode() {
    for claim in COMPOSER_CLAIMS {
        let mut app = test_app();
        app.ui_state.input_mode = InputMode::Insert;

        press(&mut app, KeyCode::Char(claim.key), KeyModifiers::NONE);

        assert_eq!(
            prompt_text(&app),
            claim.key.to_string(),
            "the claimed letter {:?} was not typed as text in Insert mode",
            claim.key
        );
    }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/// With a dialog open, a claimed letter reaches the **dialog**, not the
/// composer.
///
/// The claim step sits after the modal branch of the key chain, so a modal
/// still owns the key. The help box is the observable: it opens in Normal
/// mode, `i` is its own gesture for "filter this", and once the filter is
/// live the letter lands in the box's query. If the composer claimed `i`
/// first, the box would never enter its filter and the query would stay
/// empty.
#[test]
fn with_a_dialog_open_a_claimed_letter_reaches_the_dialog() {
    let mut app = test_app();
    normal_mode(&mut app);
    app.open_dialog(Dialog::Help(HelpRoute::Command));

    // `i` is a claimed letter, and it is also the help box's own gesture.
    press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    let query = app
        .focus_manager
        .with_dialog::<HelpDialog, _>(DialogType::Help, |dialog| dialog.query().to_string());
    assert_eq!(
        query.as_deref(),
        Some("d"),
        "the claimed letter must reach the dialog, not the composer"
    );
    assert_eq!(
        prompt_text(&app),
        "",
        "the dialog owns the key; the composer must not see it"
    );
}

/// The Workspace view is a different surface with its own composer, so the
/// claim table does not apply there.
///
/// Its dashboard filter is the observable: in Normal mode an unbound letter
/// is filter text, and a claimed letter must stay filter text here rather
/// than being swallowed by the composer's claim.
#[test]
fn the_workspace_views_filter_still_accepts_a_claimed_letter() {
    let mut app = test_app();
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    app.dialog_state.workspace_dashboard = Some(WorkspaceDashboardState::new(return_tab, epoch));
    app.ui_state.routes.navigate_to(Route::Workspace);
    normal_mode(&mut app);

    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    assert_eq!(
        app.dialog_state
            .workspace_dashboard
            .as_ref()
            .expect("the dashboard is open")
            .query,
        "d",
        "the Workspace view's filter must still accept a claimed letter"
    );
    assert_eq!(
        prompt_text(&app),
        "",
        "the Workspace view is a different surface; the composer must not see it"
    );
}

/// A focused panel closes the claim gate, so the composer's claim table is
/// not consulted while another surface owns the keyboard.
#[test]
fn a_focused_panel_closes_the_claim_gate() {
    let mut app = test_app();
    assert!(
        app.panel_host.focus(),
        "precondition: the side panel takes the keyboard"
    );
    app.ui_state.input_mode = InputMode::Normal;

    assert!(
        !app.composer_holds_keyboard(),
        "a focused panel owns the keyboard, so the composer's claim is not live"
    );
}

/// The claim table is a plain table: the lookup is a function of the key
/// event, and it is the same table the guard reads.
#[test]
fn the_claim_lookup_is_the_table() {
    for claim in COMPOSER_CLAIMS {
        for modifiers in claim.claimed_keys_modifiers() {
            let found = composer_claims::claim_for(modifiers, KeyCode::Char(claim.key));
            assert_eq!(
                found.map(|found| found.key),
                Some(claim.key),
                "the lookup did not find the claim for {claim:?} under {modifiers:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The premise the claim table rests on
// ---------------------------------------------------------------------------

/// The composer does not claim `h` and `l`, because the app already moves
/// the composer caret with them — Helix's horizontal motions.
///
/// That premise is only true if the binding exists in **every** map the app
/// can be configured with. It was not: the vim map bound `h` and `l` and the
/// default map bound neither, and `vim_mode` defaults to false. In the
/// default configuration the composer's Normal-mode caret had no letter that
/// could move it — `h` and `l` were unbound, and `w`, `b` and `e` are
/// claimed by the composer and consumed as pending verbs.
///
/// This is the test that would have caught it. The guard's rule G9 checks
/// the same property statically; this one states it in the terms the
/// documents use.
#[test]
fn the_caret_motions_are_bound_the_same_way_in_every_map() {
    for (label, bindings) in [
        ("default", codegg::tui::input::default_bindings()),
        ("vim", codegg::tui::input::vim_bindings()),
    ] {
        assert_eq!(
            bindings.get(&(KeyModifiers::NONE, KeyCode::Char('h'))),
            Some(&InputAction::Left),
            "the {label} map must bind `h` to the caret's left motion: the composer \
             does not claim `h` because {label} mode is supposed to move the caret"
        );
        assert_eq!(
            bindings.get(&(KeyModifiers::NONE, KeyCode::Char('l'))),
            Some(&InputAction::Right),
            "the {label} map must bind `l` to the caret's right motion: the composer \
             does not claim `l` because {label} mode is supposed to move the caret"
        );
    }
}

/// `h` and `l` move the composer caret in Normal mode, in every map.
///
/// The behaviour, not the binding: the point of the premise is that a user
/// in Normal mode can move the caret with the Helix keys. A binding that
/// exists but is shadowed by a surface that owns the keyboard first would
/// satisfy the map-level test and still leave the caret stuck.
#[test]
fn h_and_l_move_the_composer_caret_in_normal_mode() {
    for vim_mode in [false, true] {
        let label = if vim_mode { "vim" } else { "default" };
        let mut app = test_app();
        app.ui_state.bindings = build_bindings(None, vim_mode);
        app.prompt_state.prompt.set_text("hello".into());
        normal_mode(&mut app);

        let end = app.prompt_state.prompt.cursor_pos();
        assert_eq!(end, 5, "the caret starts at the end of the text");

        press(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(
            app.prompt_state.prompt.cursor_pos(),
            4,
            "`h` must move the caret one character left in the {label} map"
        );

        press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(
            app.prompt_state.prompt.cursor_pos(),
            5,
            "`l` must move the caret one character right in the {label} map"
        );
    }
}
