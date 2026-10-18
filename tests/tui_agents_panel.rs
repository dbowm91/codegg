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
//!
//! Bare `/agents` opens the "Send agents to panel" picker rather than jumping
//! straight into the column, so the path continues through the box: the picker
//! opens on the active panel, `j` walks the registry to Agents, and Enter
//! commits. The picker cursor addresses `SidePanelId::ALL` (filtering never
//! re-aims it), so the walk below is computed from the registry, not
//! hand-numbered.

use codegg::tui::app::App;
use codegg::tui::components::component::DialogType;
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

/// Bare `/agents` must open **agents**.
#[test]
fn bare_agents_opens_the_agent_dialog() {
    let mut app = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    type_command(&mut app, "/agents");
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::Agent,
        "bare /agents asks to see agents, so it must open agents — \
         not a question about which panel should hold them"
    );
}

/// `/agents` and `/agent` are the same command wearing two spellings; they must
/// not disagree about what a user gets.
#[test]
fn agents_and_agent_open_the_same_surface() {
    let mut via_plural = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    type_command(&mut via_plural, "/agents");
    let mut via_singular = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    type_command(&mut via_singular, "/agent");
    assert_eq!(
        via_plural.focus_manager.active_dialog_type(),
        via_singular.focus_manager.active_dialog_type(),
        "/agents and /agent must open the same surface"
    );
}

/// Selecting the Agents *panel* is still a real thing; it just lives on the
/// panel manager now, where every other panel selection lives. This is the
/// capability the retired picker used to be the only entry point to.
#[test]
fn the_panel_manager_still_loads_and_focuses_the_agents_panel() {
    let mut app = App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string());
    type_command(&mut app, "/panels");
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::PanelManager,
        "/panels is where panel placement is answered"
    );
    let steps = SidePanelId::Agents
        .index()
        .saturating_sub(app.panel_host.active().index());
    for _ in 0..steps {
        app.on_key(key(KeyCode::Char('j'), KeyModifiers::NONE));
    }
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Agents,
        "the manager must still be able to select the management panel"
    );
    assert!(
        app.panel_host.is_focused(),
        "the panel the user just selected should have the keyboard"
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
