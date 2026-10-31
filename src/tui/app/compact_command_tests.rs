//! The `/compact` command's truthfulness (C001, re-cut for C002).
//!
//! C001 stopped the command claiming a compaction it did not cause. C002
//! (ADR-0060) gives it a **sender**: it asks the daemon to compact, and it
//! reports the daemon's answer — a refusal included, as a refusal.

use super::App;
use crate::core::CoreClient;
use crate::error::AppError;
use codegg_protocol::core::{
    CommandExecuteResultDto, CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use std::sync::Mutex;
use tokio::sync::mpsc;

fn dispatch(app: &mut App, query: &str) {
    let name = query
        .split_whitespace()
        .next()
        .unwrap()
        .trim_start_matches('/');
    let cmd = app
        .command_registry
        .find_by_name_or_alias(name)
        .expect("command must exist in registry")
        .clone();
    app.dialog_state.command_palette.query = query.to_string();
    app.execute_command(&cmd, Some(query));
}

fn last_toast(app: &App) -> String {
    app.messages_state
        .toasts
        .iter()
        .last()
        .map(|toast| toast.message.clone())
        .unwrap_or_default()
}

fn bound_session(id: &str) -> crate::session::Session {
    crate::session::Session {
        id: id.into(),
        project_id: String::new(),
        workspace_id: None,
        parent_id: None,
        slug: String::new(),
        directory: String::new(),
        title: String::new(),
        version: String::new(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        revert: None,
        permission: None,
        tags: Vec::new(),
        provider_connection_id: None,
        provider_connection_revision: None,
        model_catalog_revision: None,
        selected_model_id: None,
        agent: None,
        model: None,
        time_created: 0,
        time_updated: 0,
        time_compacting: None,
        time_archived: None,
        time_deleted: None,
    }
}

/// A daemon that records what the TUI asked and answers with a chosen response.
#[derive(Default)]
struct RecordingDaemon {
    requests: Mutex<Vec<CoreRequest>>,
    answer: Mutex<Option<CoreResponse>>,
}

impl RecordingDaemon {
    fn answer_with(&self, response: CoreResponse) {
        *self.answer.lock().unwrap() = Some(response);
    }

    fn requests(&self) -> Vec<CoreRequest> {
        self.requests.lock().unwrap().clone()
    }
}

/// The daemon's answer, in the shape its executor produces.
fn daemon_compaction_answer(session_id: &str, turn_active: bool) -> CoreResponse {
    CoreResponse::CommandExecute {
        result: CommandExecuteResultDto {
            run_id: "run-1".to_string(),
            command_id: "compact".to_string(),
            status: codegg_protocol::core::CommandOutcomeStatusDto::Completed,
            summary: "ok".to_string(),
            lines: Vec::new(),
            report: None,
            started_at_ms: 0,
            completed_at_ms: 1,
            response: Some(serde_json::json!({
                "type": "session_compact",
                "session_id": session_id,
                "turn_active": turn_active,
            })),
        },
    }
}

#[async_trait::async_trait]
impl CoreClient for RecordingDaemon {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        self.requests.lock().unwrap().push(request.payload);
        Ok(self
            .answer
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(CoreResponse::Ack))
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        rx
    }
}

/// An app bound to the recording daemon, with the command channel wired.
fn app_with_daemon(daemon: std::sync::Arc<RecordingDaemon>) -> App {
    let mut app = App::new_for_testing("/tmp".into());
    app.session_state.session = Some(bound_session("s-1"));
    app.set_core_client(daemon);
    let (tx, rx) = mpsc::channel(8);
    app.tui_cmd_tx = Some(tx);
    app.tui_cmd_rx = Some(rx);
    app
}

/// Wait until the TUI has asked the daemon for something.
async fn wait_for_request(daemon: &RecordingDaemon) {
    for _ in 0..200 {
        if !daemon.requests().is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the TUI did not ask the daemon for a compaction");
}

/// Wait for the daemon's answer to come back as a completion.
async fn wait_for_completion(app: &mut App) -> crate::tui::app::TuiCommand {
    let rx = app
        .tui_cmd_rx
        .as_mut()
        .expect("the app must carry the command channel");
    tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("the daemon's answer must arrive")
        .expect("the completion must not be dropped")
}

/// The command asks the daemon's **own** catalog command to compact the
/// session the user is looking at (ADR-0052).
#[tokio::test]
async fn the_compact_command_asks_the_daemon_to_compact_the_session() {
    let daemon = std::sync::Arc::new(RecordingDaemon::default());
    let mut app = app_with_daemon(daemon.clone());
    dispatch(&mut app, "/compact");
    wait_for_request(&daemon).await;

    let requests = daemon.requests();
    let Some(CoreRequest::CommandExecute { request }) = requests.first() else {
        panic!("the command must reach the daemon as a catalog command: {requests:?}");
    };
    assert_eq!(
        request.command_id, "compact",
        "the daemon's own catalog command must be the one asked for"
    );
    assert_eq!(
        request.scope.session_id.as_deref(),
        Some("s-1"),
        "the request must name the session it compacts"
    );
}

/// The toast reports the daemon's answer, never a compaction of its own.
#[tokio::test]
async fn the_compact_command_reports_the_daemons_answer() {
    let daemon = std::sync::Arc::new(RecordingDaemon::default());
    daemon.answer_with(daemon_compaction_answer("s-1", false));
    let mut app = app_with_daemon(daemon.clone());
    app.session_state.compaction_count = 3;
    dispatch(&mut app, "/compact");
    let completion = wait_for_completion(&mut app).await;
    crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, completion).await;

    let toast = last_toast(&app);
    assert!(
        toast.contains("Compaction requested"),
        "the command must report that it asked: {toast:?}"
    );
    assert!(
        !toast.contains("Compaction triggered"),
        "the command must not claim it triggered a compaction: {toast:?}"
    );
    assert!(
        toast.contains("at the start of the next turn"),
        "the command must say when the daemon compacts: {toast:?}"
    );
    assert!(
        toast.contains('3'),
        "the command must report the count the TUI keeps: {toast:?}"
    );
}

/// A live turn is named as such — and what happens if that turn ends first
/// is named too.
#[tokio::test]
async fn the_compact_command_names_the_running_turn_when_the_daemon_says_so() {
    let daemon = std::sync::Arc::new(RecordingDaemon::default());
    daemon.answer_with(daemon_compaction_answer("s-1", true));
    let mut app = app_with_daemon(daemon.clone());
    dispatch(&mut app, "/compact");
    let completion = wait_for_completion(&mut app).await;
    crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, completion).await;

    let toast = last_toast(&app);
    assert!(
        toast.contains("the running turn's next compaction opportunity"),
        "the command must name the live turn's opportunity: {toast:?}"
    );
    assert!(
        toast.contains("if that turn ends first"),
        "the command must not predict that the live turn continues: {toast:?}"
    );
    assert!(
        !toast.contains("Compaction triggered"),
        "the command must not claim it triggered a compaction: {toast:?}"
    );
}

/// A refusal is reported as a refusal.
#[tokio::test]
async fn a_refused_compaction_request_is_reported_as_a_refusal() {
    let daemon = std::sync::Arc::new(RecordingDaemon::default());
    daemon.answer_with(CoreResponse::Error {
        code: "session_unavailable".to_string(),
        message: "session not found: s-9".to_string(),
    });
    let mut app = app_with_daemon(daemon.clone());
    dispatch(&mut app, "/compact");
    let completion = wait_for_completion(&mut app).await;
    crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, completion).await;

    let toast = last_toast(&app);
    assert!(
        toast.contains("refused"),
        "the daemon's refusal must be reported as a refusal: {toast:?}"
    );
    assert!(
        !toast.contains("Compaction requested"),
        "a refusal must not read as a request that was accepted: {toast:?}"
    );
}

/// An answer about another session is not an answer about this one.
///
/// The daemon's answer names the session it compacts, so a frontend that
/// asked about `s-1` never reports a compaction for another session.
#[tokio::test]
async fn an_answer_about_another_session_is_not_reported_as_this_sessions_compaction() {
    let daemon = std::sync::Arc::new(RecordingDaemon::default());
    daemon.answer_with(daemon_compaction_answer("s-other", false));
    let mut app = app_with_daemon(daemon.clone());
    dispatch(&mut app, "/compact");
    let completion = wait_for_completion(&mut app).await;
    crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, completion).await;

    let toast = last_toast(&app);
    assert!(
        toast.contains("another session"),
        "the command must say the answer was about another session: {toast:?}"
    );
    assert!(
        !toast.contains("Compaction requested"),
        "the command must not report a compaction it did not ask for: {toast:?}"
    );
}

/// With no session attached there is nothing to compact, and the command says
/// so instead of asking.
#[test]
fn the_compact_command_says_so_when_no_session_is_attached() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch(&mut app, "/compact");
    let toast = last_toast(&app);
    assert!(
        toast.contains("No session is attached"),
        "the command must say why it did nothing: {toast:?}"
    );
}

/// The compaction form is the daemon's, and the daemon serves it: a
/// frontend action would be a **local** effect, so none serves it.
#[test]
fn the_compact_form_is_a_daemon_effect_the_daemon_serves() {
    let descriptor = codegg_core::command_catalog::find_by_command_id("compact")
        .expect("the catalog must carry the compact command");
    assert_eq!(
        descriptor.kind,
        codegg_core::command_catalog::CommandKind::DaemonEffect,
        "the catalog must classify the form as a daemon effect"
    );
    assert_eq!(
        descriptor.capability,
        Some(codegg_core::command_catalog::CommandCapability::CommandExecute),
        "the daemon executes the form under `command.execute`"
    );
    assert_eq!(
        codegg_core::frontend_action::action_for_form("compact", ""),
        None,
        "no frontend action serves a daemon-owned compaction"
    );
    assert!(
        matches!(
            crate::tui::command_route::route_for("compact", ""),
            crate::tui::command_route::CommandRoute::DaemonEffect { .. }
        ),
        "the TUI's route must reach the daemon"
    );
}
