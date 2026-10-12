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
use codegg::tui::components::component::DialogType;
use codegg::tui::components::dialogs::panel_manager::PanelManagerDialog;
use codegg::tui::input::InputMode;
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

/// The manager is a dialog, so these tests ask the dialog what it is
/// showing rather than reaching into the host for a cursor.
fn manager_open(app: &codegg::tui::app::App) -> bool {
    app.focus_manager.has_dialog(DialogType::PanelManager)
}

fn manager_cursor(app: &mut codegg::tui::app::App) -> Option<usize> {
    app.focus_manager
        .with_component_mut::<PanelManagerDialog, _>(|dialog| dialog.cursor())
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_alt_w_opens_the_manager_without_stealing_the_composer() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    assert!(manager_open(&app));
    assert!(
        !app.panel_host.is_focused(),
        "opening the manager must not steal the composer: a user who cancels \
         would find their next keystroke going to a panel they never chose"
    );
    assert_eq!(app.panel_host.active(), SidePanelId::Session);
}

#[tokio::test(flavor = "current_thread")]
async fn shift_s_opens_the_same_manager_in_normal_mode_only() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.on_key(plain('S'));
    assert!(manager_open(&app), "Shift+S is the vim-shaped spelling");

    // In Insert it must stay the letter S.
    let mut typing = test_app();
    typing.ui_state.input_mode = InputMode::Insert;
    typing.on_key(plain('S'));
    assert!(
        !manager_open(&typing),
        "the manager key must never claim a letter mid-word"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_manager_commits_the_highlighted_panel_on_enter() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('j')); // Session -> Chat
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
    assert!(
        app.panel_host.is_focused(),
        "focus follows the pick: the manager exists to get the user to a panel"
    );
    assert!(
        !manager_open(&app),
        "the box closes once it has been answered"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn manager_arrow_keys_match_the_vim_keys() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    let before = manager_cursor(&mut app);
    app.on_key(plain('j'));
    let after_vim = manager_cursor(&mut app);

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(ctrl_alt('w'));
    assert_eq!(
        manager_cursor(&mut app),
        before,
        "reopens where it left off"
    );
    app.on_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(manager_cursor(&mut app), after_vim);
}

#[tokio::test(flavor = "current_thread")]
async fn escape_closes_the_manager_and_keeps_the_current_panel() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!manager_open(&app));
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Session,
        "a cancelled switch must not change the panel"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn manager_keys_never_reach_the_composer() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    for ch in [':', 'j', 'k', 'x'] {
        app.on_key(plain(ch));
    }
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "the box must swallow its own keys: {:?}",
        app.prompt_state.prompt.get_text()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_digit_binds_the_highlighted_panel_and_ctrl_recalls_it() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('j')); // highlight Chat
    app.on_key(plain('1')); // bind Ctrl+1 -> Chat
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(
        app.panel_hotkeys.get('1'),
        Some(SidePanelId::Chat),
        "the binding must survive being assigned"
    );

    // Now recall it. The box is gone, so this must switch panels.
    app.on_key(key(KeyCode::Char('1'), KeyModifiers::CONTROL));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
}

#[tokio::test(flavor = "current_thread")]
async fn an_unbound_ctrl_digit_is_consumed_rather_than_leaking() {
    // The digit range belongs to the manager. A `Ctrl+7` with nothing saved
    // must not fall through into the composer or any other surface.
    let mut app = test_app();
    app.on_key(key(KeyCode::Char('7'), KeyModifiers::CONTROL));
    assert!(app.panel_hotkeys.get('7').is_none());
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "an unbound slot must still be swallowed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_alt_w_toggles_the_manager_closed() {
    let mut app = test_app();
    app.on_key(ctrl_alt('w'));
    assert!(manager_open(&app));
    // The host answers the toggle itself, so it must work from inside the box
    // too rather than stacking a second copy.
    app.on_key(ctrl_alt('w'));
    assert!(!manager_open(&app), "the same key must close it again");
}

#[tokio::test(flavor = "current_thread")]
async fn switching_back_to_session_restores_prompt_input_immediately() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);

    // Re-open the manager, step back to Session, commit.
    app.on_key(ctrl_alt('w'));
    app.on_key(plain('k'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Session);

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(plain('q'));
    assert!(!app.panel_host.is_focused());
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

/// Enter Normal mode the way a user does.
///
/// The focus moves are Normal-only by contract, and the app boots into
/// Insert — so every test that reaches for `Shift+Arrow` has to make the
/// same trip a user makes, or it would assert against a state the user
/// cannot actually be in.
fn to_normal(app: &mut codegg::tui::app::App) {
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Normal,
        "precondition: the focus moves only exist in Normal mode"
    );
}

/// Select the Chat panel the way a user does: open the switcher, step to
/// it, commit. This is also the only way the panel gets a bound project, so
/// almost every focus/target assertion has to start here.
fn select_chat(app: &mut codegg::tui::app::App) {
    seed_chat(app);
    app.on_key(ctrl_alt('w'));
    // The manager opens on the live panel (Session) and steps with j/k, so
    // one `j` lands on Chat.
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Chat,
        "precondition: the chat panel is active"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_b_works_repeatedly_while_a_panel_is_focused() {
    // Regression: the toggle used to work exactly once. The first press
    // focused a panel, and because a focused panel consumed every key the
    // second press was swallowed by the panel that press had just focused.
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    assert!(app.panel_host.is_focused(), "precondition: a focused panel");

    for round in 0..3 {
        app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(
            !app.ui_state.sidebar_visible,
            "round {round}: Ctrl+B must hide the column from inside a focused panel"
        );
        app.on_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(
            app.ui_state.sidebar_visible,
            "round {round}: Ctrl+B must show it again"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shift_right_focuses_the_panel_and_shift_left_returns() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert!(!app.panel_host.is_focused(), "precondition: focus in main");

    app.on_key(key(KeyCode::Right, KeyModifiers::SHIFT));
    assert!(
        app.panel_host.is_focused(),
        "Shift+Right must move focus into the side column"
    );

    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "Shift+Left must move focus back to the main buffer"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn shift_h_returns_focus_either_encoding() {
    // Terminals disagree on whether a shifted letter carries SHIFT. Both
    // forms must reach the same action, or the focus move is a keypress
    // that works on one machine and not another.
    //
    // `L` is not here any more: it now docks an open box into the panel
    // (`dock_*` tests), which is a different intent from moving the
    // keyboard. `Shift+Right` owns panel focus.
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        let mut app = test_app();
        select_chat(&mut app);
        to_normal(&mut app);
        app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(
            !app.panel_host.is_focused(),
            "precondition: focus starts in the main buffer"
        );
        app.on_key(key(KeyCode::Right, KeyModifiers::SHIFT));
        assert!(
            app.panel_host.is_focused(),
            "Shift+Right must focus the panel"
        );
        app.on_key(key(KeyCode::Char('H'), modifiers));
        assert!(
            !app.panel_host.is_focused(),
            "H/{modifiers:?} must return to the main buffer"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shift_arrows_do_not_move_focus_in_insert_mode() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert!(!app.panel_host.is_focused());

    app.on_key(plain('i'));
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Insert,
        "precondition: composing"
    );

    app.on_key(key(KeyCode::Right, KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "the focus move is a Normal-mode gesture; Shift+Right is caret movement while composing"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bare_arrows_keep_their_existing_meaning() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    // Return focus to the main buffer, so a bare Right is judged against the
    // main-buffer bindings rather than the panel's navigation.
    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert!(!app.panel_host.is_focused(), "precondition: focus in main");

    app.on_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert!(
        !app.panel_host.is_focused(),
        "an unmodified Right must not have become a focus move"
    );
    assert_eq!(
        app.input_target(),
        codegg::tui::panels::InputTarget::Build,
        "an unmodified arrow must not have armed the chat target"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_composer_target_follows_the_focused_panel() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    assert_eq!(
        app.input_target(),
        codegg::tui::panels::InputTarget::Chat,
        "a focused chat panel arms chat delivery"
    );

    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert_eq!(
        app.input_target(),
        codegg::tui::panels::InputTarget::Build,
        "leaving the panel re-arms the agent automatically — the target is derived, never latched"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_agent_session_is_the_default_target() {
    let app = test_app();
    assert_eq!(
        app.input_target(),
        codegg::tui::panels::InputTarget::Build,
        "with no panel focused the composer targets the agent session"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_focused_chat_panel_in_insert_mode_leaves_typing_to_the_composer() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    app.on_key(key(KeyCode::Right, KeyModifiers::SHIFT));
    assert!(app.panel_host.is_focused());

    app.on_key(plain('i'));
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Insert,
        "i in the panel opens the shared composer"
    );

    app.on_key(plain('h'));
    app.on_key(plain('i'));
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "hi",
        "the composer owns typing while the panel is focused in Insert mode"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn escape_leaves_insert_mode_before_it_leaves_the_panel() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('i'));
    app.on_key(plain('h'));
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Insert
    );

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Normal,
        "Esc must leave Insert first so a typed letter is never unfocusable"
    );
    assert!(app.panel_host.is_focused(), "one Esc is not two");

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        !app.panel_host.is_focused(),
        "a second Esc releases the panel"
    );
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "h",
        "Esc must not discard what was typed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn q_types_rather_than_unfocusing_while_composing() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('i'));
    app.on_key(plain('q'));
    assert!(
        app.panel_host.is_focused(),
        "q is a letter while composing, not a command"
    );
    assert_eq!(app.prompt_state.prompt.get_text(), "q");
}

#[tokio::test(flavor = "current_thread")]
async fn q_still_unfocuses_the_panel_in_normal_mode() {
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('q'));
    assert!(
        !app.panel_host.is_focused(),
        "outside Insert mode q keeps its exit meaning"
    );
}
