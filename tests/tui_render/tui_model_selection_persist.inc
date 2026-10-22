//! `/model` selection must reach the daemon's durable session selection.
//!
//! The daemon resolves every turn from its durable `SessionSelection`,
//! not from what the TUI displays. The `/model` dialog used to mutate
//! `App::agent_state` and nothing else, so committing a choice changed
//! the status line while the next turn still ran whatever the daemon had
//! stored. That is the divergence behind the live report "I selected
//! mimo 2.6 flash and it ran muse-spark 1.3".
//!
//! These tests drive the real dialog-commit path against a fake
//! `CoreClient` and assert the daemon was actually told, and that a
//! refusal is reported instead of being displayed as a success.

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::tui::app::{App, TuiCommand, TuiMsg};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Records every `ModelSelect` the TUI issues and replies with whatever
/// the test configured.
#[derive(Default)]
struct FakeSelectionDaemon {
    /// `(session_id, model)` for each `ModelSelect` received.
    received: Mutex<Vec<(String, String)>>,
    /// `Some(message)` makes the daemon refuse every selection.
    refuse_with: Mutex<Option<String>>,
}

#[async_trait]
impl CoreClient for FakeSelectionDaemon {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::ModelSelect { session_id, model } => {
                self.received.lock().unwrap().push((session_id, model));
                match self.refuse_with.lock().unwrap().clone() {
                    Some(message) => Ok(CoreResponse::Error {
                        code: "selection_rejected".to_string(),
                        message,
                    }),
                    None => Ok(CoreResponse::Ack),
                }
            }
            // `set_session` legitimately issues unrelated requests (e.g.
            // presence capabilities). They are recorded but not policed;
            // these tests assert specifically on `ModelSelect`.
            _ => Ok(CoreResponse::Ack),
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        rx
    }
}

/// An app bound to a session, wired to a fake daemon and a command
/// channel the test drains.
fn bound_app(daemon: Arc<FakeSelectionDaemon>) -> (App, mpsc::Receiver<TuiCommand>) {
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let mut app = App::new_for_testing(project_dir.path().to_string_lossy().to_string());
    app.core_client = Some(daemon);
    let (tx, rx) = mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);
    let session = fresh_session("sess-model-select");
    app.set_session(session);
    (app, rx)
}

/// An app with a fake daemon and a command channel, but **no** bound
/// session — the state of a restored manifest tab, which starts with
/// `session_id: null`.
fn unbound_app(daemon: Arc<FakeSelectionDaemon>) -> (App, mpsc::Receiver<TuiCommand>) {
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let mut app = App::new_for_testing(project_dir.path().to_string_lossy().to_string());
    app.core_client = Some(daemon);
    let (tx, rx) = mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);
    (app, rx)
}

/// A minimal bound session with no durable selection of its own.
fn fresh_session(id: &str) -> codegg::session::Session {
    codegg::session::Session {
        id: id.to_string(),
        project_id: "project-x".to_string(),
        workspace_id: Some("ws-x".to_string()),
        parent_id: None,
        slug: "x".to_string(),
        directory: "/tmp/model-select".to_string(),
        title: "Model select".to_string(),
        version: "v1".to_string(),
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

async fn drain(rx: &mut mpsc::Receiver<TuiCommand>) -> Vec<TuiCommand> {
    let mut out = Vec::new();
    while let Ok(cmd) = rx.try_recv() {
        out.push(cmd);
        // Let the spawned persistence task run and post its completion.
        tokio::task::yield_now().await;
        while let Ok(cmd) = rx.try_recv() {
            out.push(cmd);
            tokio::task::yield_now().await;
        }
    }
    out
}

/// Commit `model` exactly as the live `/model` dialog does: the focused
/// `ModelDialog` component turns Enter into `TuiMsg::SelectModel`, which
/// `process_msg` handles.
fn commit_model_via_dialog(app: &mut App, model: &str) {
    app.set_models(vec![model.to_string()]);
    // `TuiMsg::OpenModelDialog` is the app's own route to the picker.
    app.process_msg(TuiMsg::OpenModelDialog);
    // The focused `ModelDialog` component turns Enter into exactly this
    // message; `process_msg` is what receives it.
    app.process_msg(TuiMsg::SelectModel {
        model: model.to_string(),
    });
}

#[tokio::test(flavor = "current_thread")]
async fn committing_a_model_in_the_dialog_persists_it_to_the_daemon() {
    let daemon = Arc::new(FakeSelectionDaemon::default());
    let (mut app, mut rx) = bound_app(daemon.clone());

    commit_model_via_dialog(&mut app, "opencode_go/mimo-v2.6-flash");

    let _ = drain(&mut rx).await;

    let received = daemon.received.lock().unwrap().clone();
    assert_eq!(
        received,
        vec![(
            "sess-model-select".to_string(),
            "opencode_go/mimo-v2.6-flash".to_string()
        )],
        "the chosen model must be persisted to the daemon's durable selection"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_persisted_selection_reports_no_error_to_the_operator() {
    let daemon = Arc::new(FakeSelectionDaemon::default());
    let (mut app, mut rx) = bound_app(daemon.clone());

    commit_model_via_dialog(&mut app, "opencode_go/mimo-v2.6-flash");
    let commands = drain(&mut rx).await;

    let completions: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            TuiCommand::ModelSelectPersisted { model, error } => {
                Some((model.clone(), error.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        completions.len(),
        1,
        "expected one completion, got {commands:?}"
    );
    let (model, error) = &completions[0];
    assert_eq!(model, "opencode_go/mimo-v2.6-flash");
    assert_eq!(
        error, &None,
        "a durable success must not be reported as a failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_selection_is_reported_rather_than_shown_as_accepted() {
    let daemon = Arc::new(FakeSelectionDaemon::default());
    *daemon.refuse_with.lock().unwrap() = Some("model is not available on this connection".into());
    let (mut app, mut rx) = bound_app(daemon.clone());

    commit_model_via_dialog(&mut app, "opencode_go/gone-v1");
    let commands = drain(&mut rx).await;

    let failures: Vec<Option<String>> = commands
        .iter()
        .filter_map(|c| match c {
            TuiCommand::ModelSelectPersisted { error, .. } => Some(error.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        failures,
        vec![Some(
            "model is not available on this connection".to_string()
        )],
        "a refusal must surface; the daemon still holds the old selection"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_selection_without_a_bound_session_does_not_panic() {
    // No session is bound, so there is no durable selection to write.
    // The choice is still recorded on the tab and applied when a session
    // binds; this must be inert rather than an error.
    let daemon = Arc::new(FakeSelectionDaemon::default());
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let mut app = App::new_for_testing(project_dir.path().to_string_lossy().to_string());
    app.core_client = Some(daemon.clone());
    let (tx, mut rx) = mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);

    app.persist_durable_model_selection("opencode_go/mimo-v2.6-flash".to_string());

    let commands = drain(&mut rx).await;
    assert!(
        daemon.received.lock().unwrap().is_empty(),
        "nothing durable exists to update without a session"
    );
    assert!(
        commands.is_empty(),
        "no completion should be fabricated, got {commands:?}"
    );
}

/// A tab can start with `session_id: null` (restored from the manifest, or
/// brand new), so a `/model` choice made then has no durable row to write.
/// When such a session is later created it must adopt that pending choice,
/// or the first turn falls back to the daemon default.
#[tokio::test(flavor = "current_thread")]
async fn a_fresh_session_adopts_a_model_chosen_before_it_existed() {
    let daemon = Arc::new(FakeSelectionDaemon::default());
    // No session bound yet — the restored/new-tab state.
    let (mut app, mut rx) = unbound_app(daemon.clone());
    assert_eq!(app.active_session_id(), None);

    // The user picked a model while the tab had no session yet.
    commit_model_via_dialog(&mut app, "opencode_go/muse-spark-1.3-contributor");
    let _ = drain(&mut rx).await;
    assert!(
        daemon.received.lock().unwrap().is_empty(),
        "with no bound session there is nothing durable to write"
    );

    // Now the session is created with no selection of its own.
    app.set_session(fresh_session("sess-fresh"));
    let commands = drain(&mut rx).await;

    let received = daemon.received.lock().unwrap().clone();
    assert_eq!(
        received,
        vec![(
            "sess-fresh".to_string(),
            "opencode_go/muse-spark-1.3-contributor".to_string()
        )],
        "the pending choice must be adopted by the session it was made for"
    );
    let _ = commands;
}

/// An already-populated session must never be overwritten on restore.
#[tokio::test(flavor = "current_thread")]
async fn restoring_a_session_with_its_own_selection_does_not_overwrite_it() {
    let daemon = Arc::new(FakeSelectionDaemon::default());
    let (mut app, mut rx) = unbound_app(daemon.clone());

    let mut sess = fresh_session("sess-existing");
    sess.selected_model_id = Some("opencode_go/qwen3.8-max".to_string());
    app.set_session(sess);
    let _ = drain(&mut rx).await;

    assert!(
        daemon.received.lock().unwrap().is_empty(),
        "a session that already has a durable selection is the authority"
    );
}
