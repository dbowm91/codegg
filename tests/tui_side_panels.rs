//! Swappable side panel — host wiring, selector, and focus ownership.
//!
//! Covers the contract that the panel *host* owns: which panel is active,
//! when it may take keyboard focus, that a focused panel never leaks a key
//! to the composer, and that the panel manager switches panels without a
//! second owner of the side column's focus state.
//!
//! These tests use only the public `App` surface (`on_key`, `process_msg`,
//! public state fields) plus the public `SidePanelHost` accessors, so they
//! exercise the same paths the runtime uses.

use std::sync::Arc;

use codegg::protocol::core::{
    ChatChannelDto, ChatMessageDto, CoreEvent, CoreRequest, CoreResponse, EventEnvelope,
    RequestEnvelope,
};
use codegg::tui::app::state::subagents::SubagentRow;
use codegg::tui::app::state::WorkspaceFocus;
use codegg::tui::app::{TodoEntry, TuiMsg};
use codegg::tui::components::component::DialogType;
use codegg::tui::components::dialogs::panel_manager::PanelManagerDialog;
use codegg::tui::components::sidebar::{
    SidebarFocusTarget, SidebarJob, SidebarSection, SidebarSubagent,
};
use codegg::tui::input::InputMode;
use codegg::tui::panels::SidePanelId;
use codegg::tui::route::Route;
use ratatui::layout::Rect;

use codegg::core::CoreClient;
use codegg::error::AppError;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};

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

/// Press the panel-manager key.
///
/// The one spelling of the manager is `Shift+S`, and it is Normal-mode
/// only, so the mode is stated directly rather than reached through `Esc`:
/// on the Workspace route `Esc` belongs to that view, and routing through
/// it would make these tests depend on a different surface's key handling.
fn press_manager(app: &mut codegg::tui::app::App) {
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.on_key(plain('S'));
}

/// Open the panel manager, asserting it actually opened.
fn open_manager(app: &mut codegg::tui::app::App) {
    press_manager(app);
    assert!(
        manager_open(app),
        "precondition: the panel manager should be open"
    );
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
async fn opening_the_manager_does_not_steal_the_composer() {
    let mut app = test_app();
    open_manager(&mut app);
    assert!(manager_open(&app));
    assert!(
        !app.panel_host.is_focused(),
        "opening the manager must not steal the composer: a user who cancels \
         would find their next keystroke going to a panel they never chose"
    );
    assert_eq!(app.panel_host.active(), SidePanelId::Session);
}

#[tokio::test(flavor = "current_thread")]
async fn the_slash_command_reaches_the_manager_while_composing() {
    // `⇧S` is Normal-mode only, and in Insert it is the letter `S`. With
    // `Ctrl+Alt+W` retired that left the switcher unreachable in the middle
    // of a prompt — the one moment you are most likely to want it. A typed
    // route restores it.
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    for ch in "/panels".chars() {
        app.on_key(plain(ch));
    }
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "/panels",
        "the whole name must land in the composer"
    );
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(manager_open(&app), "/panels must open the same box as ⇧S");
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "",
        "executing a command clears the composer, exactly like every other"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_slash_command_survives_inside_a_focused_panel() {
    // The one place `⇧S` cannot be answered: a focused panel consumes every
    // key by contract, so the *typed* route is the only way back into the
    // manager from there once `Ctrl+Alt+W` is gone.
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    assert!(app.panel_host.is_focused(), "precondition: a focused panel");
    // Leaving the panel is the same `⇧H` gesture a user performs, after
    // which the typed route must still work.
    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "precondition: back at the composer"
    );
    for ch in "/panels".chars() {
        app.on_key(plain(ch));
    }
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        manager_open(&app),
        "the typed route still opens the manager"
    );
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
    open_manager(&mut app);
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
    open_manager(&mut app);
    let before = manager_cursor(&mut app);
    app.on_key(plain('j'));
    let after_vim = manager_cursor(&mut app);

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    open_manager(&mut app);
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
    open_manager(&mut app);
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
    open_manager(&mut app);
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
    open_manager(&mut app);
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
async fn the_manager_key_toggles_the_box_closed() {
    let mut app = test_app();
    open_manager(&mut app);
    assert!(manager_open(&app));
    // The host answers the toggle itself, so it must work from inside the box
    // too rather than stacking a second copy.
    press_manager(&mut app);
    assert!(!manager_open(&app), "the same key must close it again");
}

#[tokio::test(flavor = "current_thread")]
async fn the_manager_key_is_reachable_from_inside_a_focused_panel() {
    // `Ctrl+Alt+W` is retired, so `Shift+S` is the manager's only *chord*.
    // A focused panel consumes everything by contract, so if it also
    // swallowed this key a user standing in a panel would have no chord at
    // all for the switcher they are standing in.
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    assert!(app.panel_host.is_focused(), "precondition: a focused panel");
    press_manager(&mut app);
    assert!(
        manager_open(&app),
        "Shift+S must open the manager from a panel"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn switching_back_to_session_restores_prompt_input_immediately() {
    let mut app = test_app();
    seed_chat(&mut app);
    open_manager(&mut app);
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);

    // Re-open the manager, step back to Session, commit.
    open_manager(&mut app);
    app.on_key(plain('k'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Session);

    app.on_key(key(KeyCode::Char('Q'), KeyModifiers::SHIFT));
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
    open_manager(&mut app);
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.workspace_focus,
        WorkspaceFocus::Chat,
        "a focused chat panel must be reported as such"
    );
    app.on_key(key(KeyCode::Char('Q'), KeyModifiers::SHIFT));
    assert_eq!(app.workspace_focus, WorkspaceFocus::Composer);
}

#[tokio::test(flavor = "current_thread")]
async fn modal_dialogs_still_take_precedence_over_the_panel() {
    let mut app = test_app();
    seed_chat(&mut app);
    open_manager(&mut app);
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
/// Put the app in Normal mode.
///
/// Idempotent on purpose: focusing a side panel already switches the app
/// to Normal, so an unconditional `Esc` here would blur the very panel the
/// caller is about to assert on. Only send the key when it is needed.
fn to_normal(app: &mut codegg::tui::app::App) {
    if app.ui_state.input_mode != codegg::tui::input::InputMode::Normal {
        app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    }
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
    open_manager(app);
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
async fn escape_leaves_insert_mode_and_never_releases_the_panel() {
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

    // `Esc` is the **mode switch** and never takes the keyboard away from a
    // panel: the selection stays where the user put it until they move it
    // with `Shift+H/J/K/L` or leave with `Shift+Q`.
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.panel_host.is_focused(),
        "Esc must not release the panel — Shift+Q is the panel exit"
    );
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "h",
        "Esc must not discard what was typed"
    );

    app.on_key(key(KeyCode::Char('Q'), KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "Shift+Q is how the user leaves the panel"
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
async fn q_is_inert_in_a_focused_panel() {
    // `q` is the program's quit key and `Shift+Q` is the panel exit. A
    // focused panel owns the keyboard, so `q` inside one must do neither:
    // one key, one meaning, and the hint row names `Shift+Q` for leaving.
    let mut app = test_app();
    select_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('q'));
    assert!(
        app.panel_host.is_focused(),
        "q is not the panel exit any more — Shift+Q is"
    );
    assert!(
        app.prompt_state.exit_armed_at.is_none(),
        "and it must not arm the program's quit from inside a panel"
    );
}

// ── Subagent and job sections ───────────────────────────────────────

/// Select and focus the Session panel the way a user does.
fn focus_session(app: &mut codegg::tui::app::App) {
    to_normal(app);
    open_manager(app);
    // The manager opens on the live panel, which is Session; committing it
    // without moving is how the user focuses the panel they are looking at.
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Session,
        "precondition: the session panel is active"
    );
    assert!(
        app.panel_host.is_focused(),
        "precondition: a focused panel owns the keyboard"
    );
}

fn sidebar_subagent(task_id: &str, name: &str, agent: &str, status: &str) -> SidebarSubagent {
    SidebarSubagent {
        child_instance_id: Some(task_id.to_string()),
        task_id: task_id.to_string(),
        name: name.to_string(),
        agent: agent.to_string(),
        status: status.to_string(),
        profile: None,
    }
}

/// Publish one subagent to every surface that reads it.
///
/// Both sides are seeded because a snapshot does both in one pass: the panel's
/// `Subagents` section and `SubagentState.rows` are two renderings of the same
/// derived vector. Seeding only the panel would be a state the runtime never
/// produces — and would silently pass or fail for the wrong reason, since
/// `open_task` resolves the row from `SubagentState` and refuses a `task_id`
/// with nothing behind it.
fn seed_subagent(
    app: &mut codegg::tui::app::App,
    task_id: &str,
    name: &str,
    agent: &str,
    status: &str,
) {
    app.sidebar
        .set_subagents(vec![sidebar_subagent(task_id, name, agent, status)]);
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        child_instance_id: Some(task_id.to_string()),
        task_id: task_id.to_string(),
        run_id: None,
        display_name: Some(name.to_string()),
        agent: agent.to_string(),
        description: "map the subsystem".to_string(),
        profile: None,
        usefulness: None,
        status: status.to_string(),
    }]);
}

/// The panel's `Enter` on a subagent row opens that subagent's buffer tab.
///
/// The whole chain is exercised: key → binding → `SessionPanel::handle_key`
/// → `PanelKey::Intent` → `process_msg` → the tab set. The panel routes an
/// intent rather than touching the tab set itself, so this is also the test
/// that would fail if the panel ever grew direct authority over tabs.
#[tokio::test(flavor = "current_thread")]
async fn enter_on_a_session_panel_subagent_row_opens_its_buffer_tab() {
    let mut app = test_app();
    seed_subagent(&mut app, "42", "Sally", "explore", "running");
    focus_session(&mut app);

    assert!(
        app.ui_state.subagents.active_tab().is_none(),
        "precondition: no subagent tab is open yet"
    );
    // Section(Subagents) header, then the row itself.
    app.on_key(plain('j'));
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.ui_state
            .subagents
            .active_tab()
            .map(|tab| tab.tab_id.as_str().to_string()),
        Some("42".to_string()),
        "Enter on a subagent row opens exactly the tab it addresses"
    );
}

/// Give the app a laid-out sidebar and return the terminal row whose tooltip
/// contains `needle`.
///
/// `App` has no geometry until it renders, and `clickable_area_at` derives
/// `ClickTarget::Sidebar` from `sidebar_area`, so a click test has to supply
/// one. The row is *found* through the hover map rather than hard-coded: the
/// whole point of the alignment invariant is that a row's rendered position
/// is not a constant, and a test that hard-codes it would pass even when the
/// map and the render disagree.
fn sidebar_row_containing(app: &mut codegg::tui::app::App, needle: &str) -> (Rect, u16) {
    let area = Rect::new(0, 0, 40, 40);
    app.sidebar_area = Some(area);
    for y in 1..area.height {
        app.sidebar.set_hover_position(2, y, Some(area));
        if app
            .sidebar
            .get_tooltip()
            .is_some_and(|tooltip| tooltip.contains(needle))
        {
            return (area, y);
        }
    }
    panic!("no sidebar row whose tooltip contains {needle:?}");
}

/// `Enter` and a click resolve to the same tab for the same row.
///
/// They take different routes through the app — the keyboard through the
/// panel, the mouse through `ClickTarget::Sidebar` — so agreement is a
/// property to assert rather than something the shared `SidebarActivation`
/// gives for free.
#[tokio::test(flavor = "current_thread")]
async fn a_clicked_subagent_row_opens_the_same_tab_as_enter() {
    let mut entered = test_app();
    seed_subagent(&mut entered, "42", "Sally", "explore", "running");
    focus_session(&mut entered);
    entered.on_key(plain('j'));
    entered.on_key(plain('j'));
    entered.on_key(key(KeyCode::Enter, KeyModifiers::NONE));

    let mut clicked = test_app();
    seed_subagent(&mut clicked, "42", "Sally", "explore", "running");
    let (area, row) = sidebar_row_containing(&mut clicked, "Sally");
    clicked.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(area.height, 40, "sanity: the synthetic layout has rows");
    assert_eq!(
        entered
            .ui_state
            .subagents
            .active_tab()
            .map(|tab| tab.tab_id.as_str().to_string()),
        Some("42".to_string())
    );
    assert_eq!(
        clicked
            .ui_state
            .subagents
            .active_tab()
            .map(|tab| tab.tab_id.as_str().to_string()),
        Some("42".to_string()),
        "click and Enter must open one tab for one row, not two subjects"
    );
}

/// A job row is status only: `Enter` opens nothing and reaches no composer.
#[tokio::test(flavor = "current_thread")]
async fn a_job_row_is_read_only_and_never_reaches_the_composer() {
    let mut app = test_app();
    app.sidebar.set_jobs(vec![SidebarJob {
        job_id: "j1".to_string(),
        kind: "test".to_string(),
        summary: "cargo test".to_string(),
        state: "running".to_string(),
        error_class: None,
    }]);
    focus_session(&mut app);

    app.on_key(plain('j'));
    app.on_key(plain('j'));
    assert_eq!(
        app.sidebar.focused_target(),
        Some(&SidebarFocusTarget::Job("j1".to_string())),
        "the job row is reachable by keyboard"
    );
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        app.ui_state.subagents.active_tab().is_none(),
        "there is no buffer for a job to open"
    );
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "",
        "the key was consumed by the panel, not submitted as a turn"
    );
    assert!(app.panel_host.is_focused());
}

/// The session panel's keyboard walk skips todo **rows** and keeps their
/// header.
///
/// A todo row had no action behind it — `Enter` on it resolved to
/// `SidebarActivation::None` — so the selection highlight was painted on a row
/// where nothing happened, and `j`/`k` walked through every one of them. The
/// "Todos" header stays focusable and still folds the section: the request was
/// to skip the rows, not the header.
#[tokio::test(flavor = "current_thread")]
async fn the_session_panel_walk_skips_todo_rows_but_keeps_their_header() {
    let mut app = test_app();
    app.sidebar.set_todos(vec![
        TodoEntry {
            content: "write the thing".to_string(),
            status: "pending".to_string(),
            priority: String::new(),
        },
        TodoEntry {
            content: "write the other thing".to_string(),
            status: "pending".to_string(),
            priority: String::new(),
        },
    ]);
    app.sidebar
        .set_subagents(vec![sidebar_subagent("42", "Sally", "explore", "running")]);
    app.sidebar.set_jobs(vec![SidebarJob {
        job_id: "j1".to_string(),
        kind: "test".to_string(),
        summary: "cargo test".to_string(),
        state: "running".to_string(),
        error_class: None,
    }]);
    focus_session(&mut app);

    // Pin the walk to the first target, so the sequence does not depend on
    // where the panel happened to leave the cursor.
    app.on_key(plain('k'));
    let mut walk = Vec::new();
    for _ in 0..5 {
        walk.push(app.sidebar.focused_target().cloned());
        app.on_key(plain('j'));
    }

    assert_eq!(
        walk,
        vec![
            Some(SidebarFocusTarget::Section(SidebarSection::Todos)),
            Some(SidebarFocusTarget::Section(SidebarSection::Subagents)),
            Some(SidebarFocusTarget::Subagent("42".to_string())),
            Some(SidebarFocusTarget::Section(SidebarSection::Jobs)),
            Some(SidebarFocusTarget::Job("j1".to_string())),
        ],
        "the walk skips the two todo rows and keeps the header"
    );

    // And the walk is exhausted: there is no sixth stop for a todo row to
    // hide behind.
    app.on_key(plain('j'));
    assert_eq!(
        app.sidebar.focused_target(),
        Some(&SidebarFocusTarget::Job("j1".to_string())),
        "the walk ran past the last actionable row"
    );
}
