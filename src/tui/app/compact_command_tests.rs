//! The `/compact` command's truthfulness (C001).
//!
//! The command used to toast "Compaction triggered - reducing context" and
//! send nothing. The daemon owns compaction and fires it automatically when
//! its own threshold is crossed, so the truthful outcome is the policy's
//! shape and the count the TUI already keeps.

use super::App;
use crate::core::CoreClient;
use crate::error::AppError;
use codegg_protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};
use std::sync::Mutex;

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

#[derive(Default)]
struct RecordingDaemon {
    requests: Mutex<Vec<String>>,
}

impl RecordingDaemon {
    fn request_names(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl CoreClient for RecordingDaemon {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        self.requests
            .lock()
            .unwrap()
            .push(format!("{:?}", request.payload));
        Ok(CoreResponse::Ack)
    }

    fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        rx
    }
}

#[test]
fn the_compact_command_reports_what_compaction_is_and_never_claims_to_triggered_one() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch(&mut app, "/compact");
    let toast = last_toast(&app);

    assert!(!toast.is_empty(), "the command must say something");
    assert!(
        !toast.contains("Compaction triggered"),
        "the command must not claim it triggered a compaction: {toast:?}"
    );
    assert!(
        !toast.contains("will occur"),
        "the command must not promise a compaction it cannot guarantee: {toast:?}"
    );
    assert!(
        toast.contains("automatic"),
        "the command must report that compaction is the daemon's automatic policy: {toast:?}"
    );
}

#[test]
fn the_compact_command_reports_the_count_the_tui_keeps() {
    let mut app = App::new_for_testing("/tmp".into());
    app.session_state.session = Some(bound_session("s-1"));
    app.session_state.compaction_count = 3;
    dispatch(&mut app, "/compact");
    let toast = last_toast(&app);

    assert!(
        toast.contains('3'),
        "the toast must report the count the TUI keeps: {toast:?}"
    );
}

#[test]
fn the_compact_command_sends_no_request_to_the_daemon() {
    let daemon = std::sync::Arc::new(RecordingDaemon::default());
    let mut app = App::new_for_testing("/tmp".into());
    app.set_core_client(daemon.clone());
    dispatch(&mut app, "/compact");

    assert_eq!(
        daemon.request_names(),
        Vec::<String>::new(),
        "the command must not reach the daemon's policy"
    );
}

#[test]
fn every_surface_that_reaches_the_compact_command_reports_the_same_truth() {
    let descriptor = codegg_core::command_catalog::find_by_command_id("compact")
        .expect("the catalog must carry the compact command");
    assert_eq!(
        descriptor.kind,
        codegg_core::command_catalog::CommandKind::FrontendEffect,
        "the catalog must classify the form as a frontend effect"
    );
    assert_eq!(
        codegg_core::frontend_action::action_for_form("compact", ""),
        Some(codegg_core::frontend_action::FrontendAction::CompactSession),
        "the declared action must be the TUI's own compact command"
    );

    let registry =
        crate::tui::command::CommandRegistry::new_for_workspace_root(std::path::Path::new("/tmp"));
    let command = registry
        .find_by_name_or_alias("compact")
        .expect("the palette must carry the compact command");
    assert!(
        matches!(
            command.action,
            crate::tui::command::CommandAction::Builtin(
                crate::tui::command::BuiltinSlashAction::Compact
            )
        ),
        "the typed command must run the compact builtin"
    );

    let mut via_builtin = App::new_for_testing("/tmp".into());
    via_builtin.session_state.session = Some(bound_session("s-1"));
    via_builtin.session_state.compaction_count = 2;
    dispatch(&mut via_builtin, "/compact");

    let mut via_declared_action = App::new_for_testing("/tmp".into());
    via_declared_action.session_state.session = Some(bound_session("s-1"));
    via_declared_action.session_state.compaction_count = 2;
    crate::tui::commands::tasks::handle_compact_session(&mut via_declared_action);

    assert_eq!(
        last_toast(&via_builtin),
        last_toast(&via_declared_action),
        "both surfaces must report the same truth"
    );
}
