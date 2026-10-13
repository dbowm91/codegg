//! Regression: `/agents` recursed until the stack died.
//!
//! `Agents` is a *bespoke* panel — it owns an `AgentsPanel` rather than
//! adopting a box. The hosted list excluded only Session/Chat/Terminal/Help,
//! so `Agents` got a hosted placeholder as well, and
//! `materialize_hosted_panel` read that placeholder as "an unloaded box",
//! called `open_panel_box`, which called `select_panel`, which called
//! `materialize_hosted_panel`. Pressing Enter on `/agents` overflowed the
//! stack and aborted the process.
//!
//! These drive the real key path — `/`, the characters, Enter — because the
//! recursion started at submit time, not at open time.

use codegg::tui::app::App;
use codegg::tui::panels::SidePanelId;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn type_command(app: &mut App, command: &str) {
    for ch in command.chars() {
        app.on_key(key(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
}

/// The crash itself. A stack overflow aborts the process, so "this test
/// finishes" is the assertion.
#[test]
fn the_agents_command_loads_its_panel_instead_of_recursing() {
    let mut app = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    type_command(&mut app, "/agents");
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Agents,
        "/agents selects the management panel"
    );
}

/// `/agents` opens the panel to be *used*. Selecting without focusing left the
/// keyboard in the composer and then warned that the panel had not taken
/// focus — true of the keyboard, false of the panel.
#[test]
fn the_agents_panel_takes_the_keyboard() {
    let mut app = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    type_command(&mut app, "/agents");
    assert!(
        app.panel_host.is_focused(),
        "the panel the user just opened should have the keyboard"
    );
}

/// The invariant behind the fix, stated where a future bespoke panel will
/// read it: a bespoke panel must never look like an unloaded hosted panel.
#[test]
fn no_bespoke_panel_reads_as_an_unloaded_hosted_one() {
    use codegg::tui::panels::SidePanelId as P;
    let app = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    for panel in [P::Session, P::Agents, P::Chat, P::Terminal, P::Help] {
        assert!(panel.is_bespoke(), "{panel:?} is one of the bespoke panels");
        assert_eq!(
            app.panel_host.hosted_panel_has_content(panel),
            None,
            "{panel:?} must have no hosted placeholder: `Some(false)` sends \
             materialize_hosted_panel into open_panel_box and straight back"
        );
    }
}
