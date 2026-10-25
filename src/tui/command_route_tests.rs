//! M007-D — the TUI's daemon-effect commands go through the daemon.
//!
//! The milestone's rule is that a **daemon effect** is executed by the
//! daemon: the frontend asks the daemon to run the command, and the daemon
//! returns the effect's authoritative response, which the frontend's
//! existing presentation code then renders. A **frontend effect** never
//! reaches the daemon at all.
//!
//! This file is the regression coverage for the commands the TUI has
//! migrated, organised by command family. The family is the catalog's
//! `CommandDomain`, and the family list is the closure artifact's family
//! column: a family's coverage is one test, and a migrated command with no
//! test fails the family it belongs to.
//!
//! The coverage has three parts, and each is a different claim:
//!
//! 1. the command's **route** is a daemon effect the daemon serves;
//! 2. the TUI issues a `CoreRequest::CommandExecute` carrying that
//!    command's identity and its argument;
//! 3. the response the TUI's presentation receives is the daemon's inner
//!    response, verbatim — so the observable outcome does not change.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg_core::command_catalog::{the_catalog, CommandKind};
use tokio::sync::mpsc;

use crate::core::CoreClient;
use crate::error::AppError;
use crate::protocol::core::{
    ChatCapabilitiesDto, ChatChannelDto, CommandCatalogScopeDto, CommandExecuteResultDto,
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use crate::tui::app::App;
use crate::tui::command_route::{route_for, CommandRoute};

/// The commands the TUI has migrated, by family.
///
/// The family is the catalog's own domain, so this list and the catalog
/// cannot drift apart: a command whose domain changed is a command whose
/// test moved.
const MIGRATED: &[(&str, &str, &str)] = &[
    // Memory
    ("memory", "", "Memory"),
    ("memory_list", "", "Memory"),
    ("memory_search", "", "Memory"),
    ("memory_remember", "", "Memory"),
    ("memory_forget", "", "Memory"),
    // System
    ("reload", "", "System"),
    ("nodes", "", "System"),
    // Execution
    ("goal", "", "Execution"),
    // Session
    ("edit_undo", "", "Session"),
    ("edit_reapply", "", "Session"),
    // Collaboration
    ("stop", "", "Collaboration"),
    ("chat_action_list", "", "Collaboration"),
];

/// The families the migrated commands cover.
const FAMILIES: &[&str] = &["Memory", "System", "Execution", "Session", "Collaboration"];

/// A client that records the requests it is given and answers a command
/// execution with the inner response the daemon would have produced.
#[derive(Default)]
struct RecordingClient {
    seen: Mutex<Vec<CoreRequest>>,
}

impl RecordingClient {
    fn seen(&self) -> Vec<CoreRequest> {
        self.seen
            .lock()
            .expect("the recorder is not poisoned")
            .clone()
    }
}

#[async_trait]
impl CoreClient for RecordingClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        let response = match &request.payload {
            // The chat surface negotiates its capabilities and resolves its
            // channel before the effect it is about to run. The recorder
            // answers both, so the effect is reached.
            CoreRequest::ChatCapabilities => CoreResponse::ChatCapabilities {
                capabilities: chat_capabilities(),
            },
            CoreRequest::ChatChannelEnsure { project_id, .. } => CoreResponse::ChatChannel {
                channel: chat_channel(project_id),
            },
            _ => CoreResponse::CommandExecute {
                result: CommandExecuteResultDto {
                    run_id: "run-1".to_string(),
                    command_id: "recorded".to_string(),
                    status: codegg_protocol::core::CommandOutcomeStatusDto::Completed,
                    summary: "ok".to_string(),
                    lines: Vec::new(),
                    report: None,
                    started_at_ms: 0,
                    completed_at_ms: 1,
                    // The inner response travels verbatim: the daemon's own
                    // response for the effect.
                    response: Some(serde_json::json!({ "type": "json", "data": { "ok": true } })),
                },
            },
        };
        self.seen
            .lock()
            .expect("the recorder is not poisoned")
            .push(request.payload);
        Ok(response)
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

/// The chat capabilities the recorder advertises.
fn chat_capabilities() -> ChatCapabilitiesDto {
    ChatCapabilitiesDto {
        supported: true,
        protocol_version: 1,
        max_body_bytes: 64 * 1024,
        max_page_limit: 100,
        max_mentions: 16,
        max_references: 16,
        composing_ttl_secs: 30,
        retention_max_messages: 1000,
        actions_supported: true,
        max_action_title_bytes: 256,
        max_action_prompt_bytes: 8192,
        backward_paging: false,
    }
}

/// The channel the recorder resolves for a project.
fn chat_channel(project_id: &str) -> ChatChannelDto {
    ChatChannelDto {
        channel_id: "channel-1".to_string(),
        project_id: project_id.to_string(),
        name: "general".to_string(),
        created_by: "principal-1".to_string(),
        created_at_ms: 0,
    }
}

/// The command identity and argument of a recorded `CommandExecute`.
fn executed(request: &CoreRequest) -> Option<(String, String, String)> {
    let CoreRequest::CommandExecute { request } = request else {
        return None;
    };
    Some((
        request.command_id.clone(),
        request.form.clone(),
        request.argument.clone(),
    ))
}

/// The first recorded request for a command, if the TUI issued one.
fn recorded_for(recorder: &RecordingClient, command_id: &str) -> Option<(String, String, String)> {
    recorder
        .seen()
        .iter()
        .filter_map(executed)
        .find(|(id, _, _)| id == command_id)
}

/// Wait until the TUI has issued a request for a command.
async fn wait_for_command(recorder: &RecordingClient, command_id: &str) {
    for _ in 0..200 {
        if recorded_for(recorder, command_id).is_some() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the TUI did not issue a request for {command_id}");
}

fn app_with_recorder(recorder: Arc<RecordingClient>) -> App {
    let mut app = App::new_for_testing("/tmp/m007d".to_string());
    app.set_core_client(recorder);
    let (tx, _rx) = mpsc::channel(8);
    app.tui_cmd_tx = Some(tx);
    app
}

/// Wait until the recorder has seen at least `count` requests.
async fn wait_for_requests(recorder: &RecordingClient, count: usize) {
    for _ in 0..200 {
        if recorder.seen().len() >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the TUI did not issue {count} request(s)");
}

// ── 1. The route ─────────────────────────────────────────────────────

/// Every migrated command's route is a daemon effect the daemon serves.
///
/// A migrated command whose route is not a daemon effect would be executed
/// locally, and the milestone's rule is that it is not.
#[test]
fn every_migrated_command_routes_to_the_daemon() {
    for (command_id, form, _) in MIGRATED {
        assert!(
            matches!(
                route_for(command_id, form),
                CommandRoute::DaemonEffect { .. }
            ),
            "{command_id} must route to the daemon"
        );
    }
}

/// The migrated commands are spread over the families the coverage claims.
///
/// A family with no migrated command would have a coverage entry with
/// nothing in it, and the family list would overstate what is covered.
#[test]
fn the_migrated_commands_cover_the_claimed_families() {
    let catalog = the_catalog();
    let mut covered: Vec<String> = Vec::new();
    for (command_id, _, claimed) in MIGRATED {
        let command = catalog
            .commands
            .iter()
            .find(|command| command.command_id == *command_id)
            .unwrap_or_else(|| panic!("{command_id} must be in the catalog"));
        let family = command.domain.to_string();
        assert_eq!(
            family, *claimed,
            "{command_id} belongs to the {family} family, which the coverage claims as {claimed}"
        );
        assert!(
            FAMILIES.contains(&family.as_str()),
            "{command_id} belongs to the {family} family, which the coverage does not claim"
        );
        if !covered.contains(&family) {
            covered.push(family);
        }
    }
    assert_eq!(
        covered.len(),
        FAMILIES.len(),
        "every claimed family is covered"
    );
}

// ── 2. The TUI's request ─────────────────────────────────────────────

/// The Memory family: the TUI issues a `CommandExecute` for each of the
/// four memory commands, carrying the command's identity and its argument.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_memory_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_recorder(recorder.clone());
    let client = app.core_client.clone().expect("the recorder is the client");

    crate::tui::commands::memory::start_memory_search(&mut app, "needle".to_string());
    wait_for_requests(&recorder, 1).await;
    assert_eq!(
        recorded_for(&recorder, "memory_search"),
        Some((
            "memory_search".to_string(),
            String::new(),
            "needle".to_string()
        )),
        "the search command must carry its query as the argument"
    );

    crate::tui::commands::memory::start_memory_remember(&mut app, "a fact".to_string());
    wait_for_requests(&recorder, 2).await;
    assert_eq!(
        recorded_for(&recorder, "memory_remember"),
        Some((
            "memory_remember".to_string(),
            String::new(),
            "a fact".to_string()
        )),
        "the remember command must carry its text as the argument"
    );

    crate::tui::commands::memory::start_memory_forget(&mut app, "mem-1".to_string());
    wait_for_requests(&recorder, 3).await;
    assert_eq!(
        recorded_for(&recorder, "memory_forget"),
        Some((
            "memory_forget".to_string(),
            String::new(),
            "mem-1".to_string()
        )),
        "the forget command must carry its id as the argument"
    );

    crate::tui::commands::memory::start_memory_summary(&mut app, "memory".to_string());
    wait_for_command(&recorder, "memory").await;
    let summary = recorded_for(&recorder, "memory").expect("the summary is a command");
    assert!(
        !summary.2.is_empty(),
        "the summary command must carry the namespace as its argument"
    );

    // `/memory-list` is a second command that shares the effect, and it is
    // addressable by its own identity.
    crate::tui::commands::memory::start_memory_summary(&mut app, "memory_list".to_string());
    wait_for_command(&recorder, "memory_list").await;
    assert_eq!(
        recorded_for(&recorder, "memory_list").map(|(id, _, _)| id),
        Some("memory_list".to_string()),
        "the list command must be executed as the catalog's memory_list command"
    );

    drop(client);
}

/// The System family: the node telemetry read is executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_system_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_recorder(recorder.clone());

    crate::tui::commands::diagnostics::request_node_telemetry(&mut app);
    wait_for_command(&recorder, "nodes").await;
    assert_eq!(
        recorded_for(&recorder, "nodes"),
        Some(("nodes".to_string(), String::new(), String::new())),
        "the node telemetry read must be executed as the catalog's nodes command"
    );
}

/// The Execution family: the goal read is executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_execution_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_recorder(recorder.clone());

    crate::tui::commands::goals::start_goal_show(&mut app, "session-1".to_string());
    wait_for_command(&recorder, "goal").await;
    assert_eq!(
        recorded_for(&recorder, "goal"),
        Some(("goal".to_string(), String::new(), String::new())),
        "the goal read must be executed as the catalog's goal command"
    );
}

/// The Session family: the checkpoint undo and reapply are executed by the
/// daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_session_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_recorder(recorder.clone());

    crate::tui::runtime::command_dispatch::dispatch_tui_command(
        &mut app,
        crate::tui::app::TuiCommand::EditUndoLatest {
            session_id: "session-1".to_string(),
            workspace_id: "workspace-1".to_string(),
        },
    )
    .await;
    wait_for_command(&recorder, "edit_undo").await;
    assert_eq!(
        recorded_for(&recorder, "edit_undo"),
        Some(("edit_undo".to_string(), String::new(), String::new())),
        "the checkpoint undo must be executed as the catalog's edit_undo command"
    );

    crate::tui::runtime::command_dispatch::dispatch_tui_command(
        &mut app,
        crate::tui::app::TuiCommand::EditReapplyLatest {
            session_id: "session-1".to_string(),
            workspace_id: "workspace-1".to_string(),
        },
    )
    .await;
    wait_for_command(&recorder, "edit_reapply").await;
    assert_eq!(
        recorded_for(&recorder, "edit_reapply"),
        Some(("edit_reapply".to_string(), String::new(), String::new())),
        "the checkpoint reapply must be executed as the catalog's edit_reapply command"
    );
}

/// The Collaboration family: the turn cancel and the structured-action
/// listing are executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_collaboration_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_recorder(recorder.clone());

    // The stop command acts on the session's running turn, so the session
    // and the turn must be bound before there is anything to stop.
    let session = crate::session::Session {
        id: "session-1".to_string(),
        project_id: "project-1".to_string(),
        ..Default::default()
    };
    app.session_state.session = Some(session);
    app.session_state.session_status = crate::tui::app::SessionStatus::Working;
    app.session_state.active_turn_id = Some("turn-1".to_string());

    crate::tui::commands::turn_control::stop(&mut app);
    wait_for_command(&recorder, "stop").await;
    assert_eq!(
        recorded_for(&recorder, "stop"),
        Some(("stop".to_string(), String::new(), "turn-1".to_string())),
        "the stop command must carry the turn id as its argument"
    );

    crate::tui::commands::chat::start_chat_action_list(
        &mut app,
        "project-1".to_string(),
        Some("msg-1".to_string()),
    );
    wait_for_command(&recorder, "chat_action_list").await;
    assert_eq!(
        recorded_for(&recorder, "chat_action_list"),
        Some((
            "chat_action_list".to_string(),
            String::new(),
            "channel-1 msg-1".to_string()
        )),
        "the action listing must carry the channel and the message id as its argument"
    );
}

// ── 3. The outcome ───────────────────────────────────────────────────
/// The response the TUI's presentation receives is the daemon's inner
/// response, verbatim.
///
/// This is the outcome-preservation claim: the daemon owns the effect and
/// the authoritative data, and the frontend keeps the formatting by
/// rendering the response it already knew how to render.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_inner_response_reaches_the_presentation_verbatim() {
    let recorder = Arc::new(RecordingClient::default());
    let client: Arc<dyn CoreClient> = recorder.clone();
    let response = crate::tui::command_route::send_command(
        &client,
        CommandCatalogScopeDto {
            project_id: "project".to_string(),
            workspace_id: "workspace".to_string(),
            session_id: Some("session".to_string()),
        },
        "memory_search",
        "",
        "needle",
    )
    .await
    .expect("the command is executed");
    match response {
        CoreResponse::Json { data } => {
            assert_eq!(data.get("ok").and_then(|v| v.as_bool()), Some(true))
        }
        other => panic!("the inner response must reach the presentation, got {other:?}"),
    }
}

// ── The frontend-effect half ─────────────────────────────────────────

/// A frontend effect never reaches the daemon.
///
/// The declared frontend-action set is the frontend's own namespace: it is
/// not the keymap's action-id namespace, and it is not the editor's. A
/// frontend-effect form is dispatched locally, from the declared action,
/// and the daemon is never asked to run it.
#[test]
fn a_frontend_effect_is_dispatched_from_the_declared_action() {
    let catalog = the_catalog();
    let mut checked = 0usize;
    for command in catalog.commands {
        for form in command.forms {
            if form.kind != CommandKind::FrontendEffect {
                continue;
            }
            match route_for(command.command_id, form.form) {
                CommandRoute::FrontendEffect { .. } => {
                    checked += 1;
                }
                other => panic!(
                    "{} must route to a frontend effect, got {other:?}",
                    command.command_id
                ),
            }
        }
    }
    assert_eq!(checked, 32, "32 forms are frontend effects");
}
