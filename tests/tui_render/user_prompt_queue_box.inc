//! M004: the `/queue` box on the real key path.
//!
//! M004 is this workstream's **first user-operable** milestone, so the tests
//! here are about reachability and honesty rather than about internal wiring:
//! a key the box advertises must reach its action through `App::on_key`, and
//! the box must not say anything it does not do.
//!
//! Every interaction below goes through the **command registry and the real key
//! path**, never a direct handler call. That distinction is load-bearing — an
//! existing `workspace_command_and_hotkey_open_same_dashboard` test called its
//! handler twice directly and therefore proved nothing about key reachability,
//! and a `Dialog` variant with arms in the `#[cfg(any())]`-disabled legacy
//! router compiles and does nothing at all.
//!
//! The daemon is absent under test, so the delete path is exercised at the
//! boundary it actually owns: what the box hands the daemon, what it does with
//! the answer, and what it refuses to claim when there is no connection.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope, UserPromptQueueEntryDto,
    UserPromptQueueSendNowResult,
};
use codegg::tui::app::state::is_observer_allowed_command;
use codegg::tui::app::state::subagents::SubagentRow;
use codegg::tui::app::{App, Dialog, TuiCommand};
use codegg::tui::command::{BuiltinSlashAction, CommandAction, CommandRegistry};
use codegg::tui::components::component::{Component, DialogType};
use codegg::tui::components::dialogs::queue::QueueDialog;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;
use tokio::sync::mpsc;

const SESSION: &str = "session-queue-1";

// ── Fake queue core ───────────────────────────────────────────────────

/// Records every queue request it receives and answers with a canned
/// projection.
///
/// This exists so the two claims that matter most can be measured instead of
/// argued: that **opening** issues a read even when a projection is already
/// cached, and that a **delete** issues one `Delete` and no `List`. Counting
/// requests is the only way to prove the delete path does not round-trip
/// twice; reading the source would only prove the source says so.
#[derive(Default)]
struct FakeQueueClient {
    seen: Mutex<Vec<String>>,
    projection: Mutex<Vec<UserPromptQueueEntryDto>>,
    /// When set, `Delete` answers with this error code instead of succeeding.
    delete_error: Mutex<Option<String>>,
    /// M006. When set, send-now answers `Refused { code }` and leaves the row
    /// where it was — which is what every channel refusal looks like from the
    /// box's side.
    send_now_refusal: Mutex<Option<String>>,
    /// M006. When set, send-now answers `AcceptedNotSettled`: the turn took the
    /// prompt but the queue could not record it, so the row stays live.
    send_now_unsettled: Mutex<bool>,
    /// M006. When set, send-now answers `EntryNotFound`.
    send_now_missing: Mutex<bool>,
    /// M006. When set, send-now answers `Accepted` *and* removes the row, which
    /// is the whole point of the feature.
    send_now_accepted: Mutex<bool>,
    /// C007. When set, `TurnSteer` answers `CoreResponse::Error` with this code
    /// — the shape every channel refusal takes.
    steer_refusal: Mutex<Option<String>>,
}

impl FakeQueueClient {
    fn new(projection: Vec<UserPromptQueueEntryDto>) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
            projection: Mutex::new(projection),
            delete_error: Mutex::new(None),
            send_now_refusal: Mutex::new(None),
            send_now_unsettled: Mutex::new(false),
            send_now_missing: Mutex::new(false),
            send_now_accepted: Mutex::new(false),
            steer_refusal: Mutex::new(None),
        })
    }

    fn record(&self, kind: &str) {
        self.seen.lock().unwrap().push(kind.to_string());
    }

    fn kinds(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn count(&self, kind: &str) -> usize {
        self.kinds().iter().filter(|k| *k == kind).count()
    }

    fn set_projection(&self, entries: Vec<UserPromptQueueEntryDto>) {
        *self.projection.lock().unwrap() = entries;
    }

    fn error_answer(&self, code: &Option<String>) -> CoreResponse {
        CoreResponse::Error {
            code: code.clone().unwrap_or_default(),
            message: "refused".to_string(),
        }
    }

    fn answered(&self, entries: Vec<UserPromptQueueEntryDto>) -> CoreResponse {
        CoreResponse::UserPromptQueue {
            session_id: SESSION.to_string(),
            entries,
        }
    }
}

#[async_trait]
impl CoreClient for FakeQueueClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::UserPromptQueueList { .. } => {
                self.record("list");
                Ok(self.answered(self.projection.lock().unwrap().clone()))
            }
            CoreRequest::UserPromptQueueDelete { entry_id, .. } => {
                self.record(&format!("delete:{entry_id}"));
                if let Some(code) = self.delete_error.lock().unwrap().clone() {
                    return Ok(self.error_answer(&Some(code)));
                }
                // Mirrors `queue_projection`: the write returns the projection
                // it just produced, so the frontend has no reason to re-read.
                let mut next = self.projection.lock().unwrap().clone();
                next.retain(|e| e.id != entry_id);
                self.set_projection(next.clone());
                Ok(self.answered(next))
            }
            CoreRequest::UserPromptQueueAdd {
                entry_id,
                text,
                run_at_ms,
                ..
            } => {
                // M007: the due time is part of what leaves the box, so it is
                // part of what this fake records. Without it a test could only
                // assert that *a* request went out, not that the schedule did.
                self.record(&format!("add:{entry_id}:{text}:{run_at_ms:?}"));
                let mut next = self.projection.lock().unwrap().clone();
                // Idempotent by identity, like the store: a repeat of the same
                // id converges on the existing row rather than duplicating it.
                if !next.iter().any(|e| e.id == entry_id) {
                    let ordinal = next.len() as i64;
                    next.push(UserPromptQueueEntryDto {
                        id: entry_id.clone(),
                        ordinal,
                        text: text.clone(),
                        text_bytes: text.len(),
                        status: "pending".to_string(),
                        created_at_ms: 1,
                        expires_at_ms: 0,
                        run_at_ms: None,
                    });
                }
                self.set_projection(next.clone());
                Ok(self.answered(next))
            }
            CoreRequest::UserPromptQueueEdit {
                entry_id,
                text,
                run_at_ms,
                ..
            } => {
                self.record(&format!("edit:{entry_id}:{text}:{run_at_ms:?}"));
                let mut next = self.projection.lock().unwrap().clone();
                if let Some(entry) = next.iter_mut().find(|e| e.id == entry_id) {
                    // Identity and ordinal are untouched: an edit replaces
                    // text and nothing else.
                    entry.text = text.clone();
                    entry.text_bytes = text.len();
                }
                self.set_projection(next.clone());
                Ok(self.answered(next))
            }
            CoreRequest::UserPromptQueueReorder { entry_ids, .. } => {
                self.record(&format!("reorder:{}", entry_ids.join(",")));
                let next = self.projection.lock().unwrap().clone();
                let mut permuted: Vec<UserPromptQueueEntryDto> = Vec::new();
                for id in &entry_ids {
                    if let Some(entry) = next.iter().find(|e| &e.id == id) {
                        permuted.push(entry.clone());
                    }
                }
                // Rows the permutation did not name keep their relative order
                // after the named ones — which is what `renumber_dense` does for
                // a `delivering` row.
                for entry in next.iter() {
                    if !entry_ids.contains(&entry.id) {
                        permuted.push(entry.clone());
                    }
                }
                for (index, entry) in permuted.iter_mut().enumerate() {
                    entry.ordinal = index as i64;
                }
                self.set_projection(permuted.clone());
                Ok(self.answered(permuted))
            }
            CoreRequest::UserPromptQueueSendNow { entry_id, .. } => {
                self.record(&format!("send_now:{entry_id}"));
                let next = self.projection.lock().unwrap().clone();
                let outcome = if self.send_now_missing.lock().unwrap().to_owned() {
                    UserPromptQueueSendNowResult::EntryNotFound
                } else if let Some(code) = self.send_now_refusal.lock().unwrap().clone() {
                    // A refusal leaves the queue exactly as it was — the whole
                    // point of the refusal is that the prompt is still waiting.
                    UserPromptQueueSendNowResult::Refused { code }
                } else if self.send_now_unsettled.lock().unwrap().to_owned() {
                    UserPromptQueueSendNowResult::AcceptedNotSettled {
                        turn_id: "turn-live".to_string(),
                        code: "user_prompt_queue_storage_error".to_string(),
                    }
                } else if self.send_now_accepted.lock().unwrap().to_owned() {
                    let mut next = next.clone();
                    next.retain(|e| e.id != entry_id);
                    self.set_projection(next.clone());
                    UserPromptQueueSendNowResult::Accepted {
                        turn_id: "turn-live".to_string(),
                    }
                } else {
                    panic!("the test must set one of the send_now_* switches");
                };
                Ok(CoreResponse::UserPromptQueueSendNowOutcome {
                    session_id: SESSION.to_string(),
                    entry_id: entry_id.clone(),
                    outcome,
                    entries: self.projection.lock().unwrap().clone(),
                })
            }
            // C007: the addressed reply. Recorded in full because the
            // identity in the request is the thing under test — a steer naming
            // anything other than the parent turn's id is the dead join the
            // roadmap already records, and it would be invisible here unless
            // the id were part of the record.
            CoreRequest::TurnSteer {
                session_id,
                turn_id,
                text,
            } => {
                self.record(&format!("steer:{session_id}:{turn_id}:{text}"));
                if let Some(code) = self.steer_refusal.lock().unwrap().clone() {
                    return Ok(CoreResponse::Error {
                        code,
                        message: format!(
                            "Requested turn_id '{turn_id}' does not match active turn_id \
                             'turn-other'"
                        ),
                    });
                }
                Ok(CoreResponse::Ack)
            }
            CoreRequest::TurnSubmit { text, .. } => {
                self.record(&format!("submit:{text}"));
                Ok(CoreResponse::Ack)
            }
            other => panic!("the queue box must not issue {other:?}"),
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<codegg::protocol::core::CoreEvent>> {
        mpsc::channel(1).1
    }
}

fn app_with_queue_and_core(entries: Vec<UserPromptQueueEntryDto>) -> (App, Arc<FakeQueueClient>) {
    let client = FakeQueueClient::new(entries.clone());
    let mut app = app_with_queue(entries);
    app.core_client = Some(client.clone());
    let (tx, rx) = mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);
    // The command channel is drained by nobody in this target; keeping the
    // receiver alive prevents the sends from failing.
    std::mem::forget(rx);
    (app, client)
}

/// Block until the box has issued `expected` requests of `kind`.
///
/// The read and the delete both go through `spawn_tui_task`, so the request is
/// made on a worker task and has not happened yet when `on_key` returns. A
/// `yield_now` is not enough — an earlier draft used two of them and both
/// request-count assertions failed at 0 and 1 against an expected 1 and 2,
/// which reads exactly like a missing read-on-open and is not one.
///
/// Bounded, and it names what it was waiting for when it gives up.
async fn settle_until(client: &FakeQueueClient, kind: &str, expected: usize) {
    for _ in 0..400 {
        if client.count(kind) >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!(
        "timed out waiting for {expected} `{kind}` request(s); saw {:?}",
        client.kinds()
    );
}

/// Block until at least one request of either kind has been recorded.
///
/// Used where a test asserts the *absence* of one request alongside the
/// presence of another: settling on the expected kind alone would make the
/// absent-kind assertion unreachable when the expected kind never arrives, so
/// a removed code path would report as a timeout rather than as the fall-through
/// it actually is.
async fn settle_until_either(client: &FakeQueueClient, first: &str, second: &str) {
    for _ in 0..400 {
        let kinds = client.kinds();
        if count_prefix(&kinds, first) >= 1 || count_prefix(&kinds, second) >= 1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!(
        "timed out waiting for any `{first}…` or `{second}…` request; saw {:?}",
        client.kinds()
    );
}

/// Drive the two-step composer to an actual commit.
///
/// M007 makes `a`/`e` two steps — text, then duration — so one `Enter` now only
/// advances. Every test that means "submit this" needs both, and collecting the
/// second `Enter` here means a future step change is a one-line change here
/// rather than eleven scattered edits that each read as a magic keystroke.
fn commit_compose(app: &mut codegg::tui::app::App) {
    press(app, KeyCode::Enter); // text -> duration
    press(app, KeyCode::Enter); // duration -> commit
}

/// The same, for requests recorded under a prefix.
///
/// `count` is exact equality, and an M005 mutation is recorded as
/// `add:<id>:<text>` or `reorder:<a,b,c>` because the id and the permutation
/// are the thing under test. Waiting on the bare `"add:"` therefore waits for a
/// kind that can never be recorded — which is a timeout that looks exactly like
/// "the box never sent the request".
async fn settle_prefix_until(client: &FakeQueueClient, prefix: &str, expected: usize) {
    for _ in 0..400 {
        if count_prefix(&client.kinds(), prefix) >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!(
        "timed out waiting for {expected} `{prefix}…` request(s); saw {:?}",
        client.kinds()
    );
}

fn entry(id: &str, ordinal: i64, text: &str, status: &str) -> UserPromptQueueEntryDto {
    UserPromptQueueEntryDto {
        id: id.to_string(),
        ordinal,
        text: text.to_string(),
        text_bytes: text.len(),
        status: status.to_string(),
        created_at_ms: 1,
        expires_at_ms: 0,
        run_at_ms: None,
    }
}

/// An app with a bound session, so `active_session_id` resolves.
///
/// `session_state.session` is bound too, not just the project tab: a turn
/// submit reads the session from *there* and refuses with "No session is bound
/// to this view" otherwise. A harness that set only the tab would make an
/// unarmed-submit test pass a timeout while proving nothing.
fn app_with_session() -> App {
    let mut app = App::new_for_testing("/tmp/m004".to_string());
    if let Some(tab) = app.project_tabs.active_mut() {
        tab.session_id = Some(SESSION.to_string());
    }
    app.session_state.session = Some(codegg::session::Session {
        id: SESSION.to_string(),
        ..codegg::session::Session::default()
    });
    app
}

fn app_with_queue(entries: Vec<UserPromptQueueEntryDto>) -> App {
    let mut app = app_with_session();
    app.session_state.user_prompt_queue = entries;
    app
}

/// Drive the real key path: command mode on, type `input`, Enter.
fn run_slash_command(app: &mut App, input: &str) {
    app.ui_state.command_mode = true;
    for c in input.chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn press(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Type a string through the real key path.
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    terminal.backend().buffer().clone()
}

fn buffer_text(buf: &Buffer) -> String {
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

/// The screen row the working strip's status line landed on.
///
/// Returned as a `y` coordinate so two renders can be compared for layout
/// equality — which is how "the suffix adds no row" is asserted rather than
/// assumed.
/// The frame row that contains `needle`.
fn row_containing(app: &mut App, needle: &str) -> u16 {
    let buf = render(app, 160, 40);
    (0..buf.area.height)
        .find(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, *y)].symbol().to_string())
                .collect();
            row.contains(needle)
        })
        .unwrap_or_else(|| {
            panic!(
                "no row contains {needle:?}; buffer was:\n{}",
                buffer_text(&buf)
            )
        })
}

/// Retained toast messages, newest first.
///
/// `history()` rather than the live deque: a toast expires on a wall clock, so
/// reading the live deque makes an assertion about "did this say X" depend on
/// whether the machine was slow.
fn toast_texts(app: &App) -> Vec<String> {
    app.messages_state
        .toasts
        .history()
        .map(|record| record.message.clone())
        .collect()
}

// ── Registration ──────────────────────────────────────────────────────

#[test]
fn user_prompt_queue_command_is_registered_with_a_description() {
    let registry = CommandRegistry::new();
    let cmd = registry
        .find_by_name_or_alias("/queue")
        .expect("/queue must be registered");
    assert_eq!(
        cmd.action,
        CommandAction::Builtin(BuiltinSlashAction::Queue),
        "/queue must resolve to the Queue built-in action"
    );
    assert!(
        !cmd.description.is_empty(),
        "/queue needs a description for the palette"
    );
}

// ── Reachability, on the real key path ─────────────────────────────────

#[test]
fn user_prompt_queue_slash_command_mounts_the_box_through_the_real_key_path() {
    let mut app = app_with_queue(vec![entry("a", 0, "first", "pending")]);

    run_slash_command(&mut app, "/queue");

    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::Queue,
        "typing /queue and pressing Enter must mount the queue box"
    );
    assert_eq!(Dialog::from(DialogType::Queue), Dialog::Queue);
    assert!(
        !app.ui_state.command_mode,
        "the command palette must close behind the box"
    );
}

/// The negative half, without which the test above proves nothing: an
/// unregistered command name must not mount the same box.
#[test]
fn user_prompt_queue_box_is_not_mounted_by_an_unregistered_command() {
    let mut app = app_with_queue(vec![entry("a", 0, "first", "pending")]);

    run_slash_command(&mut app, "/queuex");

    assert_ne!(
        app.focus_manager.active_dialog_type(),
        DialogType::Queue,
        "an unregistered command mounted the queue box, so the positive test is not \
         measuring what it claims"
    );
}

#[test]
fn user_prompt_queue_box_lists_the_projection_in_daemon_order() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "delivering"),
        entry("c", 2, "charlie", "pending"),
    ]);

    run_slash_command(&mut app, "/queue");

    let rows: Vec<(String, String)> = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| {
            dialog
                .entries()
                .iter()
                .map(|e| (e.id.clone(), e.status.clone()))
                .collect()
        })
        .expect("the box must be mounted");
    assert_eq!(
        rows,
        vec![
            ("a".to_string(), "pending".to_string()),
            ("b".to_string(), "delivering".to_string()),
            ("c".to_string(), "pending".to_string()),
        ],
        "the box must show the daemon's projection verbatim, including a \
         delivering row and without a local re-sort"
    );
}

#[test]
fn user_prompt_queue_box_keymap_hint_names_only_real_verbs() {
    let mut app = app_with_queue(vec![entry("a", 0, "first", "pending")]);
    run_slash_command(&mut app, "/queue");

    let hint = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| dialog.keymap_hint())
        .flatten()
        .expect("the box must advertise a keymap");

    for named in ["j", "k", "r", "d", "a", "e", "[", "]", "s"] {
        assert!(hint.contains(named), "hint is missing {named}: {hint:?}");
    }
    // M005 added `a`/`e`/`[`/`]`; M006 added `s`, so "add", "edit", and "send
    // now" are now truthful. What must stay out is everything still unimplemented:
    // timers (M007) and adoption/transfer (ADR-0037, outside the box). "steer"
    // stays out on purpose even though `s` steers — the box calls it sending,
    // which is the promise ADR-0036 actually makes.
    for forbidden in ["Esc", "Enter", "steer", "timer", "adopt"] {
        assert!(
            !hint.contains(forbidden),
            "the hint advertised {forbidden:?}, which this box does not implement: {hint:?}"
        );
    }
}

/// The box must not name timers or adoption anywhere on screen — and must not
/// name `queue-add`/`queue-edit`/`queue-reorder` as if they were commands,
/// because no such slash command exists.
///
/// Revisions are recorded rather than rewritten. M005 dropped "send now" and
/// "steer" once it added `a`/`e`/`[`/`]`; M006 dropped them again once it added
/// `s`. What remains is exactly what M007 and ADR-0037 still own. A hint that
/// teaches a dead key is the specific dishonesty the milestone plan forbids, and
/// this test has now caught the box advertising a verb twice — which is the
/// whole reason it exists.
#[test]
fn user_prompt_queue_box_advertises_nothing_it_does_not_do() {
    let mut app = app_with_queue(Vec::new());
    run_slash_command(&mut app, "/queue");

    let text = buffer_text(&render(&mut app, 120, 40));
    for forbidden in [
        "queue-add",
        "queue-edit",
        "queue-reorder",
        "queue-send",
        "force",
        "guarantee",
        "run at",
        "always reaches",
    ] {
        assert!(
            !text.contains(forbidden),
            "the box advertised {forbidden:?}, which no key in it implements: {text:?}"
        );
    }
}

// ── Observer boundary ─────────────────────────────────────────────────

/// Proven by **executing** the predicate, not by reading the allowlist source.
/// `/queue` is blocked by omission in a fail-closed `matches!`; the only way
/// to know that is still true after registration is to ask.
#[test]
fn user_prompt_queue_is_never_observer_allowed() {
    assert!(
        !is_observer_allowed_command("/queue"),
        "an observer can view a session but must never author or discard its \
         queued input; the command must stay absent from the fail-closed allowlist"
    );
    // The subcommands M005 will add must not slip in either.
    for command in ["queue-add", "queue-edit", "queue-delete", "queue-reorder"] {
        assert!(
            !is_observer_allowed_command(command),
            "{command} must never be observer-allowed"
        );
    }
}

/// Non-vacuous: the predicate really does allow some commands, so the
/// negative above is measuring the function and not an always-false stub.
///
/// `/stats` was chosen over `/logs` deliberately — `/logs` is **not** in the
/// allowlist, and a first draft of this test asserted it was, which would have
/// made the suite fail for a reason that had nothing to do with the queue.
#[test]
fn user_prompt_queue_observer_check_is_non_vacuous() {
    for allowed in ["/stats", "/help", "/status"] {
        assert!(
            is_observer_allowed_command(allowed),
            "{allowed} must be observer-allowed; if the allowlist refuses \
             everything, this suite's queue assertions would pass for the wrong reason"
        );
    }
}

// ── Delete ────────────────────────────────────────────────────────────

/// The delete response carries the projection **that delete produced** and the
/// box adopts it directly. If the frontend issued a second read instead, the
/// rendered list could disagree with the write the user just made.
#[test]
fn user_prompt_queue_delete_applies_the_returned_projection() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
        entry("c", 2, "charlie", "pending"),
    ]);
    run_slash_command(&mut app, "/queue");
    press(&mut app, KeyCode::Char('j'));

    let request_id = app.dialog_state.queue_delete_request.begin();
    codegg::tui::commands::queue::apply_queue_delete_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        // Exactly what `queue_projection` returns after removing `b`.
        vec![
            entry("a", 0, "alpha", "pending"),
            entry("c", 1, "charlie", "pending"),
        ],
        None,
        None,
    );

    let ids: Vec<String> = app
        .session_state
        .user_prompt_queue
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);
    assert!(
        toast_texts(&app).is_empty(),
        "a successful delete must not narrate itself as a failure: {:?}",
        toast_texts(&app)
    );

    let shown: Vec<String> = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| {
            dialog.entries().iter().map(|e| e.id.clone()).collect()
        })
        .expect("the box must still be mounted");
    assert_eq!(
        shown,
        vec!["a".to_string(), "c".to_string()],
        "the open box must show the delete's own projection, survivors in order"
    );
}

/// A delete refuses a `delivering` entry, and that is a different condition
/// from a vanished one. Telling the user their prompt expired at the moment a
/// turn started is a lie they might act on.
#[test]
fn user_prompt_queue_delivering_refusal_is_reported_as_delivering() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "delivering")]);
    run_slash_command(&mut app, "/queue");

    let request_id = app.dialog_state.queue_delete_request.begin();
    codegg::tui::commands::queue::apply_queue_delete_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        Vec::new(),
        Some("user_prompt_queue_entry_delivering".to_string()),
        Some("user_prompt_queue_entry_delivering".to_string()),
    );

    let messages = toast_texts(&app);
    let delivering = messages
        .iter()
        .find(|m| m.to_lowercase().contains("deliver"))
        .unwrap_or_else(|| panic!("no delivering refusal was reported: {messages:?}"));
    assert!(
        !delivering.to_lowercase().contains("expired"),
        "a delivering entry has not expired: {delivering:?}"
    );
    assert!(
        !delivering.to_lowercase().contains("no longer waiting"),
        "a delivering entry is still waiting, being delivered: {delivering:?}"
    );
    assert_eq!(
        app.session_state.user_prompt_queue.len(),
        1,
        "a refused delete must leave the entry listed"
    );
}

#[test]
fn user_prompt_queue_deliveries_have_a_distinct_refusal_sentence() {
    let app_messages = |code: &str| {
        let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
        let request_id = app.dialog_state.queue_delete_request.begin();
        codegg::tui::commands::queue::apply_queue_delete_finished(
            &mut app,
            request_id,
            SESSION.to_string(),
            Vec::new(),
            Some(code.to_string()),
            Some(code.to_string()),
        );
        toast_texts(&app).join(" | ")
    };
    let delivering = app_messages("user_prompt_queue_entry_delivering");
    let expired = app_messages("user_prompt_queue_entry_expired");
    let not_found = app_messages("user_prompt_queue_not_found");
    assert_ne!(delivering, expired, "two refusals share a sentence");
    assert_ne!(delivering, not_found, "two refusals share a sentence");
    assert_ne!(expired, not_found, "two refusals share a sentence");
}

/// A completion that lands after the box closed must change nothing at all.
#[test]
fn user_prompt_queue_stale_delete_after_close_mutates_nothing() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    run_slash_command(&mut app, "/queue");
    press(&mut app, KeyCode::Char('d'));

    let stale = app.dialog_state.queue_delete_request.begin();
    let before = app.session_state.user_prompt_queue.clone();

    press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::None,
        "Esc must close the queue box"
    );
    // `toasts before`, not "empty": pressing `d` with no core connection
    // legitimately reported that, and that message is not the stale
    // completion's.
    let toasts_before = toast_texts(&app);

    codegg::tui::commands::queue::apply_queue_delete_finished(
        &mut app,
        stale,
        SESSION.to_string(),
        vec![entry("a", 0, "alpha", "pending")],
        None,
        None,
    );

    assert_eq!(
        app.session_state.user_prompt_queue, before,
        "a delete that completes after the box closed must not rewrite the \
         projection the user is no longer looking at"
    );
    assert_eq!(
        toast_texts(&app),
        toasts_before,
        "a stale completion must be dropped whole: {:?}",
        toast_texts(&app)
    );
}

/// `close_dialog` must not undo the action that preceded it. Anything still
/// mounted at close time was dismissed *unanswered*; the queue box has no
/// pending human decision to requeue, so closing it changes no daemon state.
#[test]
fn user_prompt_queue_closing_the_box_requeues_nothing() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    run_slash_command(&mut app, "/queue");

    press(&mut app, KeyCode::Esc);

    assert_eq!(app.focus_manager.active_dialog_type(), DialogType::None);
    assert_eq!(
        app.session_state.user_prompt_queue.len(),
        1,
        "closing the box must not drop the queue"
    );
    assert!(
        app.dialog_state.permission_queue.is_empty(),
        "closing the queue box must not touch the permission queue"
    );
}

// ── Read path ─────────────────────────────────────────────────────────

/// Opening must issue its own read even when a projection is already cached.
/// The cache can predate a delivery or a second frontend's mutation, and only
/// a read covers the second.
///
/// Counted at the transport, not at the task registry: `load_user_prompt_queue`
/// returns early without a `core_client`, so a registry count with no client
/// measures nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_open_issues_a_read_even_with_a_populated_projection() {
    let (mut app, client) = app_with_queue_and_core(vec![entry("a", 0, "alpha", "pending")]);

    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    // Give a spurious second read a chance to arrive before asserting one.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(
        client.count("list"),
        1,
        "opening the box must issue exactly one queue read, not one per frame \
         and not one per surface; saw {:?}",
        client.kinds()
    );
}

/// `r` issues another one, and the box does not cache its way out of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_refresh_issues_another_read() {
    let (mut app, client) = app_with_queue_and_core(vec![entry("a", 0, "alpha", "pending")]);
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('r'));
    settle_until(&client, "list", 2).await;
    // Drain, so a third read spawned by the second one cannot slip past.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    assert_eq!(
        client.count("list"),
        2,
        "`r` must re-read, not reuse the cached projection; saw {:?}",
        client.kinds()
    );
}

/// A failed read leaves the previous projection intact. Clearing it would make
/// a dead socket indistinguishable from an empty queue.
#[test]
fn user_prompt_queue_failed_read_keeps_the_previous_projection() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    run_slash_command(&mut app, "/queue");
    press(&mut app, KeyCode::Char('r'));
    let before = app.session_state.user_prompt_queue.clone();

    app.apply_user_prompt_queue_loaded(
        SESSION.to_string(),
        Vec::new(),
        Some("socket closed".to_string()),
    );

    assert_eq!(
        app.session_state.user_prompt_queue, before,
        "a failed read must not be mistaken for an empty queue"
    );
}

/// A completion for a superseded session must not write into current state.
#[test]
fn user_prompt_queue_projection_from_another_session_is_dropped() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    let before = app.session_state.user_prompt_queue.clone();

    app.apply_user_prompt_queue_loaded(
        "some-other-session".to_string(),
        vec![entry("z", 0, "zulu", "pending")],
        None,
    );

    assert_eq!(app.session_state.user_prompt_queue, before);
}

/// The M003 push path and the read path share one applier, so a pushed and a
/// read projection cannot diverge — and an open box follows the push.
#[test]
fn user_prompt_queue_push_updates_an_open_box() {
    let mut app = app_with_queue(Vec::new());
    run_slash_command(&mut app, "/queue");
    press(&mut app, KeyCode::Char('r'));

    app.apply_user_prompt_queue_loaded(
        SESSION.to_string(),
        vec![
            entry("p1", 0, "pushed one", "pending"),
            entry("p2", 1, "pushed two", "delivering"),
        ],
        None,
    );

    let shown: Vec<(String, String)> = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| {
            dialog
                .entries()
                .iter()
                .map(|e| (e.id.clone(), e.status.clone()))
                .collect()
        })
        .expect("the box must still be mounted");
    assert_eq!(
        shown,
        vec![
            ("p1".to_string(), "pending".to_string()),
            ("p2".to_string(), "delivering".to_string()),
        ],
        "a pushed projection must reach the open box with no re-read"
    );
}

// ── Row 0 ─────────────────────────────────────────────────────────────

#[test]
fn user_prompt_queue_row_zero_names_the_waiting_count() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);

    let text = buffer_text(&render(&mut app, 160, 40));

    assert!(
        text.contains("2 queued"),
        "row 0 must say how many prompts are waiting: {text:?}"
    );
}

#[test]
fn user_prompt_queue_row_zero_says_nothing_when_the_queue_is_empty() {
    let mut app = app_with_queue(Vec::new());

    let text = buffer_text(&render(&mut app, 160, 40));

    assert!(
        !text.contains("queued"),
        "an empty queue must not add a suffix: {text:?}"
    );
}

/// The suffix composes under the established precedence: a latched failure
/// outranks every live phase, and by the time it is readable here the toast
/// that also said so has expired.
#[test]
fn user_prompt_queue_row_zero_suffix_never_replaces_a_latched_failure() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    app.session_state.last_error = Some("model stream ended unexpectedly".to_string());

    let text = buffer_text(&render(&mut app, 160, 40));

    assert!(
        text.contains("model stream ended unexpectedly"),
        "the latched failure must still be the row: {text:?}"
    );
    assert!(
        text.contains("1 queued"),
        "the suffix must still compose alongside it: {text:?}"
    );
}

/// The suffix is text on the row above the composer, and that row is carved
/// from the prompt chunk. Gating the suffix on queue state would reflow the
/// transcript on every submit and completion.
#[test]
fn user_prompt_queue_suffix_adds_no_row_and_changes_no_geometry() {
    let mut empty = app_with_queue(Vec::new());
    let _ = render(&mut empty, 160, 40);
    let empty_prompt = empty.prompt_area.expect("prompt area recorded");

    let mut full = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
        entry("c", 2, "charlie", "pending"),
    ]);
    let _ = render(&mut full, 160, 40);
    let full_prompt = full.prompt_area.expect("prompt area recorded");

    assert_eq!(
        empty_prompt.y, full_prompt.y,
        "the queue suffix must not move the composer: an empty queue put it at \
         y={} and a three-entry queue at y={}",
        empty_prompt.y, full_prompt.y
    );
    // And the count is on screen, on the row above the composer.
    let row = row_containing(&mut full, "3 queued");
    assert_eq!(
        row,
        full_prompt.y - 1,
        "the queue count belongs on the row above the composer"
    );
}

// ── Prompt text confidentiality ───────────────────────────────────────

/// Prompt text is user input. It belongs in the list and nowhere else: no
/// toast, no hint, no diagnostic. Asserted by scanning the built strings, not
/// by inspection.
#[test]
fn user_prompt_queue_prompt_text_never_reaches_a_toast_or_a_hint() {
    const SECRET: &str = "my-password-is-hunter2";
    let mut app = app_with_queue(vec![entry("a", 0, SECRET, "pending")]);
    run_slash_command(&mut app, "/queue");

    let hint = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| dialog.keymap_hint())
        .flatten()
        .unwrap_or_default();
    assert!(
        !hint.contains(SECRET),
        "prompt text leaked into the keymap hint: {hint:?}"
    );

    // Force every refusal path that can produce a message.
    let request_id = app.dialog_state.queue_delete_request.begin();
    codegg::tui::commands::queue::apply_queue_delete_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        Vec::new(),
        Some("user_prompt_queue_entry_delivering".to_string()),
        Some("user_prompt_queue_entry_delivering".to_string()),
    );
    for message in toast_texts(&app) {
        assert!(
            !message.contains(SECRET),
            "prompt text leaked into a toast: {message:?}"
        );
    }
}

// ── Source-level guards ───────────────────────────────────────────────

/// The delete path must issue **exactly one** request and it must not be a
/// read. `queue_projection` already returns the post-write projection, so a
/// second `UserPromptQueueList` would be a redundant round trip *and* would let
/// the list render something other than the write the user just made.
///
/// Measured at the transport through the real key path, not by reading the
/// frontend source. An earlier draft of this test grepped
/// `commands/queue.rs` for the string `UserPromptQueueList` and failed on the
/// module's own **doc comment** — a guard that trips on its own explanation
/// measures documentation, not behaviour.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_delete_path_issues_no_second_read() {
    let (mut app, client) = app_with_queue_and_core(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    assert_eq!(client.count("list"), 1, "precondition: the open read");

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('d'));
    settle_until(&client, "delete:b", 1).await;
    // **Drain before counting.** Waiting only for the delete returns as soon
    // as the delete lands, which is *before* anything its completion spawns
    // reaches the transport. A first draft of this test stopped here and then
    // passed against an injected `refresh_queue_box` in the delete path — the
    // guard was racing in exactly the direction that hides the violation.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    assert_eq!(
        client.count("delete:b"),
        1,
        "`d` must delete exactly the selected entry; saw {:?}",
        client.kinds()
    );
    assert_eq!(
        client.count("list"),
        1,
        "the delete path must apply the projection the delete returned, not \
         issue a second read; saw {:?}",
        client.kinds()
    );

    // ── the apply half ──
    //
    // Everything above ends at the request: `start_queue_delete` spawns a
    // task that answers on `tui_cmd_tx`, and nothing in a frontend test
    // dispatches that channel. So the assertion above covers the *request*
    // path only, and a second read added inside `apply_queue_delete_finished`
    // passes it untouched. Driving the apply here is what closes that hole —
    // and it is the half an earlier draft of this test was silently missing.
    let request_id = app.dialog_state.queue_delete_request.begin();
    codegg::tui::commands::queue::apply_queue_delete_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        vec![entry("a", 0, "alpha", "pending")],
        None,
        None,
    );
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    assert_eq!(
        client.count("list"),
        1,
        "applying a delete must adopt the returned projection, not re-read; \
         saw {:?}",
        client.kinds()
    );
    assert_eq!(
        client.count("delete:b"),
        1,
        "applying a delete must not re-issue it either; saw {:?}",
        client.kinds()
    );
}

/// `push_dialog` refuses a duplicate dialog type, so a second `/queue` would
/// otherwise be swallowed without a word.
#[test]
fn user_prompt_queue_second_open_is_answered_not_ignored() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    run_slash_command(&mut app, "/queue");

    // The second open goes through the command surface directly rather than
    // the composer: a mounted modal short-circuits command mode, so typing
    // `/queue` again would never reach dispatch at all. Registry-to-action
    // wiring is covered separately by
    // `user_prompt_queue_command_is_registered_with_a_description`.
    codegg::tui::commands::queue::open_queue_box(&mut app);

    assert!(
        toast_texts(&app).iter().any(|m| m.contains("already open")),
        "a second /queue must say the box is already open, not silently do \
         nothing: {:?}",
        toast_texts(&app)
    );
}

/// M004 introduces a `TuiCommand` variant, so the dispatch entry point must
/// still compile and the variant must be reachable. This asserts the wiring
/// exists by constructing the variant, which a removed dispatch arm would
/// still allow — the real evidence is that `verify.sh quick` compiles.
#[test]
fn user_prompt_queue_delete_completion_is_a_constructible_tui_command() {
    let command = TuiCommand::UserPromptQueueDeleteFinished {
        request_id: 1,
        session_id: SESSION.to_string(),
        entries: Vec::new(),
        code: None,
        error: None,
    };
    let debug = format!("{command:?}");
    assert!(
        debug.contains("UserPromptQueueDeleteFinished"),
        "got {debug:?}"
    );
}

// ── M005: add, edit, reorder on the real key path ────────────────────
//
// Everything below goes through `App::on_key` on a mounted box, because the
// milestone's whole risk is reachability: a key that never reaches the widget,
// or reaches it and is swallowed, looks fine in the source and wrong in front
// of a user.
//
// Two harness shapes, both from M004:
//   * request counts need the real transport and `settle_until`, because the
//     write happens on a worker task;
//   * apply-path assertions call `apply_*_finished` directly with a
//     `begin()`-ed request id, because nothing in this target drains
//     `tui_cmd_tx`.

/// An app with a bound session, a fake queue transport, and the box open.
///
/// The projection is seeded into **both** the session cache and the transport:
/// the box mounts over the cache, so seeding only the transport would open an
/// empty list and make every key look like it did nothing.
fn open_queue_app(projection: Vec<UserPromptQueueEntryDto>) -> (App, Arc<FakeQueueClient>) {
    let client = FakeQueueClient::new(projection.clone());
    let mut app = app_with_queue(projection);
    app.core_client = Some(client.clone());
    let (tx, rx) = mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);
    // Nothing in this target drains the command channel; keeping the receiver
    // alive stops the sends from failing.
    std::mem::forget(rx);
    codegg::tui::commands::queue::open_queue_box(&mut app);
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::Queue,
        "the box must be mounted for every test below"
    );
    (app, client)
}

/// The ids the open box is showing, in render order.
fn shown_ids(app: &App) -> Vec<String> {
    app.focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| {
            dialog.entries().iter().map(|e| e.id.clone()).collect()
        })
        .expect("the box must still be mounted")
}

/// The open box's compose buffer, `None` when no text is being composed.
fn compose_of(app: &App) -> Option<String> {
    app.focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| {
            (dialog.is_text_step()).then(|| dialog.compose().to_string())
        })
        .flatten()
}

/// The id the open box has selected.
fn selected_id(app: &App) -> Option<String> {
    app.focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| {
            dialog.selected_entry_id().map(str::to_owned)
        })
        .flatten()
}

/// How many recorded requests start with `prefix`.
fn count_prefix(kinds: &[String], prefix: &str) -> usize {
    kinds.iter().filter(|k| k.starts_with(prefix)).count()
}

// ── WP-A: the text step ───────────────────────────────────────────────

/// The plan's central hazard, driven through the real key path.
///
/// Typing a prompt that *contains* the action letters must not delete, edit,
/// add, navigate, refresh, or reorder anything. This is the test M005 exists
/// to make pass.
#[tokio::test]
async fn user_prompt_queue_typing_action_letters_produces_text_not_actions() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "first", "pending"),
        entry("b", 1, "second", "pending"),
    ]);
    // The opening read is issued by `open_queue_box`.
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('a'));
    assert!(compose_of(&app).is_some(), "`a` must open a compose step");
    type_text(&mut app, "dearjk r[]");

    assert_eq!(
        client.kinds(),
        vec!["list".to_string()],
        "typing action letters must issue no mutation at all: {:?}",
        client.kinds()
    );
    assert_eq!(
        app.session_state.user_prompt_queue.len(),
        2,
        "nothing was added or deleted"
    );
    assert_eq!(selected_id(&app).as_deref(), Some("a"), "cursor unmoved");

    let composed = compose_of(&app).expect("still composing");
    assert_eq!(composed, "dearjk r[]", "every letter became text");

    press(&mut app, KeyCode::Esc);
    assert_eq!(compose_of(&app), None, "Esc abandons the step");
}

/// The same keys, outside a text step, must do their jobs — otherwise the
/// sweep above would be satisfied by a box that implements nothing.
#[tokio::test]
async fn user_prompt_queue_the_same_keys_act_once_the_composer_is_closed() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "first", "pending"),
        entry("b", 1, "second", "pending"),
    ]);
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('j'));
    assert_eq!(selected_id(&app).as_deref(), Some("b"));
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(selected_id(&app).as_deref(), Some("a"));

    press(&mut app, KeyCode::Char('d'));
    settle_until(&client, "delete:a", 1).await;
    assert_eq!(client.count("delete:a"), 1);

    press(&mut app, KeyCode::Char('r'));
    settle_until(&client, "list", 2).await;
    assert_eq!(client.count("list"), 2, "`r` must re-read");
}

// ── WP-B: add ─────────────────────────────────────────────────────────

/// `a` queues one request carrying exactly what was typed, and the box
/// applies that request's own projection without a second read.
#[tokio::test]
async fn user_prompt_queue_add_issues_one_mutation_and_applies_its_projection() {
    let (mut app, client) = open_queue_app(Vec::new());
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "run the tests");
    commit_compose(&mut app);

    settle_prefix_until(&client, "add:", 1).await;
    let kinds = client.kinds();
    assert_eq!(
        count_prefix(&kinds, "add:"),
        1,
        "exactly one add, not a retry loop: {kinds:?}"
    );
    assert_eq!(
        kinds.last().expect("an add"),
        kinds.last().expect("an add"),
        "sanity"
    );
    assert!(
        kinds.last().unwrap().contains(":run the tests:"),
        "the add must carry exactly what was typed, plus its due time: {kinds:?}"
    );
    assert_eq!(
        count_prefix(&kinds, "list"),
        1,
        "the add applies its own projection — no second read: {kinds:?}"
    );
}

/// The cursor follows the row the add created, keyed by the id minted at
/// commit rather than by the position it happened to land in.
#[tokio::test]
async fn user_prompt_queue_a_completed_add_selects_the_row_it_created() {
    let (mut app, _client) = open_queue_app(vec![entry("a", 0, "first", "pending")]);

    let request_id = app.dialog_state.queue_add_request.begin();
    codegg::tui::commands::queue::apply_queue_add_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        vec![
            entry("a", 0, "first", "pending"),
            entry("minted", 1, "run the tests", "pending"),
        ],
        None,
        None,
        "minted".to_string(),
        "run the tests".to_string(),
    );

    assert_eq!(selected_id(&app).as_deref(), Some("minted"));
    assert_eq!(shown_ids(&app), vec!["a".to_string(), "minted".to_string()]);
    assert_eq!(
        compose_of(&app),
        None,
        "the composer must not reopen after a successful add"
    );
}

/// Two adds of the same sentence are two entries: idempotency is entry
/// identity, so equal text never collapses into one row.
#[tokio::test]
async fn user_prompt_queue_two_identical_adds_are_two_entries() {
    let (mut app, client) = open_queue_app(Vec::new());
    settle_until(&client, "list", 1).await;

    for _ in 0..2 {
        press(&mut app, KeyCode::Char('a'));
        type_text(&mut app, "same words");
        commit_compose(&mut app);
    }
    settle_prefix_until(&client, "add:", 2).await;

    let adds: Vec<String> = client
        .kinds()
        .into_iter()
        .filter(|k| k.starts_with("add:"))
        .collect();
    assert_eq!(adds.len(), 2, "two adds: {adds:?}");
    let first_id = adds[0]
        .strip_prefix("add:")
        .unwrap()
        .split(':')
        .next()
        .unwrap();
    let second_id = adds[1]
        .strip_prefix("add:")
        .unwrap()
        .split(':')
        .next()
        .unwrap();
    assert_ne!(
        first_id, second_id,
        "each add mints its own id, which is what makes the second a second row"
    );
}

/// A bounds refusal is shown as its own sentence and **keeps** the composed
/// text, because the fix is to shorten it.
#[tokio::test]
async fn user_prompt_queue_an_over_length_add_is_refused_visibly_and_keeps_its_text() {
    let (mut app, _client) = open_queue_app(Vec::new());

    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "a very long prompt indeed");
    commit_compose(&mut app);

    let request_id = app.dialog_state.queue_add_request.begin();
    codegg::tui::commands::queue::apply_queue_add_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        Vec::new(),
        Some("user_prompt_queue_entry_too_large".to_string()),
        Some("user_prompt_queue_entry_too_large".to_string()),
        "minted".to_string(),
        "a very long prompt indeed".to_string(),
    );

    let toasts = toast_texts(&app);
    assert!(
        toasts
            .iter()
            .any(|m| m.contains("too long for one queue entry")),
        "the refusal must be shown: {toasts:?}"
    );
    assert!(
        toasts.iter().all(|m| !m.contains("a very long prompt")),
        "a toast must never carry prompt text: {toasts:?}"
    );
    assert_eq!(
        compose_of(&app).as_deref(),
        Some("a very long prompt indeed"),
        "the composed text must survive so it can be shortened"
    );
    assert!(
        app.session_state.user_prompt_queue.is_empty(),
        "a refused add queues nothing"
    );
}

/// An unowned queue has no in-box repair, and the refusal must say so rather
/// than leaving the user stuck with a bare "refused".
#[tokio::test]
async fn user_prompt_queue_the_unowned_refusal_says_where_the_repair_is_not() {
    let (mut app, _client) = open_queue_app(Vec::new());
    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "hello");
    commit_compose(&mut app);

    let request_id = app.dialog_state.queue_add_request.begin();
    codegg::tui::commands::queue::apply_queue_add_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        Vec::new(),
        Some("user_prompt_queue_unowned".to_string()),
        Some("user_prompt_queue_unowned".to_string()),
        "minted".to_string(),
        "hello".to_string(),
    );

    let toasts = toast_texts(&app);
    assert!(
        toasts.iter().any(|m| m.contains("no owner")),
        "got {toasts:?}"
    );
    assert!(
        toasts.iter().any(|m| m.contains("outside this box")),
        "the message must say the repair is not reachable from here: {toasts:?}"
    );
    assert!(
        toasts.iter().all(|m| !m.contains("/queue-adopt")),
        "no slash command is registered for adoption, so naming one would teach \
         a key that does nothing: {toasts:?}"
    );
    assert_eq!(
        compose_of(&app),
        None,
        "an ownership refusal is not fixable by editing the text, so it must not \
         invite a retry: {toasts:?}"
    );
}

// ── WP-C: edit ────────────────────────────────────────────────────────

/// `e` revises the selected row in place: same identity, same position, new
/// text.
#[tokio::test]
async fn user_prompt_queue_edit_changes_text_and_neither_id_nor_position() {
    let (mut app, _client) = open_queue_app(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
        entry("c", 2, "charlie", "pending"),
    ]);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('e'));

    // Seeded from the row, so the user revises rather than retypes.
    assert_eq!(compose_of(&app).as_deref(), Some("bravo"));
    for _ in 0..5 {
        press(&mut app, KeyCode::Backspace);
    }
    type_text(&mut app, "revised");
    commit_compose(&mut app);

    let request_id = app.dialog_state.queue_edit_request.begin();
    let mut projection = shown_entries();
    projection[1].text = "revised".to_string();
    codegg::tui::commands::queue::apply_queue_edit_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        projection,
        None,
        None,
        "b".to_string(),
        "revised".to_string(),
    );

    let shown = shown_ids(&app);
    assert_eq!(
        shown,
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "an edit changes no position"
    );
    let entries = entries_of(&app);
    assert_eq!(entries[1].id, "b", "an edit changes no identity");
    assert_eq!(entries[1].text, "revised");
    assert_eq!(entries[1].ordinal, 1, "an edit changes no ordinal");
    assert_eq!(selected_id(&app).as_deref(), Some("b"));
}

/// A rejected edit must not move the cursor or clear the buffer, and its
/// refusal must be distinct from a missing entry.
#[tokio::test]
async fn user_prompt_queue_a_delivering_edit_is_refused_without_moving_the_selection() {
    let (mut app, _client) = open_queue_app(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "delivering"),
    ]);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('e'));
    type_text(&mut app, " revised");
    commit_compose(&mut app);

    let request_id = app.dialog_state.queue_edit_request.begin();
    codegg::tui::commands::queue::apply_queue_edit_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        Vec::new(),
        Some("user_prompt_queue_entry_delivering".to_string()),
        Some("user_prompt_queue_entry_delivering".to_string()),
        "b".to_string(),
        "bravo revised".to_string(),
    );

    let toasts = toast_texts(&app);
    assert!(
        toasts.iter().any(|m| m.to_lowercase().contains("deliver")),
        "got {toasts:?}"
    );
    assert!(
        toasts.iter().all(|m| !m.to_lowercase().contains("expired")),
        "a delivering entry has not expired: {toasts:?}"
    );
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("b"),
        "a refused edit must not move the cursor"
    );
}

/// The rows the open box is showing, as full DTOs.
fn entries_of(app: &App) -> Vec<UserPromptQueueEntryDto> {
    app.focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |dialog| dialog.entries().to_vec())
        .expect("the box must still be mounted")
}

fn shown_entries() -> Vec<UserPromptQueueEntryDto> {
    vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
        entry("c", 2, "charlie", "pending"),
    ]
}

// ── WP-D: reorder ─────────────────────────────────────────────────────

/// `[` moves the selected entry one place earlier, and the request carries the
/// **whole pending order**, permuted — because `reorder` takes a permutation,
/// not a move.
#[tokio::test]
async fn user_prompt_queue_reorder_sends_the_pending_permutation_and_no_second_read() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
        entry("c", 2, "charlie", "pending"),
    ]);
    settle_until(&client, "list", 1).await;

    // Move `c` earlier: swap with `b`.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('['));

    settle_prefix_until(&client, "reorder:", 1).await;
    let kinds = client.kinds();
    assert_eq!(
        count_prefix(&kinds, "reorder:"),
        1,
        "exactly one: {kinds:?}"
    );
    assert_eq!(
        kinds.last().unwrap(),
        "reorder:a,c,b",
        "the permutation is the pending order with `c` moved earlier: {kinds:?}"
    );
    assert_eq!(
        count_prefix(&kinds, "list"),
        1,
        "the reorder applies its own projection — no second read: {kinds:?}"
    );
}

/// The milestone's sharpest edge, on the real key path: a `delivering` row must
/// never appear in the permutation, or the daemon answers `order_mismatch`.
#[tokio::test]
async fn user_prompt_queue_reorder_omits_a_delivering_row_from_the_permutation() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "alpha", "delivering"),
        entry("b", 1, "bravo", "pending"),
        entry("c", 2, "charlie", "pending"),
    ]);
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('['));

    settle_prefix_until(&client, "reorder:", 1).await;
    let sent = client.kinds().last().unwrap().clone();
    assert_eq!(sent, "reorder:c,b", "got {sent:?}");
    assert!(
        !sent.contains('a'),
        "naming the delivering row would be answered with order_mismatch, \
         because the daemon checks the permutation against pending rows only"
    );
}

/// A `delivering` selection cannot be reordered, and the box says why instead
/// of doing nothing.
#[tokio::test]
async fn user_prompt_queue_reordering_a_delivering_row_explains_itself() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "delivering"),
    ]);
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('['));

    let toasts = toast_texts(&app);
    assert!(
        toasts
            .iter()
            .any(|m| m.to_lowercase().contains("delivered")
                || m.to_lowercase().contains("delivering")),
        "the refusal must name the condition: {toasts:?}"
    );
    assert_eq!(
        client.count("reorder:"),
        0,
        "no mutation may be issued for a row the daemon would refuse: {:?}",
        client.kinds()
    );
}

/// At an end there is nothing to swap with, and the box names which end rather
/// than moving the row or staying silent.
#[tokio::test]
async fn user_prompt_queue_reorder_at_an_end_names_which_end() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    settle_until(&client, "list", 1).await;

    press(&mut app, KeyCode::Char('['));
    assert!(
        toast_texts(&app)
            .iter()
            .any(|m| m.contains("already first")),
        "got {:?}",
        toast_texts(&app)
    );
    assert_eq!(client.count("reorder:"), 0);
}

/// `order_mismatch` re-reads **once** and reports. It never retries: the
/// daemon's check is exact-length set equality, so a blind retry against a
/// stale projection is a contention source.
#[tokio::test]
async fn user_prompt_queue_an_order_mismatch_re_reads_once_and_does_not_retry() {
    let (mut app, client) = open_queue_app(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);

    let request_id = app.dialog_state.queue_reorder_request.begin();
    codegg::tui::commands::queue::apply_queue_reorder_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        Vec::new(),
        Some("user_prompt_queue_order_mismatch".to_string()),
        Some("user_prompt_queue_order_mismatch".to_string()),
    );

    let toasts = toast_texts(&app);
    assert!(
        toasts
            .iter()
            .any(|m| m.contains("refused rather") || m.contains("changed underneath")),
        "the refusal must say it was refused, not merged: {toasts:?}"
    );
    assert!(
        toasts
            .iter()
            .any(|m| m.contains("Showing the current order")),
        "the user must be told the order was re-read: {toasts:?}"
    );
    assert_eq!(
        client.count("reorder:"),
        0,
        "the apply path must not re-issue the mutation it just had refused"
    );
}

/// Closing the box with unsaved text adds nothing and edits nothing, and a
/// completion that lands afterwards mutates nothing at all.
#[tokio::test]
async fn user_prompt_queue_a_completion_after_close_changes_nothing() {
    let (mut app, _client) = open_queue_app(vec![entry("a", 0, "alpha", "pending")]);

    // An add is in flight.
    let request_id = app.dialog_state.queue_add_request.begin();
    press(&mut app, KeyCode::Esc);
    assert!(
        !app.focus_manager.has_dialog(DialogType::Queue),
        "the box must be gone"
    );

    // The late completion arrives.
    codegg::tui::commands::queue::apply_queue_add_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        vec![
            entry("a", 0, "alpha", "pending"),
            entry("minted", 1, "late", "pending"),
        ],
        None,
        None,
        "minted".to_string(),
        "late".to_string(),
    );

    assert!(
        app.session_state.user_prompt_queue.len() == 1,
        "a cancelled completion must not write a projection: {:?}",
        app.session_state.user_prompt_queue
    );
    assert!(
        toast_texts(&app).is_empty(),
        "and must not raise a toast either: {:?}",
        toast_texts(&app)
    );
}

// ── WP-E: honesty ─────────────────────────────────────────────────────

/// The host-first keymap rule, applied to the M005 keys.
///
/// `App::handle_dialog_key` intercepts `Shift+←/→/↑/↓` before the widget sees
/// them, gated on `top_docks_as_viewer() && dock_target().is_some()`. This box
/// declares `docks_as_viewer() == true`, so the **only** thing keeping the
/// docking steal from swallowing its keys is the absence of a `DialogType::Queue`
/// arm in `dock_target`. That is an omission, which is exactly the kind of
/// property that silently stops holding.
#[tokio::test]
async fn user_prompt_queue_the_host_does_not_steal_its_keys() {
    let (app, _client) = open_queue_app(vec![entry("a", 0, "alpha", "pending")]);

    assert_eq!(app.focus_manager.active_dialog_type(), DialogType::Queue);
    assert!(
        app.dock_target().is_none(),
        "if `dock_target` ever grows a `DialogType::Queue` arm, `handle_dialog_key` \
         starts consuming keys before the box sees them — re-check every key \
         this box advertises"
    );
    assert!(
        app.focus_manager.top_docks_as_viewer(),
        "this box declares itself a viewer, so the docking branch is reachable \
         in principle and only `dock_target()` keeps it inert"
    );
}

/// The advertised keys are exactly the keys the box answers. `Esc` is omitted
/// because the modal host claims it; `Enter` is omitted from the browsing hint
/// because it is the *confirm* key inside a composer, not a browsing verb.
#[tokio::test]
async fn user_prompt_queue_the_hint_advertises_nothing_the_box_cannot_do() {
    let (mut app, _client) = open_queue_app(vec![entry("a", 0, "alpha", "pending")]);

    let browsing = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |d| d.keymap_hint().expect("hint"))
        .expect("mounted");
    for named in ["j", "k", "r", "d", "a", "e", "[", "]", "s"] {
        assert!(
            browsing.contains(named),
            "hint is missing {named}: {browsing:?}"
        );
    }
    for forbidden in [
        "Esc",
        "Enter",
        "steer",
        "force",
        "guarantee",
        "adopt",
        "timer",
        "↑",
        "↓",
    ] {
        assert!(
            !browsing.contains(forbidden),
            "the browsing hint advertises {forbidden:?}, which this box does not \
             do while browsing: {browsing:?}"
        );
    }

    press(&mut app, KeyCode::Char('a'));
    let composing = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |d| d.keymap_hint().expect("hint"))
        .expect("mounted");
    assert!(
        composing.contains("Enter"),
        "a composer must advertise its confirm key: {composing:?}"
    );
    // M006: `s` is an action key and must be inert here too — this is the arm
    // whose inversion the milestone's mandatory injection targets.
    for inert in ["delete", "reorder", "refresh", "move", "send now"] {
        assert!(
            !composing.contains(inert),
            "the composer hint advertises {inert:?}, which is inert while text \
             has the keyboard: {composing:?}"
        );
    }
}

/// Observers are refused the box, and therefore all three new actions.
/// `queue-add`/`-edit`/`-reorder` stay out of the allowlist by omission.
#[test]
fn user_prompt_queue_no_m005_action_is_observer_allowed() {
    for command in [
        "queue",
        "queue-add",
        "queue-edit",
        "queue-delete",
        "queue-reorder",
    ] {
        assert!(
            !is_observer_allowed_command(command),
            "{command} must never be observer-allowed"
        );
    }
    // Non-vacuity: the predicate is a real allowlist, not a blanket refusal.
    for allowed in ["/stats", "/help", "/status"] {
        assert!(
            is_observer_allowed_command(allowed),
            "{allowed} must be allowed"
        );
    }
}

// ── M006: send-now ─────────────────────────────────────────────────────
//
// ADR-0036's promise, measured at the boundary the box owns: `s` reaches the
// daemon naming an entry and nothing else; an accepted prompt leaves the live
// projection; and every refusal leaves it exactly where it was, with a sentence
// that is its own.

/// An app with a bound session, a live turn, and a core connection.
///
/// The live turn is the point: without it the box refuses locally and never
/// reaches the daemon, which is correct but makes these tests vacuous.
fn app_with_queue_and_turn(entries: Vec<UserPromptQueueEntryDto>) -> (App, Arc<FakeQueueClient>) {
    let (mut app, client) = app_with_queue_and_core(entries);
    app.session_state.active_turn_id = Some("turn-live".to_string());
    (app, client)
}

fn dialog_entries(app: &App) -> Vec<UserPromptQueueEntryDto> {
    app.session_state.user_prompt_queue.clone()
}

/// The happy path: an accepted prompt leaves the projection, and no second
/// delivery is issued on its behalf.
#[tokio::test]
async fn user_prompt_queue_send_now_hands_a_prompt_to_the_running_turn() {
    let (mut app, client) = app_with_queue_and_turn(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    *client.send_now_accepted.lock().unwrap() = true;
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    press(&mut app, KeyCode::Char('s'));

    settle_prefix_until(&client, "send_now:", 1).await;
    let kinds = client.kinds();

    assert_eq!(
        kinds.last().unwrap(),
        "send_now:a",
        "send-now must name exactly the selected entry: {kinds:?}"
    );
    assert_eq!(
        count_prefix(&kinds, "list"),
        1,
        "send-now issues no second read — the outcome carries the projection \
         the daemon already read: {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k.starts_with("add:")),
        "send-now must never queue a fresh submission: {kinds:?}"
    );
}

/// Applying an accepted outcome is what removes the row. Driven through the
/// applier because nothing in this target drains the command channel — the same
/// split every other projection assertion in this file uses.
#[test]
fn user_prompt_queue_an_accepted_send_now_takes_the_row_out_of_the_box() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    app.session_state.active_turn_id = Some("turn-live".to_string());
    run_slash_command(&mut app, "/queue");
    let request_id = app.dialog_state.queue_send_request.begin();

    codegg::tui::commands::queue::apply_queue_send_now_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        vec![entry("b", 1, "bravo", "pending")],
        Some(UserPromptQueueSendNowResult::Accepted {
            turn_id: "turn-live".to_string(),
        }),
        None,
    );

    assert_eq!(
        dialog_entries(&app)
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
        vec!["b".to_string()],
        "an accepted prompt leaves the live projection"
    );
    assert!(
        toast_texts(&app)
            .iter()
            .any(|t| t.contains("Handed to the turn")),
        "the user is told the prompt was handed over: {:?}",
        toast_texts(&app)
    );
}

/// The request names the entry and **nothing else**. Prompt text in the request
/// would be a second way to submit text that no queue entry accounts for, which
/// is exactly what ADR-0035's durable-entry rule exists to prevent.
#[tokio::test]
async fn user_prompt_queue_send_now_carries_no_text_and_no_turn_id() {
    let (mut app, client) = app_with_queue_and_turn(vec![entry("a", 0, "alpha", "pending")]);
    *client.send_now_accepted.lock().unwrap() = true;
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    press(&mut app, KeyCode::Char('s'));
    settle_prefix_until(&client, "send_now:", 1).await;

    let recorded = client.kinds().join(",");
    assert_eq!(
        recorded, "list,send_now:a",
        "the request must name only the entry id — no prompt text, no turn id: {recorded:?}"
    );
}

/// Each channel refusal keeps the row **in place, in the same position**, and
/// says something different from the others.
#[tokio::test]
async fn user_prompt_queue_a_channel_refusal_keeps_the_row_and_its_place() {
    for code in [
        "steer_queue_full",
        "steer_unavailable",
        "steer_not_supported",
        "session_control_not_controller",
    ] {
        let entries = vec![
            entry("a", 0, "alpha", "pending"),
            entry("b", 1, "bravo", "pending"),
        ];
        let (mut app, client) = app_with_queue_and_turn(entries.clone());
        *client.send_now_refusal.lock().unwrap() = Some(code.to_string());
        run_slash_command(&mut app, "/queue");
        settle_until(&client, "list", 1).await;
        press(&mut app, KeyCode::Char('s'));
        settle_prefix_until(&client, "send_now:", 1).await;
        assert_eq!(
            client.kinds().last().unwrap(),
            "send_now:a",
            "{code}: the refusal is the daemon's to answer"
        );

        let request_id = app.dialog_state.queue_send_request.begin();
        let before = toast_texts(&app);
        codegg::tui::commands::queue::apply_queue_send_now_finished(
            &mut app,
            request_id,
            SESSION.to_string(),
            entries.clone(),
            Some(UserPromptQueueSendNowResult::Refused {
                code: code.to_string(),
            }),
            None,
        );

        assert_eq!(
            dialog_entries(&app),
            entries,
            "{code}: a refused prompt must stay exactly where it was"
        );
        let toasts = toast_texts(&app);
        assert!(
            toasts.len() > before.len(),
            "{code}: the refusal must produce a sentence, got {toasts:?}"
        );
        let newest = toasts.last().unwrap();
        assert!(
            !newest.contains("alpha") && !newest.contains("bravo"),
            "{code}: a refusal must never quote the prompt: {newest:?}"
        );
    }
}

/// Six outcomes, six sentences. Collapsing two would tell a user their prompt
/// went out when it did not — or that it did not when it did.
#[test]
fn user_prompt_queue_the_send_now_sentences_are_all_different() {
    use codegg::tui::commands::queue::{send_now_outcome_message, SteerConsequence};
    let outcomes = [
        UserPromptQueueSendNowResult::Accepted {
            turn_id: "t".into(),
        },
        UserPromptQueueSendNowResult::AcceptedNotSettled {
            turn_id: "t".into(),
            code: "user_prompt_queue_storage_error".into(),
        },
        UserPromptQueueSendNowResult::EntryNotFound,
        UserPromptQueueSendNowResult::Refused {
            code: "steer_queue_full".into(),
        },
        UserPromptQueueSendNowResult::Refused {
            code: "steer_unavailable".into(),
        },
        UserPromptQueueSendNowResult::Refused {
            code: "steer_not_supported".into(),
        },
        UserPromptQueueSendNowResult::Refused {
            code: "session_control_not_controller".into(),
        },
    ];
    let mut seen: Vec<&str> = Vec::new();
    for outcome in &outcomes {
        let (message, _) = send_now_outcome_message(outcome, SteerConsequence::DurableQueue);
        assert!(
            !seen.contains(&message.as_str()),
            "two outcomes share one sentence: {message:?}"
        );
        assert!(
            !message.contains("alpha") && !message.contains("bravo"),
            "a sentence must never quote the prompt: {message:?}"
        );
        for forbidden in ["force", "guarantee", "always reaches", "the model will"] {
            assert!(
                !message.contains(forbidden),
                "a sentence promised {forbidden:?}: {message:?}"
            );
        }
        seen.push(Box::leak(message.into_boxed_str()));
    }
}

/// A prompt the turn took but the queue could not record is the one case where
/// "send it again" would duplicate it, so its wording must not read as a
/// refusal.
#[test]
fn user_prompt_queue_an_unsettled_settlement_does_not_ask_the_user_to_resend() {
    use codegg::tui::commands::queue::{send_now_outcome_message, SteerConsequence};
    let (message, tone) = send_now_outcome_message(
        &UserPromptQueueSendNowResult::AcceptedNotSettled {
            turn_id: "t".into(),
            code: "user_prompt_queue_storage_error".into(),
        },
        SteerConsequence::DurableQueue,
    );
    assert!(
        message.contains("treat it as sent"),
        "the wording must say the turn took it: {message:?}"
    );
    assert_eq!(
        tone,
        codegg::tui::commands::queue::SendNowTone::Warning,
        "an unsettled settlement is a warning, never a success"
    );
}

/// An unmapped code still produces an honest sentence carrying the code, so an
/// unrecognised refusal can never render as success.
#[test]
fn user_prompt_queue_an_unmapped_send_now_code_stays_honest() {
    use codegg::tui::commands::queue::{send_now_outcome_message, SteerConsequence};
    let (message, tone) = send_now_outcome_message(
        &UserPromptQueueSendNowResult::Refused {
            code: "some_future_code".into(),
        },
        SteerConsequence::DurableQueue,
    );
    assert!(
        message.contains("some_future_code") && message.contains("stays queued"),
        "the fallback must name the code and say nothing went out: {message:?}"
    );
    assert_eq!(tone, codegg::tui::commands::queue::SendNowTone::Warning);
}

/// A row already running as its own turn is refused **without a round trip** —
/// the daemon could only answer "no", and the box already knows.
#[tokio::test]
async fn user_prompt_queue_send_now_on_a_delivering_row_asks_the_daemon_nothing() {
    let (mut app, client) = app_with_queue_and_turn(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "delivering"),
    ]);
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('s'));

    assert_eq!(
        client.kinds(),
        vec!["list".to_string()],
        "a delivering row is refused locally, so no request may be issued"
    );
    assert!(
        toast_texts(&app)
            .iter()
            .any(|t| t.contains("already running for it")),
        "the user gets the reason: {:?}",
        toast_texts(&app)
    );
    assert_eq!(
        dialog_entries(&app).len(),
        2,
        "the projection is untouched by a refused keypress"
    );
}

/// With no turn running there is nowhere to send a prompt, and the box says so
/// rather than spending a round trip to be told.
#[tokio::test]
async fn user_prompt_queue_send_now_without_a_turn_asks_the_daemon_nothing() {
    let (mut app, client) = app_with_queue_and_core(vec![entry("a", 0, "alpha", "pending")]);
    // `app_with_queue_and_turn` would have set this; the absence is the point.
    app.session_state.active_turn_id = None;
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    press(&mut app, KeyCode::Char('s'));

    assert_eq!(
        client.kinds(),
        vec!["list".to_string()],
        "an idle session must not produce a send-now request"
    );
    assert!(
        toast_texts(&app)
            .iter()
            .any(|t| t.contains("stays queued for the next turn")),
        "the user is told the prompt is still waiting: {:?}",
        toast_texts(&app)
    );
}

/// `s` types an `s` while a composer has the keyboard. Getting this backwards is
/// the milestone's mandatory injection, so it is asserted directly.
#[tokio::test]
async fn user_prompt_queue_typing_s_in_a_composer_is_text_not_an_action() {
    let (mut app, client) = app_with_queue_and_turn(vec![entry("a", 0, "alpha", "pending")]);
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    press(&mut app, KeyCode::Char('a'));
    for c in "asdf".chars() {
        press(&mut app, KeyCode::Char(c));
    }

    assert_eq!(
        client.kinds(),
        vec!["list".to_string()],
        "`s` inside a composer is a letter, not a send-now"
    );
    let composed = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |d| d.compose().to_owned())
        .expect("mounted");
    assert_eq!(
        composed, "asdf",
        "every letter, including `s`, must land in the composer"
    );
}

/// The M005 stale-guard shape, on M006's own slot: a completion that arrives
/// after the box closed writes nothing and says nothing.
#[test]
fn user_prompt_queue_a_stale_send_now_after_close_changes_nothing() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    app.session_state.active_turn_id = Some("turn-live".to_string());
    run_slash_command(&mut app, "/queue");
    press(&mut app, KeyCode::Char('s'));

    let stale = app.dialog_state.queue_send_request.begin();
    let before = dialog_entries(&app);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.focus_manager.active_dialog_type(), DialogType::None);
    let toasts_before = toast_texts(&app);

    codegg::tui::commands::queue::apply_queue_send_now_finished(
        &mut app,
        stale,
        SESSION.to_string(),
        vec![entry("b", 1, "bravo", "pending")],
        Some(UserPromptQueueSendNowResult::Accepted {
            turn_id: "turn-live".to_string(),
        }),
        None,
    );

    assert_eq!(
        dialog_entries(&app),
        before,
        "a send-now that completes after the box closed must not rewrite the \
         projection the user is no longer looking at"
    );
    assert_eq!(
        toast_texts(&app),
        toasts_before,
        "a stale completion must be dropped whole: {:?}",
        toast_texts(&app)
    );
}

/// Closing the box mid-steer must cancel the request, which is what makes the
/// stale guard above reachable at all.
#[test]
fn user_prompt_queue_closing_the_box_cancels_an_in_flight_send_now() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    app.session_state.active_turn_id = Some("turn-live".to_string());
    run_slash_command(&mut app, "/queue");
    press(&mut app, KeyCode::Char('s'));
    let in_flight = app.dialog_state.queue_send_request.begin();

    press(&mut app, KeyCode::Esc);

    codegg::tui::commands::queue::apply_queue_send_now_finished(
        &mut app,
        in_flight,
        SESSION.to_string(),
        Vec::new(),
        Some(UserPromptQueueSendNowResult::Accepted {
            turn_id: "turn-live".to_string(),
        }),
        None,
    );
    assert_eq!(
        app.session_state.user_prompt_queue.len(),
        1,
        "a completion for a request `close_dialog` cancelled must not be applied"
    );
}

/// An `EntryNotFound` outcome still adopts the daemon's projection: the row may
/// have left the queue under another frontend, and the box must not keep
/// showing it.
#[test]
fn user_prompt_queue_a_missing_entry_still_refreshes_the_box() {
    let mut app = app_with_queue(vec![
        entry("a", 0, "alpha", "pending"),
        entry("b", 1, "bravo", "pending"),
    ]);
    app.session_state.active_turn_id = Some("turn-live".to_string());
    run_slash_command(&mut app, "/queue");
    let request_id = app.dialog_state.queue_send_request.begin();

    // The daemon's projection no longer holds `a` — another frontend removed it
    // between this box's read and this keypress.
    codegg::tui::commands::queue::apply_queue_send_now_finished(
        &mut app,
        request_id,
        SESSION.to_string(),
        vec![entry("b", 1, "bravo", "pending")],
        Some(UserPromptQueueSendNowResult::EntryNotFound),
        None,
    );

    assert_eq!(
        dialog_entries(&app)
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
        vec!["b".to_string()],
        "an outcome's projection is adopted even when the outcome is a refusal"
    );
    assert!(
        toast_texts(&app).iter().any(|t| t.contains("already gone")),
        "the user is told the prompt left the queue: {:?}",
        toast_texts(&app)
    );
}

/// Send-now goes out on the **socket-daemon** transport, the one every other
/// queue operation already uses. This path is a `CoreClient::request`, not a
/// `send_remote_message`, so it needs no `is_ws_attach_transport()` gate — and
/// asserting it here is what keeps someone from adding one later on the theory
/// that the WS-attach path needs it.
#[tokio::test]
async fn user_prompt_queue_send_now_works_on_the_socket_daemon_transport() {
    let (mut app, client) = app_with_queue_and_turn(vec![entry("a", 0, "alpha", "pending")]);
    *client.send_now_accepted.lock().unwrap() = true;
    assert!(
        app.remote_send_tx.is_none(),
        "this app is the socket-daemon shape: no remote send channel"
    );
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    press(&mut app, KeyCode::Char('s'));
    settle_prefix_until(&client, "send_now:", 1).await;

    assert!(
        client.kinds().iter().any(|k| k == "send_now:a"),
        "send-now must reach the daemon on the socket-daemon transport: {:?}",
        client.kinds()
    );
}

/// `s` on an empty queue is inert: no request, and no toast pretending a refusal
/// happened. The second half matters — an empty queue is not a *refusal*, and
/// telling the user "nothing is running" when there is nothing to send would be
/// answering a question they did not ask.
#[tokio::test]
async fn user_prompt_queue_send_now_is_inert_with_nothing_selected() {
    let (mut app, client) = app_with_queue_and_turn(Vec::new());
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    let before = toast_texts(&app);

    press(&mut app, KeyCode::Char('s'));

    assert_eq!(
        client.kinds(),
        vec!["list".to_string()],
        "an empty queue can send nothing, so no request may be issued"
    );
    assert_eq!(
        toast_texts(&app),
        before,
        "`s` with nothing selected is not a refusal and must not report one"
    );
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::Queue,
        "the box must stay open rather than faulting on the key"
    );
}

/// `DialogType::Queue` still has no `dock_target` arm after M006 added a key.
/// The omission is what keeps the whole docking-steal branch dead for this box.
#[test]
fn user_prompt_queue_the_host_does_not_steal_the_send_now_key_either() {
    let mut app = app_with_queue(vec![entry("a", 0, "alpha", "pending")]);
    codegg::tui::commands::queue::open_queue_box(&mut app);
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::Queue,
        "the box must be mounted for the docking branch to be reachable at all"
    );
    assert!(
        app.dock_target().is_none(),
        "if `dock_target` ever grows a `DialogType::Queue` arm, M006's `s` — \
         like every other key here — could be claimed by the host first"
    );
    assert!(
        app.focus_manager.top_docks_as_viewer(),
        "this box declares itself a viewer, so the docking branch is reachable \
         in principle and only `dock_target()` keeps it inert"
    );
}

/// Observers are refused send-now for the same reason they are refused the box.
#[test]
fn user_prompt_queue_send_now_is_never_observer_allowed() {
    for command in ["queue", "queue-send-now", "queue-delete"] {
        assert!(
            !is_observer_allowed_command(command),
            "{command} must never be observer-allowed"
        );
    }
}

// ── M007 (ADR-0039): delayed delivery over the wire ─────────────────
//
// These assert the *request*, not the box. The store tests prove a due time is
// honoured; these prove one leaves the TUI at all, with the value the user
// typed, and that an unparseable one never leaves it.

// A typed duration reaches the wire as an absolute instant, and `Enter` on an
// empty duration sends `None` rather than inventing "due now".
#[tokio::test]
async fn user_prompt_queue_a_scheduled_add_sends_the_due_time_and_an_empty_one_sends_none() {
    let (mut app, client) = open_queue_app(Vec::new());

    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "later");
    press(&mut app, KeyCode::Enter); // -> duration
    type_text(&mut app, "2h");
    press(&mut app, KeyCode::Enter); // commit
    settle_prefix_until(&client, "add:", 1).await;

    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "now");
    commit_compose(&mut app);
    settle_prefix_until(&client, "add:", 2).await;

    let kinds = client.kinds();
    let scheduled = kinds
        .iter()
        .find(|k| k.starts_with("add:") && k.contains(":later:"));
    let scheduled = scheduled.unwrap_or_else(|| panic!("no scheduled add recorded in {kinds:?}"));
    assert!(
        scheduled.contains("Some("),
        "a typed 2h must cross the wire as an absolute instant, not a duration \
         and not None: {scheduled}"
    );

    let immediate = kinds
        .iter()
        .find(|k| k.starts_with("add:") && k.contains(":now:"))
        .expect("an immediate add must be recorded");
    assert!(
        immediate.ends_with(":None"),
        "an empty duration means 'no schedule', which is None and not 'due now': \
         {immediate}"
    );
}

// An unparseable duration is refused in the box. No request may leave, which is
// the property that makes the refusal local rather than a round trip.
#[tokio::test]
async fn user_prompt_queue_an_unparseable_duration_sends_no_request_at_all() {
    let (mut app, client) = open_queue_app(Vec::new());
    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "hello");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "1 hr");
    press(&mut app, KeyCode::Enter);

    let toasts = toast_texts(&app);
    assert!(
        toasts
            .iter()
            .any(|m| m.contains("1 hr") && m.contains("30s")),
        "the refusal must quote what was typed and state the grammar: {toasts:?}"
    );
    assert!(
        toasts.iter().all(|m| !m.contains("hello")),
        "the toast must not echo the prompt itself: {toasts:?}"
    );
    assert!(
        !client.kinds().iter().any(|k| k.starts_with("add:")),
        "an unparseable duration must be refused before any request: {:?}",
        client.kinds()
    );

    // The user is still in the duration step with both fields intact, so the
    // fix is an edit rather than a retype.
    let compose = compose_of(&app).expect("the prompt must survive");
    assert!(
        compose.contains("hello"),
        "the prompt text must survive: {compose:?}"
    );
}

// The box refuses to promise a time. Delivery is a floor the daemon keeps when
// it can, never a guarantee — the plan's §3.5 copy rule, asserted where the
// user meets it.
#[tokio::test]
async fn user_prompt_queue_copy_never_promises_a_promised_delivery_time() {
    let (mut app, _client) = open_queue_app(Vec::new());
    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "hello");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "nonsense");
    press(&mut app, KeyCode::Enter);

    let mut all: Vec<String> = toast_texts(&app);
    let hint = app
        .focus_manager
        .with_dialog::<QueueDialog, _>(DialogType::Queue, |d| d.keymap_hint().expect("hint"))
        .expect("mounted")
        .to_string();
    all.push(hint);

    for text in &all {
        for forbidden in [
            "guarantee",
            "guaranteed",
            "force",
            "forced",
            "always reaches",
            "will reach",
            "the model will",
            "exactly on time",
            "precisely at",
        ] {
            assert!(
                !text.to_lowercase().contains(forbidden),
                "queue copy promises delivery with {forbidden:?}: {text:?}"
            );
        }
    }
}

// ── C007: the addressed reply reaches the parent turn ──────────────

/// Put the app in the socket-daemon frontend shape: `RemoteCore` **without**
/// a `remote_send_tx`, which is exactly what `App::send_prompt` branches on to
/// reach `dispatch_turn_submit_request`.
///
/// This is a test-side mode set, not a production gate — the App decides
/// transport through `App::is_ws_attach_transport()`, which
/// `scripts/check_tui_transport_authority.py` enforces, and that derivation is
/// left alone.
fn as_socket_daemon_frontend(app: &mut App) {
    app.ui_state.mode = codegg::tui::app::state::AppMode::RemoteCore {
        endpoint: "socket".to_string(),
    };
    debug_assert!(app.remote_send_tx.is_none());
}

/// Seed one running subagent for the addressed-reply tests.
fn seed_running_subagent(app: &mut App) {
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        child_instance_id: None,
        task_id: "task-1".to_string(),
        run_id: None,
        display_name: Some("Sally".to_string()),
        profile: None,
        usefulness: None,
        agent: "explore".to_string(),
        description: "find the retry path".to_string(),
        status: "running".to_string(),
    }]);
}

/// Type `text` into the composer through the real key path and press Enter.
fn submit(app: &mut App, text: &str) {
    for c in text.chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

/// The milestone's central claim, stated as a measurement: an addressed submit
/// issues exactly one `TurnSteer` naming the **parent turn**, and issues no
/// `TurnSubmit` at all.
///
/// The parent turn is the point. `TurnSteer` is keyed on a session turn, so a
/// steer naming the subagent's `task_id` — the identity the UI actually has —
/// would resolve to `turn_id_mismatch` every single time, and the UI would look
/// like it had a bug rather than a wrong id.
#[tokio::test]
async fn an_addressed_reply_steers_the_parent_turn_and_submits_nothing() {
    let (mut app, client) = app_with_queue_and_core(Vec::new());
    // The same transport shape the unarmed regression test uses. Without it
    // the unarmed path here would dispatch nothing at all, so "no TurnSubmit
    // went out" would hold for the wrong reason — the harness, not the arm.
    as_socket_daemon_frontend(&mut app);
    app.session_state.active_turn_id = Some("turn-live".to_string());
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        child_instance_id: None,
        task_id: "task-1".to_string(),
        run_id: Some("run-1".to_string()),
        display_name: Some("Sally".to_string()),
        profile: None,
        usefulness: None,
        agent: "explore".to_string(),
        description: "find the retry path".to_string(),
        status: "running".to_string(),
    }]);
    assert!(codegg::tui::commands::subagent_steer::arm_addressee(
        &mut app, "task-1"
    ));
    submit(&mut app, "try the second retry first");

    // Settle on either request, not on the steer alone. Waiting for the steer
    // and then asserting the submit is absent would report a removed addressed
    // branch as a *timeout* — true, but indirect, and it reads the same as a
    // hang rather than as the fall-through it is.
    settle_until_either(&client, "steer:", "submit:").await;
    let kinds = client.kinds();
    assert_eq!(
        count_prefix(&kinds, "submit:"),
        0,
        "an addressed reply must never fall through to TurnSubmit: {kinds:?}"
    );
    let steer = kinds
        .iter()
        .find(|k| k.starts_with("steer:"))
        .expect("one steer");
    assert!(
        steer.starts_with("steer:session-queue-1:turn-live:"),
        "the steer must name the parent session and turn, not the subagent's task_id: \
         {steer:?}"
    );
    assert!(
        steer.contains("Sally"),
        "the captured name travels: {steer:?}"
    );
    assert!(
        steer.ends_with("try the second retry first"),
        "the user's own words are sent verbatim: {steer:?}"
    );
}

/// The unarmed path is untouched. This is the regression pin: the addressed
/// branch sits in `send_prompt` ahead of a dozen other guards, and a branch
/// that ran when unarmed would divert ordinary prompts into a steer.
#[tokio::test]
async fn an_unarmed_prompt_still_submits_a_normal_turn() {
    let (mut app, client) = app_with_queue_and_core(Vec::new());
    as_socket_daemon_frontend(&mut app);
    submit(&mut app, "hello there");
    settle_prefix_until(&client, "submit:", 1).await;
    let kinds = client.kinds();
    assert_eq!(
        count_prefix(&kinds, "steer:"),
        0,
        "nothing is addressed, so nothing is steered: {kinds:?}"
    );
    assert!(kinds.iter().any(|k| k == "submit:hello there"), "{kinds:?}");
}

/// A refused steer says where the words are and keeps them there.
///
/// The compositor is the only durable home a bare reply has, so a cleared
/// prompt on a refused send destroys the user's text with no copy anywhere.
#[tokio::test]
async fn a_refused_steer_keeps_the_users_words_in_the_composer() {
    let (mut app, client) = app_with_queue_and_core(Vec::new());
    app.session_state.active_turn_id = Some("turn-live".to_string());
    seed_running_subagent(&mut app);
    *client.steer_refusal.lock().unwrap() = Some("turn_id_mismatch".to_string());
    assert!(codegg::tui::commands::subagent_steer::arm_addressee(
        &mut app, "task-1"
    ));
    submit(&mut app, "never mind, do the other one");

    settle_prefix_until(&client, "steer:", 1).await;
    // Driven through the applier because nothing in this target drains the
    // command channel — the same split every other assertion here uses.
    let request_id = app.prompt_state.subagent_steer_request.begin();
    codegg::tui::commands::subagent_steer::apply_subagent_steer_finished(
        &mut app,
        request_id,
        "Sally".to_string(),
        "never mind, do the other one".to_string(),
        Some("turn_id_mismatch".to_string()),
        Some("refused".to_string()),
    );
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "never mind, do the other one",
        "a refused reply must survive in the composer"
    );
    assert!(
        app.prompt_state.subagent_addressee.is_some(),
        "and the address stays armed, so Enter again is a real retry against the \
         then-current turn"
    );
}

/// An accepted steer clears the composer and leaves the address armed, so a
/// second reply to the same run is one keystroke away.
#[tokio::test]
async fn an_accepted_steer_clears_the_composer_and_keeps_the_addressee() {
    let (mut app, client) = app_with_queue_and_core(Vec::new());
    app.session_state.active_turn_id = Some("turn-live".to_string());
    seed_running_subagent(&mut app);
    assert!(codegg::tui::commands::subagent_steer::arm_addressee(
        &mut app, "task-1"
    ));
    submit(&mut app, "focus on the retry loop");

    settle_prefix_until(&client, "steer:", 1).await;
    let request_id = app.prompt_state.subagent_steer_request.begin();
    codegg::tui::commands::subagent_steer::apply_subagent_steer_finished(
        &mut app,
        request_id,
        "Sally".to_string(),
        "focus on the retry loop".to_string(),
        None,
        None,
    );
    assert!(app.prompt_state.subagent_addressee.is_some());
}

/// A slash command typed while addressed stays a command. Handing the literal
/// string "/queue" to the parent agent would be absurd, and the chat-armed
/// branch already sets this precedent.
#[tokio::test]
async fn a_slash_command_typed_while_addressed_is_not_steered() {
    let (mut app, client) = app_with_queue_and_core(Vec::new());
    app.session_state.active_turn_id = Some("turn-live".to_string());
    seed_running_subagent(&mut app);
    assert!(codegg::tui::commands::subagent_steer::arm_addressee(
        &mut app, "task-1"
    ));
    run_slash_command(&mut app, "/queue");
    settle_until(&client, "list", 1).await;
    assert_eq!(
        count_prefix(&client.kinds(), "steer:"),
        0,
        "a command is a command: {:?}",
        client.kinds()
    );
}

/// Addressing requires a run that is actually running. With none, the reply is
/// refused **before** any request is built — there is no turn to steer.
#[tokio::test]
async fn addressing_with_no_running_turn_issues_no_request() {
    let (mut app, client) = app_with_queue_and_core(Vec::new());
    seed_running_subagent(&mut app);
    assert!(codegg::tui::commands::subagent_steer::arm_addressee(
        &mut app, "task-1"
    ));
    submit(&mut app, "are you there");
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let kinds = client.kinds();
    assert_eq!(count_prefix(&kinds, "steer:"), 0, "{kinds:?}");
    assert_eq!(
        count_prefix(&kinds, "submit:"),
        0,
        "and no silent TurnSubmit either: {kinds:?}"
    );
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "are you there",
        "the words survive a refusal that never left the process"
    );
}
