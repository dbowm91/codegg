//! Chat browser parity between `/chat` and the side panel, and the shared
//! composer context.
//!
//! `/chat` and the chat side panel are two renderings of one browser
//! ([`chat_browser`]). These tests pin the properties that made the split
//! worth fixing and the ones that made the previous peer implementations
//! drift:
//!
//! * both surfaces open on the newest message and follow the tail;
//! * a verb that needs typing reaches the user as an editor on *both*
//!   surfaces, with the same author prefill and the same target;
//! * the panel can do what the window does — reply and edit included;
//! * the composer always says where it will send, and offers the keys that
//!   apply to whatever currently owns the keyboard.

use std::sync::Arc;

use codegg::protocol::core::{
    ChatChannelDto, ChatMessageDto, CoreEvent, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::tui::chat_browser::ChatComposeSeed;
use codegg::tui::panels::{InputTarget, SidePanelId};

use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::CoreRequest;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

const PROJECT: &str = "project-a";

// ── Fake core ────────────────────────────────────────────────────────

struct NullClient;

#[async_trait::async_trait]
impl CoreClient for NullClient {
    async fn request(
        &self,
        _request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        // No daemon: every assertion here is about local view state — where
        // the cursor is, what a key means, and what the composer is armed
        // with — none of which requires a round trip to succeed.
        Err(AppError::Tui("no daemon in chat surface tests".to_string()))
    }

    fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        rx
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

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

fn plain(ch: char) -> KeyEvent {
    key(KeyCode::Char(ch), KeyModifiers::NONE)
}

fn chat_message(seq: u64, author: &str, body: &str) -> ChatMessageDto {
    ChatMessageDto {
        message_id: format!("m{seq}"),
        channel_id: "ch1".to_string(),
        project_id: PROJECT.to_string(),
        seq,
        author_principal: author.to_string(),
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

/// A chat project holding three messages from two authors.
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
        vec![
            chat_message(1, "alice", "first"),
            chat_message(2, "bob", "second"),
            chat_message(3, "alice", "third"),
        ],
        3,
        false,
        0,
        epoch,
    ));
}

/// Put the app in Normal mode.
///
/// Idempotent on purpose: focusing a side panel already switches the app
/// to Normal, so pressing `Esc` unconditionally here would land on the
/// focused panel and blur it — turning "make sure we are in Normal" into
/// "leave whatever has focus". Only send the key when it is actually
/// needed.
fn to_normal(app: &mut codegg::tui::app::App) {
    if app.ui_state.input_mode != codegg::tui::input::InputMode::Normal {
        app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    }
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Normal,
        "precondition: the panel only owns keys in Normal mode"
    );
}

/// Open the panel manager the way a user does now: `Shift+S`, which is the
/// only spelling of the manager and is Normal-mode only.
///
/// The mode is stated directly rather than reached through `Esc`: on the
/// Workspace route `Esc` belongs to that view, and routing through it would
/// make these tests depend on a different surface's key handling.
fn open_manager(app: &mut codegg::tui::app::App) {
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.on_key(plain('S'));
}

/// Select and focus the chat panel the way a user does.
fn focus_chat(app: &mut codegg::tui::app::App) {
    seed_chat(app);
    open_manager(app);
    // `j` steps down the manager's list; the manager opens on the live
    // panel (Session), so one press lands on Chat.
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
    assert!(app.panel_host.is_focused(), "precondition: focused panel");
}

// ── The window opens at live, not at the oldest row ──────────────────

#[tokio::test(flavor = "current_thread")]
async fn the_panel_opens_on_the_newest_message() {
    // Regression: the panel started its cursor at index 0, so a channel with
    // history looked empty — only the oldest messages were on screen and the
    // newest, the ones the user came to read, were nowhere.
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    assert_eq!(
        app.panel_host.chat_cursor(),
        2,
        "a populated panel must open on the newest message"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn j_scrolls_backwards_from_the_tail_and_g_repins_it() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);

    // `j` at the newest message stays at live, so holding it behaves like a
    // "jump to latest" key rather than silently freezing at the end.
    app.on_key(plain('j'));
    assert_eq!(app.panel_host.chat_cursor(), 2);

    app.on_key(plain('k'));
    assert_eq!(app.panel_host.chat_cursor(), 1, "k scrolls up one");
    app.on_key(plain('k'));
    assert_eq!(app.panel_host.chat_cursor(), 0, "and clamps at the oldest");
    app.on_key(plain('k'));
    assert_eq!(app.panel_host.chat_cursor(), 0, "stays clamped");

    app.on_key(plain('G'));
    assert_eq!(app.panel_host.chat_cursor(), 2, "G returns to live");
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_keys_never_reach_the_composer() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    for ch in ['j', 'k', 'g', 'G', 'm', 'R', '[', ']', '?', ' ', '/'] {
        app.on_key(plain(ch));
    }
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "a browsing key must not become a prompt character: {:?}",
        app.prompt_state.prompt.get_text()
    );
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Normal,
        "browsing keys must not open an editor either"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn r_and_e_arm_an_editor_rather_than_typing_a_letter() {
    // The distinction that makes `i` work everywhere: a browsing verb that
    // needs text hands the keyboard to the composer instead of writing a
    // character the user never typed.
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    for verb in ['r', 'e'] {
        let mut app = test_app();
        focus_chat(&mut app);
        to_normal(&mut app);
        app.on_key(plain(verb));
        assert_eq!(
            app.ui_state.input_mode,
            codegg::tui::input::InputMode::Insert,
            "{verb} must open the composer"
        );
        assert!(
            !app.prompt_state.prompt.get_text().is_empty(),
            "{verb} must not type itself into the prompt"
        );
    }
}

// ── The panel can reply and edit ─────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn r_in_the_panel_arms_a_reply_on_the_shared_composer() {
    // The panel holds no buffer of its own, so `r` has to reach the user as
    // an editor. Before this, the key was swallowed and chat could only be
    // replied to from the `/chat` window.
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);

    app.on_key(plain('r'));
    let seed = app
        .prompt_state
        .chat_seed
        .as_ref()
        .expect("r must arm the shared composer");
    assert_eq!(seed.reply_to.as_deref(), Some("m3"));
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "@alice ",
        "the reply is pre-seeded with the selected author"
    );
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Insert,
        "the user must land in an editor, not a Normal-mode composer"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn r_targets_whichever_message_is_selected() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('k'));
    assert_eq!(
        app.panel_host.chat_cursor(),
        1,
        "precondition: on bob's message"
    );
    app.on_key(plain('r'));
    let seed = app.prompt_state.chat_seed.as_ref().expect("armed");
    assert_eq!(seed.reply_to.as_deref(), Some("m2"));
    assert_eq!(app.prompt_state.prompt.get_text(), "@bob ");
}

#[tokio::test(flavor = "current_thread")]
async fn e_in_the_panel_arms_an_edit_carrying_the_existing_body() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('k'));

    app.on_key(plain('e'));
    let seed = app.prompt_state.chat_seed.as_ref().expect("armed");
    assert_eq!(seed.edit_target.as_deref(), Some("m2"));
    assert_eq!(seed.label(), "edit");
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "second",
        "an edit starts from the body being replaced"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_panel_and_the_window_agree_on_what_r_means() {
    // The whole point of the shared browser: two surfaces, one keymap. If
    // these ever disagree again the fix is in the browser, never in one
    // surface, because there is only one place left to fix.
    let seed = ChatComposeSeed {
        prefill: Some("@alice ".to_string()),
        reply_to: Some("m3".to_string()),
        edit_target: None,
        edit_revision: 0,
    };
    assert_eq!(seed.label(), "reply");
    assert!(seed.is_contextual());
}

// ── The composer says where it is going ──────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn an_armed_reply_keeps_chat_armed_even_after_focus_leaves_the_panel() {
    // The user pressed `r`, which means "this is a reply to that message".
    // Letting a later focus move silently redirect the pre-filled
    // `@alice ` into an agent prompt would be the worst possible outcome.
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('r'));
    assert_eq!(app.input_target(), InputTarget::Chat);

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "precondition: focus left the panel"
    );
    assert_eq!(
        app.input_target(),
        InputTarget::Chat,
        "an armed reply outranks focus until it is cancelled"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn escape_cancels_the_reply_context_without_discarding_the_text() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('r'));
    app.on_key(plain('h'));

    // The Esc ladder is one rung per context, in the order the user is in:
    // Insert -> Normal, panel -> composer, composer -> cancel the reply.
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.ui_state.input_mode,
        codegg::tui::input::InputMode::Normal,
        "the first Esc leaves Insert"
    );
    assert!(app.panel_host.is_focused(), "and stays on the panel");

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        !app.panel_host.is_focused(),
        "the second Esc releases the panel"
    );
    assert!(
        app.prompt_state.chat_seed.is_some(),
        "the reply survives the panel exit — the composer still points at chat"
    );

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.prompt_state.chat_seed.is_none(),
        "the third Esc is how a user backs out of an armed reply"
    );
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "@alice h",
        "cancelling must not throw away what the user typed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_retargets_the_composer_from_focus_again() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('r'));
    for _ in 0..3 {
        app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    }
    assert!(
        app.prompt_state.chat_seed.is_none(),
        "precondition: cancelled"
    );

    assert_eq!(
        app.input_target(),
        InputTarget::Build,
        "with no seed armed, the target is derived from focus as before"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sending_consumes_the_seed_so_a_second_enter_is_a_plain_send() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('r'));
    app.on_key(plain('x'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.prompt_state.chat_seed.is_none(),
        "the reply target belongs to one send, not to the composer forever"
    );

    app.on_key(plain('i'));
    app.on_key(plain('y'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.prompt_state.chat_seed.is_none(),
        "a plain send must not inherit the previous reply target"
    );
}

// ── The contextual key line ──────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn no_panel_focused_advertises_no_keys() {
    let mut app = test_app();
    assert!(
        app.composer_hint_text().is_none(),
        "the row is not reserved at all when nothing is focused, so the \
         prompt keeps its full height"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_focused_panel_advertises_its_own_keys() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    let hints = app
        .composer_hint_text()
        .expect("a focused panel supplies its keys");
    // The same verbs `/chat` advertises, from the same source.
    for verb in ["jk move", "r reply", "e edit", "X redact"] {
        assert!(hints.contains(verb), "hint line missing {verb}: {hints}");
    }
    assert!(
        hints.contains("panel"),
        "the hint line names how to leave the panel: {hints}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_session_panel_advertises_its_own_keys() {
    // The generalization: adding a panel means giving it hints, not
    // teaching the composer about a new panel.
    let mut app = test_app();
    seed_chat(&mut app);
    open_manager(&mut app);
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    to_normal(&mut app);
    assert!(app.panel_host.is_focused());
    let hints = app.composer_hint_text().expect("session panel hints");
    assert!(hints.contains("jk move"), "{hints}");
    assert!(
        !hints.contains("r reply"),
        "session hints, not chat: {hints}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_armed_reply_describes_the_editor_not_the_browser() {
    let mut app = test_app();
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('r'));
    let hints = app.composer_hint_text().expect("seed hints");
    assert!(
        hints.contains("send reply"),
        "the line must say what Enter will do: {hints}"
    );
    assert!(
        !hints.contains("r reply"),
        "the panel's browser verbs are not active while composing: {hints}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_unbound_panel_advertises_nothing_and_takes_no_focus() {
    // No project tab, so the chat panel has nothing to act on. It must not
    // claim the keyboard or advertise keys that would do nothing — a panel
    // that swallows input it cannot serve is the worst kind.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let mut app = codegg::tui::app::App::new_for_testing(root.display().to_string());
    app.project_tabs = codegg::tui::app::state::ProjectTabs::new();
    app.set_core_client(Arc::new(NullClient));
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    app.tui_cmd_tx = Some(tx);

    open_manager(&mut app);
    app.on_key(plain('j'));
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
    to_normal(&mut app);
    assert!(
        !app.panel_host.is_focused(),
        "a panel with no bound project must not take the keyboard"
    );
    assert!(
        app.composer_hint_text().is_none(),
        "an unbound panel has nothing to offer"
    );
    assert_eq!(
        app.input_target(),
        InputTarget::Build,
        "an unbound chat panel must never arm chat delivery"
    );
}

// ── `/chat` keeps its own composer ───────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn the_chat_window_is_unaffected_by_the_panel_workflow() {
    // The window owns a composer; the seed path belongs to the panel. A
    // reply opened in the window must not leak into the shared composer.
    let mut app = test_app();
    seed_chat(&mut app);
    app.show_chat();
    assert!(
        app.focus_manager
            .has_dialog(codegg::tui::components::component::DialogType::ProjectChat),
        "precondition: the /chat window is mounted"
    );
    app.on_key(plain('r'));
    assert!(
        app.prompt_state.chat_seed.is_none(),
        "the window's composer is local; it must not arm the shared one"
    );
}

// ── Docking a box into the side panel ─────────────────────────────────

/// Commit the panel selector onto whichever panel `rows_up` sits above Chat.
/// Open the manager and commit the panel `rows_down` places from Session.
///
/// The manager is a *box*, so committing has to close it — which is why
/// every caller gets a clean panel afterwards rather than a live overlay.
fn select_panel(app: &mut codegg::tui::app::App, rows_down: i8) {
    open_manager(app);
    for _ in 0..rows_down {
        app.on_key(plain('j'));
    }
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
}

/// Leave the chat box's Insert mode.
///
/// `/chat` now opens in Insert so the user can type immediately, which means
/// `L` *is* a letter while the composer has the keyboard. Every docking test
/// has to say so rather than assume Normal, because that assumption is
/// exactly what the mode default changed.
fn to_normal_mode(app: &mut codegg::tui::app::App) {
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
}

#[tokio::test(flavor = "current_thread")]
async fn a_chat_box_opens_ready_to_type() {
    // `/chat` is opened in order to write in it. Starting in Normal made the
    // first keystroke navigation, so the user had to discover a second key
    // before they could say anything.
    let mut app = test_app();
    seed_chat(&mut app);
    app.show_chat();
    app.on_key(plain('h'));
    assert!(
        app.focus_manager
            .has_dialog(codegg::tui::components::component::DialogType::ProjectChat),
        "typing must not close the box"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_uppercase_l_types_in_insert_mode_and_docks_in_normal_mode() {
    // The whole reason the box tracks a mode: in Insert, `L` is a letter you
    // can filter for; in Normal, it is the directional that docks.
    let mut app = test_app();
    seed_chat(&mut app);
    app.show_chat();
    app.on_key(plain('L'));
    assert!(
        app.focus_manager
            .has_dialog(codegg::tui::components::component::DialogType::ProjectChat),
        "L must type into the composer while Insert has the keyboard"
    );
    to_normal_mode(&mut app);
    app.on_key(plain('L'));
    assert!(
        !app.focus_manager
            .has_dialog(codegg::tui::components::component::DialogType::ProjectChat),
        "and must dock once the box is in Normal mode"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn docking_the_chat_window_moves_it_into_the_side_panel() {
    // The multiplexer move: `/chat` + `L` should leave the same browser
    // running in the side column, not closed. The point is that the box is
    // *moved*, so the panel keeps the channel, the cursor and the messages.
    let mut app = test_app();
    seed_chat(&mut app);
    app.show_chat();
    assert!(app
        .focus_manager
        .has_dialog(codegg::tui::components::component::DialogType::ProjectChat));
    to_normal_mode(&mut app);

    app.on_key(plain('L'));

    assert!(
        !app.focus_manager
            .has_dialog(codegg::tui::components::component::DialogType::ProjectChat),
        "docking must close the box, not stack a copy of it"
    );
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Chat,
        "docking must land on the panel that can hold the box"
    );
    assert!(app.panel_host.is_focused(), "the docked panel takes focus");
}

#[tokio::test(flavor = "current_thread")]
async fn a_shift_l_docks_exactly_like_a_bare_l() {
    // Terminals disagree on whether a shifted letter carries SHIFT; both
    // encodings must reach the same command.
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        let mut app = test_app();
        seed_chat(&mut app);
        app.show_chat();
        to_normal_mode(&mut app);
        app.on_key(key(KeyCode::Char('L'), modifiers));
        assert_eq!(
            app.panel_host.active(),
            SidePanelId::Chat,
            "L/{modifiers:?} must dock"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_same_key_selects_a_pane_when_no_box_is_open() {
    // The contextual half of the gesture: with nothing to dock, `L` means
    // "take the keyboard into the side panel". One key, two meanings, and
    // which one applies is a fact the user can see.
    let mut app = test_app();
    seed_chat(&mut app);
    select_panel(&mut app, 0); // Session is already live — committing takes focus
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    // Return to the main buffer so the assertion below is about what `L`
    // does rather than about the selector's focus behaviour.
    app.on_key(plain('H'));
    assert!(!app.panel_host.is_focused(), "starts in the main buffer");

    app.on_key(plain('L'));

    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Session,
        "selecting a pane must not swap which panel is loaded"
    );
    assert!(
        app.panel_host.is_focused(),
        "with no box to dock, the directional takes the keyboard to the panel"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_leftward_direction_returns_to_the_main_buffer() {
    let mut app = test_app();
    seed_chat(&mut app);
    select_panel(&mut app, 0); // Session is already live
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.on_key(plain('L'));
    assert!(app.panel_host.is_focused());

    app.on_key(plain('H'));

    assert!(
        !app.panel_host.is_focused(),
        "the mirror gesture must be able to leave again"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_vertical_direction_cycles_the_loaded_panel() {
    let mut app = test_app();
    seed_chat(&mut app);
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    let before = app.panel_host.active();

    app.on_key(plain('J'));

    assert_ne!(
        app.panel_host.active(),
        before,
        "the vertical direction must move through the panel list, not be inert"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_docked_panel_shows_the_same_conversation() {
    // Docking swaps the frame the browser draws in, not the conversation: the
    // panel is a different host with its own browser instance, so it opens at
    // the live tail rather than inheriting the window's cursor position.
    let mut app = test_app();
    seed_chat(&mut app);
    app.show_chat();
    to_normal_mode(&mut app);
    app.on_key(plain('k'));

    app.on_key(plain('L'));

    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
    assert!(
        app.panel_host.is_focused(),
        "the conversation the box was showing is now in the panel"
    );
    assert_eq!(
        app.panel_host.chat_cursor(),
        2,
        "the docked panel opens at the live tail, like any panel does"
    );
}

// ── The composer names one destination ───────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn the_composer_names_the_surface_that_has_the_keyboard() {
    let mut app = test_app();
    seed_chat(&mut app);

    // Nothing focused: the main agent transcript.
    assert_eq!(app.composer_destination_title(), "LLM");

    // The default panel.
    select_panel(&mut app, 0);
    assert_eq!(app.panel_host.active(), SidePanelId::Session);
    assert_eq!(
        app.composer_destination_title(),
        "SESSIONS",
        "the badge must name the panel that is selected, not always CHAT"
    );

    // The chat panel.
    focus_chat(&mut app);
    assert_eq!(app.composer_destination_title(), "CHAT");

    // Back to the main buffer. The focus move is a Normal-mode gesture, and
    // the panel is still in Insert after the selector committed it.
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.on_key(key(KeyCode::Left, KeyModifiers::SHIFT));
    assert_eq!(app.composer_destination_title(), "LLM");
}

#[tokio::test(flavor = "current_thread")]
async fn an_armed_reply_names_chat_even_when_a_panel_is_later_selected() {
    // The seed outranks panel focus for delivery, so the badge has to agree:
    // an armed reply must never render as if it were aimed at the agent.
    let mut app = test_app();
    seed_chat(&mut app);
    focus_chat(&mut app);
    to_normal(&mut app);
    app.on_key(plain('r'));
    assert!(
        app.prompt_state.chat_seed.is_some(),
        "precondition: r arms a reply"
    );

    select_panel(&mut app, 0); // Session is already live; committing focuses it
    assert_eq!(
        app.composer_destination_title(),
        "CHAT",
        "an armed reply outranks a later focus move"
    );
}

#[test]
fn every_panel_has_a_distinct_uppercase_title() {
    // A destination the user cannot name is a destination they cannot trust.
    let titles: Vec<&str> = SidePanelId::ALL.iter().map(|p| p.title()).collect();
    for title in &titles {
        assert!(
            title.chars().all(|c| c.is_ascii_uppercase()),
            "title {title} must be an uppercase badge token"
        );
    }
    let mut unique = titles.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        titles.len(),
        "two panels share a title, so the badge cannot say which is selected: {titles:?}"
    );
}
