//! Swappable side panel — host wiring, selector, and focus ownership.
//!
//! Covers the contract that the panel *host* owns: which panel is active,
//! when it may take keyboard focus, that a focused panel never leaks a key
//! to the composer, and that the `Ctrl+Alt+W` selector switches panels
//! without a second owner of the side column's focus state.
//!
//! These tests use only the public `App` surface (`on_key`, `process_msg`,
//! public state fields) plus the public `SidePanelHost` accessors, so they
//! exercise the same paths the runtime uses.

use std::sync::Arc;

use codegg::protocol::core::{
    ChatChannelDto, ChatMessageDto, CoreEvent, CoreRequest, CoreResponse, EventEnvelope,
    RequestEnvelope,
};
use codegg::tui::app::state::WorkspaceFocus;
use codegg::tui::app::TuiMsg;
use codegg::tui::panels::SidePanelId;
use codegg::tui::route::Route;

use codegg::core::CoreClient;
use codegg::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

/// Project bound by the active tab; every chat locator resolves to it.
const PROJECT: &str = "project-a";

// ── Fake core ────────────────────────────────────────────────────────

struct NullClient;

#[async_trait::async_trait]
impl CoreClient for NullClient {
    async fn request(
        &self,
        _request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        // Nothing here needs a live daemon: every assertion is about which
        // panel holds focus and where a keystroke lands.
        Err(AppError::Tui("no daemon in panel tests".to_string()))
    }

    fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        rx
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// An app with one explicitly-bound project tab.
///
/// The tab matters: the chat panel's project is resolved from the active
/// tab's execution context, never from the process cwd, so a tab-less app
/// could never bind the chat panel and every chat assertion would be
/// vacuous.
fn test_app() -> codegg::tui::app::App {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();

    let mut app = codegg::tui::app::App::new_for_testing(root.display().to_string());
    app.project_tabs = codegg::tui::app::state::ProjectTabs::new();
    let mut tab = codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "Alpha".to_string(),
    );
    tab.project_id = Some(PROJECT.to_string());
    tab.workspace_id = Some("workspace-a".to_string());
    tab.workspace_root = Some(root);
    app.project_tabs.add_tab(tab);
    let first = app.project_tabs.ordered()[0].tab_id.clone();
    app.project_tabs.set_active(&first);

    app.set_core_client(Arc::new(NullClient));
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    app.tui_cmd_tx = Some(tx);
    app
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// The legacy-safe chord form. `Ctrl+Shift+X` is byte-identical to `Ctrl+X`
/// in legacy xterm encoding and can never arrive as written; see
/// `tests/modifier_chords.rs`.
fn ctrl_alt(ch: char) -> KeyEvent {
    key(KeyCode::Char(ch), KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn plain(ch: char) -> KeyEvent {
    key(KeyCode::Char(ch), KeyModifiers::NONE)
}

fn chat_message(seq: u64, body: &str) -> ChatMessageDto {
    ChatMessageDto {
        message_id: format!("m{seq}"),
        channel_id: "ch1".to_string(),
        project_id: PROJECT.to_string(),
        seq,
        author_principal: "alice".to_string(),
        author_agent: None,
        body: body.to_string(),
        reply_to: None,
        thread_root: None,
        mentions: Vec::new(),
        references: Vec::new(),
        revision: 1,
        edited_at_ms: None,
        redacted: false,
        created_at_ms: 100 + seq as i64,
    }
}

fn seed_chat(app: &mut codegg::tui::app::App) {
    let epoch = app.chat.reconnect_epoch;
    let id = app.chat.begin_ensure(PROJECT).expect("begin_ensure");
    assert!(app.chat.apply_channel_ensured(
        id,
        PROJECT,
        &ChatChannelDto {
            channel_id: "ch1".to_string(),
            project_id: PROJECT.to_string(),
            name: "general".to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 1,
        },
        epoch,
    ));
    let request = app
        .chat
        .begin_history(PROJECT, "ch1")
        .expect("begin_history");
    assert!(app.chat.apply_history(
        request,
        PROJECT,
        "ch1",
        vec![chat_message(1, "hello")],
        1,
        false,
        0,
        epoch,
    ));
}

// ── Tests ────────────────────────────────────────────────────────────

#[test]
fn session_panel_is_the_default_and_starts_unfocused() {
    let app = test_app();
    assert_eq!(app.panel_host.active(), SidePanelId::Session);
    assert!(
        !app.panel_host.is_focused(),
        "startup must not capture the keyboard"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_alt_w_opens_the_selector_over_the_active_panel() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    assert!(app.panel_host.selector_open());
    assert!(
        !app.panel_host.is_focused(),
        "opening the switcher must not steal the composer: a user who cancels \
         would find their next keystroke going to a panel they never chose"
    );
    assert_eq!(app.panel_host.active(), SidePanelId::Session);
}

#[tokio::test(flavor = "current_thread")]
async fn selector_commits_the_highlighted_panel_on_enter() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    // `l` moves one column right in the two-column grid.
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
    assert!(app.panel_host.is_focused());
    assert!(
        !app.panel_host.selector_open(),
        "the overlay must close once it has been answered"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn selector_arrow_keys_match_the_vim_keys() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    let before = app.panel_host.selector_cursor();
    app.on_key(plain('l'));
    let after_vim = app.panel_host.selector_cursor();

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(ctrl_alt('w'));
    assert_eq!(app.panel_host.selector_cursor(), before);
    app.on_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.panel_host.selector_cursor(), after_vim);
}

#[tokio::test(flavor = "current_thread")]
async fn escape_cancels_the_selector_and_keeps_the_current_panel() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.panel_host.selector_open());
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Session,
        "a cancelled switch must not change the panel"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn selector_keys_never_reach_the_composer() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    for ch in ['/', ':', 'j', 'k', 'h', 'l', 'x'] {
        app.on_key(plain(ch));
    }
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "the overlay must swallow navigation keys: {:?}",
        app.prompt_state.prompt.get_text()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_focused_chat_panel_owns_printable_keys() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);

    // `i` opens the panel composer, then the letters become message text.
    app.on_key(plain('i'));
    for ch in "hello".chars() {
        app.on_key(plain(ch));
    }
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "typing in the panel must never become a prompt turn: {:?}",
        app.prompt_state.prompt.get_text()
    );
    assert_eq!(
        app.chat.draft_for(PROJECT),
        "hello",
        "the typed text belongs to the chat composer"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn escape_returns_from_the_panel_to_the_composer() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    app.on_key(plain('i'));
    assert!(app.panel_host.chat_panel_mut().mode().is_input());

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        !app.panel_host.chat_panel_mut().mode().is_input(),
        "Esc leaves Compose before it leaves the panel"
    );
    assert!(app.panel_host.is_focused());

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        !app.panel_host.is_focused(),
        "a second Esc must return keys to the composer"
    );
    app.on_key(plain('z'));
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "z",
        "once focus is released the composer must work again"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn switching_back_to_session_restores_prompt_input_immediately() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);

    // Re-open the selector, step left to Session, commit.
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('h'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Session);

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(plain('q'));
    assert!(!app.panel_host.is_focused());
}

#[tokio::test(flavor = "current_thread")]
async fn q_also_drops_panel_focus() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.panel_host.chat_panel_mut().mode().is_input());
    app.on_key(plain('q'));
    assert!(!app.panel_host.is_focused());
}

#[tokio::test(flavor = "current_thread")]
async fn q_types_while_the_chat_composer_is_open() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    app.on_key(plain('i'));
    app.on_key(plain('q'));
    assert_eq!(
        app.chat.draft_for(PROJECT),
        "q",
        "q must be text while composing, not an exit"
    );
    assert!(app.panel_host.is_focused());
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_b_is_refused_while_the_side_column_is_hidden() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = false;
    app.on_key(ctrl_alt('w'));
    assert!(
        !app.panel_host.selector_open(),
        "the switcher describes the region it switches; with no region there is nothing to select into"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_entry_selects_the_chat_panel() {
    let mut app = test_app();
    assert_eq!(app.panel_host.active(), SidePanelId::Session);
    app.process_msg(TuiMsg::OpenWorkspaceDashboard);
    assert!(matches!(app.ui_state.routes.current(), Route::Workspace));
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Chat,
        "Workspace keeps its M005 behaviour of showing project chat on entry"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_focus_mirrors_which_side_owns_the_keyboard() {
    let mut app = test_app();
    app.process_msg(TuiMsg::OpenWorkspaceDashboard);
    assert_eq!(
        app.workspace_focus,
        WorkspaceFocus::Composer,
        "selecting a panel is not the same as focusing it"
    );
    app.on_key(ctrl_alt('w'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.workspace_focus,
        WorkspaceFocus::Chat,
        "a focused chat panel must be reported as such"
    );
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.workspace_focus, WorkspaceFocus::Composer);
}

#[tokio::test(flavor = "current_thread")]
async fn modal_dialogs_still_take_precedence_over_the_panel() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('l'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.panel_host.is_focused());

    app.process_msg(TuiMsg::OpenHelpDialog);
    assert!(!app.focus_manager.is_empty());
    // Escape closes the modal rather than dropping panel focus.
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.panel_host.is_focused(),
        "the panel kept focus while the modal was up"
    );
}
