//! M003: the chat and change-review row models, counted.
//!
//! `tui_frame_cost.inc` measures the per-frame cost M001 removed from the
//! frame composition; `tui_file_tree_cost.inc` measures the file tree's row
//! model. This member measures the two surfaces whose cost scaled with
//! **retained history** rather than with the viewport:
//!
//! * the project chat — the docked panel and the `/chat` box — wraps every
//!   retained message body on every draw, because its display cap is applied
//!   by the command-line projection and not by the renderer;
//! * the change review — every frame deep-cloned its whole parsed patch, which
//!   is bounded only by `MAX_REVIEW_PATCH_BYTES`.
//!
//! Four counters do the accounting:
//!
//! * `chat_row_builds` — one per miss of a surface's row cache, which is one
//!   full re-wrap of the channel;
//! * `chat_wrap_passes` — one per non-redacted body a build wraps, so a build
//!   of `n` messages is `1` row build and `n` wrap passes;
//! * `chat_unread_scans` — one per scan of the message window; the count is
//!   memoized on the window and the read marker;
//! * `review_hunk_copies` — one per parsed hunk copied.
//!
//! The **cost** tests are written against the *after* state: on the baseline
//! they fail, and the failure output is the before-count — every frame paid
//! one build, one wrap per retained body, one unread scan and one whole-patch
//! copy. The equivalence tests pass on the baseline too, which is what makes
//! them guard rails rather than evidence.

use std::sync::Arc;

use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    ChatChannelDto, ChatMessageDto, CoreEvent, CoreRequest, CoreResponse, EventEnvelope,
    RequestEnvelope,
};
use codegg::tui::app::App;
use codegg::tui::components::component::DialogType;
use codegg::tui::panels::SidePanelId;
use codegg::tui::render_cost::{self, RenderCost};
use codegg::tui::unified_diff::MAX_REVIEW_PATCH_BYTES;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

const PROJECT: &str = "project-a";
const OTHER_PROJECT: &str = "project-b";

// ── Fake core ────────────────────────────────────────────────────────────

struct NullClient;

#[async_trait::async_trait]
impl CoreClient for NullClient {
    async fn request(
        &self,
        _request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        Err(AppError::Tui("no daemon in chat cost tests".to_string()))
    }

    fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        rx
    }
}

// ── Fixtures ─────────────────────────────────────────────────────────────

/// The workspace root every app in this member shares.
///
/// Deliberately **not** a fresh tempdir per app: the footer draws the active
/// workspace root, and two apps with different roots can never produce equal
/// buffers — which is exactly the comparison
/// `a_warm_panel_draws_what_a_cold_app_draws_after_a_redaction` makes.
fn workspace_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join("codegg-m003-chat-cost");
    std::fs::create_dir_all(&root).expect("workspace root");
    root
}

fn test_app() -> App {
    let root = workspace_root();

    let mut app = App::new_for_testing(root.display().to_string());
    app.project_tabs = codegg::tui::app::state::ProjectTabs::new();
    for (name, project) in [("Alpha", PROJECT), ("Beta", OTHER_PROJECT)] {
        let mut tab = codegg::tui::app::state::ProjectTabState::empty(
            codegg::tui::app::state::ProjectTabId::new(),
            name.to_string(),
        );
        tab.project_id = Some(project.to_string());
        tab.workspace_id = Some(format!("workspace-{project}"));
        tab.workspace_root = Some(root.clone());
        app.project_tabs.add_tab(tab);
    }
    let first = app.project_tabs.ordered()[0].tab_id.clone();
    app.project_tabs.set_active(&first);

    app.set_core_client(Arc::new(NullClient));
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    app.tui_cmd_tx = Some(tx);
    app
}

fn chat_message(project: &str, channel: &str, seq: u64, body: &str) -> ChatMessageDto {
    ChatMessageDto {
        message_id: format!("{channel}-m{seq}"),
        channel_id: channel.to_string(),
        project_id: project.to_string(),
        seq,
        author_principal: "alice".to_string(),
        author_agent: None,
        body: body.to_string(),
        reply_to: None,
        thread_root: None,
        mentions: Vec::new(),
        references: Vec::new(),
        revision: 1,
        redacted: false,
        created_at_ms: 100 + seq as i64,
        edited_at_ms: None,
    }
}

fn channel(project: &str, channel_id: &str) -> ChatChannelDto {
    ChatChannelDto {
        channel_id: channel_id.to_string(),
        project_id: project.to_string(),
        name: channel_id.to_string(),
        created_by: "alice".to_string(),
        created_at_ms: 1,
    }
}

/// Ensure `channel_id` exists and is the active channel.
fn ensure_channel(app: &mut App, project: &str, channel_id: &str) {
    let epoch = app.chat.reconnect_epoch;
    let request = app.chat.begin_ensure(project).expect("begin_ensure");
    assert!(
        app.chat
            .apply_channel_ensured(request, project, &channel(project, channel_id), epoch),
        "the channel must apply"
    );
}

/// Deliver a history page for `channel_id`; this also makes it active.
fn deliver_history(app: &mut App, project: &str, channel_id: &str, bodies: &[String]) {
    let epoch = app.chat.reconnect_epoch;
    let messages: Vec<ChatMessageDto> = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| chat_message(project, channel_id, index as u64 + 1, body.as_str()))
        .collect();
    let request = app
        .chat
        .begin_history(project, channel_id)
        .expect("begin_history");
    assert!(
        app.chat
            .apply_history(request, project, channel_id, messages, 0, false, 0, epoch),
        "the page must apply"
    );
}

/// A body of approximately `bytes` bytes, distinctive per index.
fn body_of(index: usize, bytes: usize) -> String {
    let mut text = format!("message {index} ");
    while text.len() < bytes {
        text.push_str("lorem ipsum dolor sit amet ");
    }
    text.truncate(bytes);
    text
}

fn seed_messages(app: &mut App, project: &str, count: usize, body_bytes: usize) {
    ensure_channel(app, project, "ch1");
    let bodies: Vec<String> = (0..count).map(|index| body_of(index, body_bytes)).collect();
    deliver_history(app, project, "ch1", &bodies);
}

/// Dock and focus the chat panel the way a user does.
fn focus_chat(app: &mut App) {
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.on_key(plain('S'));
    app.on_key(plain('j'));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_host.active(), SidePanelId::Chat);
    assert!(app.panel_host.is_focused(), "precondition: focused panel");
}

fn plain(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

// ── Measurement ──────────────────────────────────────────────────────────

fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("the frame draws");
    terminal.backend().buffer().clone()
}

/// One drawn frame's operation counts.
fn frame_cost(app: &mut App) -> RenderCost {
    let before = render_cost::snapshot();
    let _buffer = render(app, 120, 40);
    render_cost::snapshot().since(&before)
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── The docked panel: one build per content change ───────────────────────

/// The property the milestone exists for: a draw that applied nothing wraps
/// nothing.
#[test]
fn a_chat_panel_frame_that_changed_nothing_rebuilds_and_wraps_nothing() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    // A known read marker, so the unread count is actually derived: with an
    // unknown marker the reducer fails closed at zero and the scan would not
    // be exercised at all.
    app.chat.apply_read_marker(PROJECT, "ch1", 5);
    focus_chat(&mut app);
    let _warm = render(&mut app, 120, 40);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.chat_row_builds, 0,
        "the panel rebuilt its row model on a frame where nothing changed"
    );
    assert_eq!(
        cost.chat_wrap_passes, 0,
        "the panel wrapped every retained body on a frame where nothing changed"
    );
    assert_eq!(
        cost.chat_unread_scans, 0,
        "the panel scanned the message window on a frame where nothing changed"
    );
}

/// The cold frame is the baseline: one build and one wrap per retained body.
/// On the pre-change code **every** frame paid exactly this.
#[test]
fn the_cold_panel_frame_is_one_build_and_one_wrap_per_message() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    focus_chat(&mut app);

    let cold = frame_cost(&mut app);
    assert_eq!(cold.chat_row_builds, 1, "the first frame must build once");
    assert_eq!(
        cold.chat_wrap_passes, 10,
        "a build wraps one body per retained message"
    );

    let warm = frame_cost(&mut app);
    assert_eq!(warm.chat_row_builds, 0, "the second frame reads the cache");
    assert_eq!(warm.chat_wrap_passes, 0, "and wraps nothing");
}

/// A message that arrives rebuilds once and wraps the retained window.
#[test]
fn a_new_message_rebuilds_the_panel_once() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    focus_chat(&mut app);
    let _warm = render(&mut app, 120, 40);

    let epoch = app.chat.reconnect_epoch;
    let request = app.chat.begin_history(PROJECT, "ch1").expect("begin");
    assert!(app.chat.apply_history(
        request,
        PROJECT,
        "ch1",
        (1..=11)
            .map(|seq| chat_message(PROJECT, "ch1", seq, &body_of(seq as usize, 200)))
            .collect(),
        0,
        false,
        0,
        epoch
    ));

    let cost = frame_cost(&mut app);
    assert_eq!(cost.chat_row_builds, 1, "an applied page rebuilds once");
    assert_eq!(
        cost.chat_wrap_passes, 11,
        "the rebuild covers the retained window, which is the coarse key's cost"
    );
}

/// A read-marker advance moves the unread divider; the next frame rebuilds.
#[test]
fn a_read_marker_advance_rebuilds_the_panel() {
    let mut app = test_app();
    // Short bodies so the whole ten-message window and its divider fit the
    // panel's viewport: this test is about the marker, not about scrolling.
    seed_messages(&mut app, PROJECT, 10, 24);
    focus_chat(&mut app);
    app.chat.apply_read_marker(PROJECT, "ch1", 6);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(
        before.contains("4 unread"),
        "the premise: the divider names the count, got {before:?}"
    );

    app.chat.apply_read_marker(PROJECT, "ch1", 10);
    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.chat_row_builds, 1,
        "the marker moved the divider and the sequence did not"
    );
    let after = buffer_text(&render(&mut app, 120, 40));
    assert!(
        !after.contains("unread"),
        "the divider must be gone once the marker reached the tail: {after:?}"
    );
}

/// A column resize re-wraps; drawing at the same width again does not.
#[test]
fn a_width_change_rebuilds_the_panel_and_the_same_width_does_not() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    focus_chat(&mut app);
    let _warm = render(&mut app, 120, 40);

    let same = frame_cost(&mut app);
    assert_eq!(same.chat_row_builds, 0, "the same width must not rebuild");

    // A drag on the docked column: the body column narrows, so the bodies
    // re-wrap. The frame size is unchanged, which is what makes this a width
    // change and not a resize of everything.
    let before = render_cost::snapshot();
    app.ui_state.layout.config.left_panel_width = Some(40);
    app.ui_state.layout.config.right_panel_width = Some(40);
    let _narrower = render(&mut app, 120, 40);
    let resized = render_cost::snapshot().since(&before);
    assert_eq!(
        resized.chat_row_builds, 1,
        "a narrower body column re-wraps the rows"
    );
}

/// The cursor styles the selected message's rows, so a move rebuilds.
#[test]
fn a_cursor_move_rebuilds_the_panel_rows() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    focus_chat(&mut app);
    let _warm = render(&mut app, 120, 40);

    app.on_key(plain('k'));
    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.chat_row_builds, 1,
        "the selection marker moved to another message"
    );
}

/// A redaction that arrives while the panel is open reaches the **next
/// frame**: the reducer moves its sequence, so the cache cannot serve the
/// pre-redaction body. This is the milestone's security property at the
/// frame, not just at the key.
#[test]
fn a_redaction_reaches_the_next_frame() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 3, 200);
    focus_chat(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(
        before.contains("message 2"),
        "the premise: the body is drawn, got {before:?}"
    );

    assert!(
        app.chat.apply_event_redacted(PROJECT, "ch1", "ch1-m3", 2),
        "the redaction must apply"
    );
    let cost = frame_cost(&mut app);
    let after = buffer_text(&render(&mut app, 120, 40));

    assert_eq!(
        cost.chat_row_builds, 1,
        "the redaction moved the sequence, so the rows must rebuild"
    );
    assert!(
        !after.contains("message 2"),
        "a redacted body was drawn from the pre-redaction rows: {after:?}"
    );
    assert!(
        after.contains("[redacted]"),
        "the redaction must be visible on the next frame: {after:?}"
    );
}

/// An edit (`revision` bumped, same `message_id`) reaches the next frame.
#[test]
fn an_edit_reaches_the_next_frame() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 3, 200);
    focus_chat(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(
        before.contains("message 2"),
        "the premise: the old body is drawn, got {before:?}"
    );

    let mut edited = chat_message(PROJECT, "ch1", 3, "revised wording");
    edited.revision = 2;
    assert!(app.chat.apply_event_edited(&edited), "the edit must apply");
    let after = buffer_text(&render(&mut app, 120, 40));

    assert!(
        after.contains("revised wording"),
        "the edited body must reach the frame: {after:?}"
    );
    assert!(
        !after.contains("message 2"),
        "the replaced body was still drawn: {after:?}"
    );
    // The `(r2)` marker is asserted on the row model, not here: it is pushed
    // after the selected row's full-width padding, so at this panel width it
    // falls outside the drawn columns — unchanged behaviour, out of scope.
}

// ── The `/chat` box: the same property, its own cache ────────────────────

#[test]
fn a_chat_box_frame_that_changed_nothing_rebuilds_and_wraps_nothing() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    app.show_chat();
    assert!(
        app.focus_manager.has_dialog(DialogType::ProjectChat),
        "the box is mounted"
    );
    let _warm = render(&mut app, 120, 40);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.chat_row_builds, 0,
        "the box rebuilt its row model on a frame where nothing changed"
    );
    assert_eq!(
        cost.chat_wrap_passes, 0,
        "the box wrapped every retained body on a frame where nothing changed"
    );
}

/// A snapshot pushed into the box is its content-changed signal.
#[test]
fn a_chat_box_snapshot_rebuilds_the_rows_once() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 10, 200);
    app.show_chat();
    let _warm = render(&mut app, 120, 40);

    // A fresh snapshot is pushed into the mounted box — the production path
    // `refresh_chat_panel` takes after every completion and live event.
    app.show_chat();

    let cost = frame_cost(&mut app);
    assert_eq!(cost.chat_row_builds, 1, "an applied snapshot rebuilds once");
    assert_eq!(cost.chat_wrap_passes, 10, "and wraps the retained window");
}

// ── Scale: the cost is one build per change, not per retained message ────

/// The counter values the report quotes: 1 / 10 / 100 retained messages, with
/// a ~200-byte and a ~8 KiB body, on both surfaces. An unchanged frame costs
/// zero at every scale; a rebuild costs one build and one wrap per message.
#[test]
fn the_chat_cost_is_one_build_per_change_at_every_scale() {
    for count in [1usize, 10, 100] {
        for bytes in [200usize, 8 * 1024] {
            let mut app = test_app();
            seed_messages(&mut app, PROJECT, count, bytes);
            focus_chat(&mut app);

            let cold = frame_cost(&mut app);
            assert_eq!(
                cold.chat_row_builds, 1,
                "{count} messages of {bytes} bytes: the first frame builds once"
            );
            assert_eq!(
                cold.chat_wrap_passes, count as u64,
                "{count} messages of {bytes} bytes: one wrap per body"
            );

            let warm = frame_cost(&mut app);
            assert_eq!(
                warm.chat_row_builds, 0,
                "{count} messages of {bytes} bytes: an unchanged frame rebuilt"
            );
            assert_eq!(
                warm.chat_wrap_passes, 0,
                "{count} messages of {bytes} bytes: an unchanged frame wrapped"
            );
            assert_eq!(
                warm.chat_unread_scans, 0,
                "{count} messages of {bytes} bytes: an unchanged frame scanned"
            );

            // One more message: the window keeps its 100-message bound.
            let bodies: Vec<String> = (0..count + 1).map(|index| body_of(index, bytes)).collect();
            deliver_history(&mut app, PROJECT, "ch1", &bodies);
            let rebuilt = frame_cost(&mut app);
            let retained = (count + 1).min(100) as u64;
            assert_eq!(
                rebuilt.chat_row_builds, 1,
                "{count} messages of {bytes} bytes: an arrival rebuilds once"
            );
            assert_eq!(
                rebuilt.chat_wrap_passes, retained,
                "{count} messages of {bytes} bytes: the rebuild wraps the retained window"
            );
        }
    }
}

/// The same scale sweep for the `/chat` box, which has its own browser and
/// therefore its own cache.
#[test]
fn the_chat_box_cost_is_one_build_per_change_at_every_scale() {
    for count in [1usize, 10, 100] {
        for bytes in [200usize, 8 * 1024] {
            let mut app = test_app();
            seed_messages(&mut app, PROJECT, count, bytes);
            app.show_chat();

            let cold = frame_cost(&mut app);
            assert_eq!(
                cold.chat_row_builds, 1,
                "box, {count} messages of {bytes} bytes: the first frame builds once"
            );
            assert_eq!(
                cold.chat_wrap_passes, count as u64,
                "box, {count} messages of {bytes} bytes: one wrap per body"
            );

            let warm = frame_cost(&mut app);
            assert_eq!(
                warm.chat_row_builds, 0,
                "box, {count} messages of {bytes} bytes: an unchanged frame rebuilt"
            );
            assert_eq!(
                warm.chat_wrap_passes, 0,
                "box, {count} messages of {bytes} bytes: an unchanged frame wrapped"
            );
        }
    }
}

// ── Cancellation: a channel, project or session change drops the cache ───

/// A channel switch drops the cached rows: the next frame rebuilds and
/// draws only the new channel's bodies.
#[test]
fn a_channel_switch_drops_the_panel_rows() {
    let mut app = test_app();
    ensure_channel(&mut app, PROJECT, "ch1");
    ensure_channel(&mut app, PROJECT, "ch2");
    deliver_history(
        &mut app,
        PROJECT,
        "ch1",
        &[body_of(0, 200), body_of(1, 200)],
    );
    focus_chat(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(
        before.contains("message 0"),
        "the premise: channel one is drawn, got {before:?}"
    );

    deliver_history(&mut app, PROJECT, "ch2", &["channel two body".to_string()]);
    let cost = frame_cost(&mut app);
    let after = buffer_text(&render(&mut app, 120, 40));

    assert_eq!(cost.chat_row_builds, 1, "a channel switch rebuilt the rows");
    assert!(
        !after.contains("message 0"),
        "the previous channel's rows were drawn after the switch: {after:?}"
    );
    assert!(
        after.contains("channel two body"),
        "the new channel's rows must be drawn: {after:?}"
    );
}

/// A project switch drops the cached rows: the panel is rebound to another
/// project and the next frame rebuilds with that project's window.
#[test]
fn a_project_switch_drops_the_panel_rows() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 3, 200);
    focus_chat(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(
        before.contains("message 0"),
        "the premise: project A is drawn, got {before:?}"
    );

    // The binding change `sync_chat_panel_project` performs: the panel's
    // per-view state is dropped before the new project is bound.
    app.chat_panel_project = Some(OTHER_PROJECT.to_string());
    app.panel_host.chat_panel_mut().reset();
    app.panel_host.chat_panel_mut().set_bound(true);
    ensure_channel(&mut app, OTHER_PROJECT, "ch1");
    deliver_history(
        &mut app,
        OTHER_PROJECT,
        "ch1",
        &["project b body".to_string()],
    );

    let cost = frame_cost(&mut app);
    let after = buffer_text(&render(&mut app, 120, 40));

    assert_eq!(cost.chat_row_builds, 1, "a project switch rebuilt the rows");
    assert!(
        !after.contains("message 0"),
        "the previous project's rows were drawn after the switch: {after:?}"
    );
    assert!(
        after.contains("project b body"),
        "the new project's rows must be drawn: {after:?}"
    );
}

/// A session teardown clears the projection and unbinds the panel: nothing of
/// the old session is drawn, and a later binding starts from a build.
#[test]
fn a_session_teardown_drops_the_panel_rows() {
    let mut app = test_app();
    seed_messages(&mut app, PROJECT, 3, 200);
    focus_chat(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(
        before.contains("message 0"),
        "the premise: the session is drawn, got {before:?}"
    );

    // `reset_chat_panel` + `clear_all`, the teardown paths.
    app.chat.clear_all();
    app.chat_panel_project = None;
    app.panel_host.chat_panel_mut().reset();
    app.panel_host.chat_panel_mut().set_bound(false);

    let torn_down = buffer_text(&render(&mut app, 120, 40));
    assert!(
        !torn_down.contains("message 0"),
        "a torn-down session still drew its rows: {torn_down:?}"
    );

    // A binding after the teardown rebuilds rather than serving the old rows.
    app.chat_panel_project = Some(PROJECT.to_string());
    app.panel_host.chat_panel_mut().set_bound(true);
    seed_messages(&mut app, PROJECT, 2, 200);
    let cost = frame_cost(&mut app);
    assert_eq!(
        cost.chat_row_builds, 1,
        "a rebind after a teardown must rebuild"
    );
}

// ── The change review: zero hunk copies per frame ────────────────────────

/// A review patch of `hunks` hunks, each with one context line and
/// `lines_per_hunk` added lines.
fn patch_of(hunks: usize, lines_per_hunk: usize) -> String {
    let mut text = String::new();
    for hunk in 0..hunks {
        let start = hunk * 10 + 1;
        text.push_str(&format!("@@ -{start},1 +{start},1 @@\n context\n"));
        for line in 0..lines_per_hunk {
            text.push_str(&format!("+line {line}\n"));
        }
    }
    text
}

fn open_review(app: &mut App, patch: &str) {
    let patches = vec![("src/lib.rs".to_string(), patch.to_string())];
    app.change_review_state.open(
        "preview-1".to_string(),
        "edit".to_string(),
        "Rename helper".to_string(),
        "agent".to_string(),
        false,
        codegg::tui::unified_diff::parse_review(&patches),
        vec!["src/lib.rs".to_string()],
    );
    assert!(
        app.change_review_state.is_open(),
        "precondition: the review is open"
    );
}

/// A review frame copies no hunks — at a small patch and at the 512 KiB
/// bound, which used to be cloned whole on every frame.
#[test]
fn a_review_frame_copies_no_hunks() {
    let at_bound = patch_of(256, 230);
    assert!(
        at_bound.len() >= MAX_REVIEW_PATCH_BYTES,
        "the fixture must reach the review's patch bound, got {} bytes",
        at_bound.len()
    );
    for (label, patch) in [("small", patch_of(2, 20)), ("at the bound", at_bound)] {
        let mut app = test_app();
        open_review(&mut app, &patch);
        let _warm = render(&mut app, 120, 40);

        let cost = frame_cost(&mut app);

        assert_eq!(
            cost.review_hunk_copies, 0,
            "the {label} review copied its parsed patch on a frame"
        );
    }
}

/// The counter is produced by the copy itself: a whole-patch clone moves it.
#[test]
fn a_review_that_cloned_its_patch_would_move_the_counter() {
    let mut app = test_app();
    open_review(&mut app, &patch_of(2, 20));
    let patch = app
        .change_review_state
        .active_patch()
        .expect("the premise: a patch is active");
    let before = render_cost::snapshot();
    let _copy = patch.hunks.clone();
    let moved = render_cost::snapshot().since(&before);
    assert!(
        moved.review_hunk_copies >= 1,
        "cloning the parsed patch must move review_hunk_copies, got {}",
        moved.review_hunk_copies
    );
}

// ── The rendered output is unchanged ─────────────────────────────────────

/// One chat fixture: the app state a frame draws.
struct Fixture {
    name: &'static str,
    bodies: Vec<String>,
    read_marker: Option<u64>,
    truncated: bool,
    cursor_up: usize,
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "empty",
            bodies: Vec::new(),
            read_marker: None,
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "one message",
            bodies: vec!["hello".to_string()],
            read_marker: None,
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "wrapping",
            bodies: vec!["word ".repeat(60)],
            read_marker: None,
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "multi-byte",
            bodies: vec!["héllo 日本語 wörld".to_string()],
            read_marker: None,
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "escape sequence",
            bodies: vec!["before \u{1b}[31mcoloured\u{1b}[0m after".to_string()],
            read_marker: None,
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "redacted",
            bodies: vec!["first".to_string(), "withdrawn".to_string()],
            read_marker: None,
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "unread at the first message",
            bodies: vec!["one".to_string(), "two".to_string(), "three".to_string()],
            read_marker: Some(0),
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "unread at the last message",
            bodies: vec!["one".to_string(), "two".to_string(), "three".to_string()],
            read_marker: Some(2),
            truncated: false,
            cursor_up: 0,
        },
        Fixture {
            name: "selected away from the tail",
            bodies: vec!["one".to_string(), "two".to_string(), "three".to_string()],
            read_marker: None,
            truncated: false,
            cursor_up: 2,
        },
        Fixture {
            name: "truncated history header",
            bodies: vec!["one".to_string(), "two".to_string()],
            read_marker: None,
            truncated: true,
            cursor_up: 0,
        },
    ]
}

fn app_for(fixture: &Fixture) -> App {
    let mut app = test_app();
    ensure_channel(&mut app, PROJECT, "ch1");
    deliver_history(&mut app, PROJECT, "ch1", &fixture.bodies);
    if fixture.name == "redacted" {
        let epoch = app.chat.reconnect_epoch;
        let request = app.chat.begin_history(PROJECT, "ch1").expect("begin");
        let mut messages: Vec<ChatMessageDto> = fixture
            .bodies
            .iter()
            .enumerate()
            .map(|(index, body)| chat_message(PROJECT, "ch1", index as u64 + 1, body.as_str()))
            .collect();
        messages[1].redacted = true;
        assert!(app
            .chat
            .apply_history(request, PROJECT, "ch1", messages, 0, false, 0, epoch));
    }
    if let Some(marker) = fixture.read_marker {
        app.chat.apply_read_marker(PROJECT, "ch1", marker);
    }
    if let Some(entry) = app.chat.get_mut(PROJECT) {
        entry.history_truncated = fixture.truncated;
    }
    app
}

/// The warm frame is the cold frame, pixel for pixel, over the fixture set:
/// the cache changes the cost, not the picture.
#[test]
fn a_chat_panel_draws_the_same_buffer_warm_and_cold() {
    for fixture in fixtures() {
        let mut app = app_for(&fixture);
        focus_chat(&mut app);
        for _ in 0..fixture.cursor_up {
            app.on_key(plain('k'));
        }

        // The whole buffer: symbols **and** styles, so a cache that kept the
        // text and lost the palette fails the same assertion.
        let cold = render(&mut app, 120, 40);
        let warm = render(&mut app, 120, 40);
        assert_eq!(
            cold, warm,
            "the {:?} fixture's warm render differs from its cold one",
            fixture.name
        );
    }
}

/// A warm frame after a content change draws exactly what a **cold** app in
/// the same final state draws.
///
/// This is the equivalence the cache must not break, and — unlike the
/// warm-vs-cold pair above, which only catches a cache whose hits differ from
/// its own builds — it fails when the key misses the change: the warm frame
/// keeps the pre-redaction body while the cold app draws `[redacted]`.
#[test]
fn a_warm_panel_draws_what_a_cold_app_draws_after_a_redaction() {
    // The warm app: draw once, then redact through the live path, then draw.
    let mut warm_app = test_app();
    seed_messages(&mut warm_app, PROJECT, 3, 60);
    focus_chat(&mut warm_app);
    let before = buffer_text(&render(&mut warm_app, 120, 40));
    assert!(
        before.contains("message 1"),
        "the premise: the body is drawn, got {before:?}"
    );
    assert!(
        warm_app
            .chat
            .apply_event_redacted(PROJECT, "ch1", "ch1-m2", 2),
        "the redaction must apply"
    );
    let warm = render(&mut warm_app, 120, 40);

    // The cold app: the same final state, built before its first frame.
    let mut cold_app = test_app();
    ensure_channel(&mut cold_app, PROJECT, "ch1");
    let bodies: Vec<String> = (0..3).map(|index| body_of(index, 60)).collect();
    let epoch = cold_app.chat.reconnect_epoch;
    let request = cold_app
        .chat
        .begin_history(PROJECT, "ch1")
        .expect("begin_history");
    let mut messages: Vec<ChatMessageDto> = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| chat_message(PROJECT, "ch1", index as u64 + 1, body.as_str()))
        .collect();
    messages[1].redacted = true;
    messages[1].revision = 2;
    assert!(cold_app
        .chat
        .apply_history(request, PROJECT, "ch1", messages, 0, false, 0, epoch));
    focus_chat(&mut cold_app);
    let cold = render(&mut cold_app, 120, 40);

    assert_eq!(
        cold, warm,
        "the warm frame after a redaction differs from a cold app in the same state"
    );
}

/// The same equality for the `/chat` box, whose viewport and retention header
/// are its own.
#[test]
fn a_chat_box_draws_the_same_buffer_warm_and_cold() {
    for fixture in fixtures() {
        let mut app = app_for(&fixture);
        app.show_chat();
        for _ in 0..fixture.cursor_up {
            app.on_key(plain('k'));
        }

        let cold = render(&mut app, 120, 40);
        let warm = render(&mut app, 120, 40);
        assert_eq!(
            cold, warm,
            "the {:?} fixture's box render differs warm and cold",
            fixture.name
        );
    }
}

/// Both surfaces draw the same rows for the same window: one producer, not
/// two. Rendering the panel then the box must not change what either shows.
#[test]
fn the_panel_and_the_box_agree_about_the_rows() {
    let fixture = Fixture {
        name: "shared",
        bodies: vec![
            "alpha".to_string(),
            "beta beta beta beta beta beta beta beta beta beta".to_string(),
            "gamma".to_string(),
        ],
        read_marker: None,
        truncated: false,
        cursor_up: 0,
    };
    let mut panel_app = app_for(&fixture);
    focus_chat(&mut panel_app);
    let panel = buffer_text(&render(&mut panel_app, 120, 40));

    let mut box_app = app_for(&fixture);
    box_app.show_chat();
    let window = buffer_text(&render(&mut box_app, 120, 40));

    for body in ["alpha", "gamma", "beta"] {
        assert!(panel.contains(body), "the panel draws {body:?}: {panel:?}");
        assert!(window.contains(body), "the box draws {body:?}: {window:?}");
    }
}
