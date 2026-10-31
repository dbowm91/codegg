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
    ("project_init", "", "System"),
    ("policy", "", "System"),
    ("approval", "", "System"),
    ("network", "", "System"),
    ("team", "", "System"),
    // Execution
    ("goal", "", "Execution"),
    ("task_del", "", "Execution"),
    ("terminal_attach", "", "Execution"),
    ("terminal_resume", "", "Execution"),
    ("terminal_list", "", "Execution"),
    ("test", "", "Execution"),
    ("tasks", "", "Execution"),
    ("task", "", "Execution"),
    ("state", "", "Execution"),
    // Agent
    ("schedules", "", "Agent"),
    // Provider
    ("models_refresh", "", "Provider"),
    // Session
    ("edit_undo", "", "Session"),
    ("edit_reapply", "", "Session"),
    ("edit_checkpoints", "", "Session"),
    ("share", "", "Session"),
    ("unshare", "", "Session"),
    ("export", "", "Session"),
    ("compact", "", "Session"),
    ("reasoning", "", "Session"),
    ("control", "", "Session"),
    // Collaboration
    ("stop", "", "Collaboration"),
    ("chat_action_list", "", "Collaboration"),
    ("stop_observing", "", "Collaboration"),
    ("chat_history", "", "Collaboration"),
    ("chat_sync", "", "Collaboration"),
    ("chat_read", "", "Collaboration"),
    ("observe", "", "Collaboration"),
    ("collaborators", "", "Collaboration"),
    ("collaborators", "presence", "Collaboration"),
];

/// The families the migrated commands cover.
const FAMILIES: &[&str] = &[
    "Memory",
    "System",
    "Execution",
    "Agent",
    "Provider",
    "Session",
    "Collaboration",
];

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
            // The schedule deletion resolves the user's id against the
            // daemon's list first, so the recorder answers the list.
            CoreRequest::ScheduleList { .. } => CoreResponse::ScheduleList {
                schedules: vec![crate::protocol::dto::ScheduleSummaryDto {
                    schedule_id: "schedule-1".to_string(),
                    workspace_id: "workspace-1".to_string(),
                    session_id: None,
                    kind: serde_json::json!("loop"),
                    state: "active".to_string(),
                    overlap_policy: "skip".to_string(),
                    missed_run_policy: serde_json::json!("skip"),
                    next_run_at_ms: None,
                    last_occurrence_at_ms: None,
                    created_at_ms: 0,
                    updated_at_ms: 0,
                }],
            },
            // The Task view asks the daemon whether it can serve work
            // orders before it lists them.
            CoreRequest::WorkOrderCapabilities => CoreResponse::WorkOrderCapabilities {
                capabilities: crate::protocol::work_order::WorkOrderCapabilitiesDto {
                    supported: true,
                    protocol_version: 1,
                    max_title_chars: 200,
                    max_prompt_bytes: 65536,
                    max_batch_items: 32,
                    max_list_limit: 200,
                    max_repeat_count: 100,
                    max_lanes_per_project: 32,
                    max_lane_members: 32,
                },
            },
            // The observation surface negotiates its capabilities before it
            // subscribes; a daemon that does not advertise the projection
            // makes the arm degrade to the unavailable state instead of
            // issuing the command.
            CoreRequest::ProjectionCapabilities => CoreResponse::ProjectionCapabilitiesResponse {
                supported: true,
                projection_version: 1,
                max_events_per_batch: 128,
                max_event_bytes: 65536,
                max_subscriptions_per_client: 8,
                max_subscriptions_per_daemon: 64,
                retention_session_max_events: 1024,
                retention_project_max_events: 1024,
            },
            // The team surface negotiates its capabilities before it
            // lists anything.
            CoreRequest::TeamCapabilities => CoreResponse::TeamCapabilities {
                capabilities: crate::protocol::core::TeamCapabilitiesDto {
                    supported: true,
                    protocol_version: 1,
                    max_list_limit: 100,
                },
            },
            // A migrated command's effect arrives as the command
            // execution's own inner response, so the recorder answers
            // per command id.
            CoreRequest::CommandExecute { request } => {
                command_execute_response(&request.command_id)
            }
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

/// The command execution the recorder answers with, per command id.
///
/// The daemon's executor returns the inner response verbatim, so the
/// recorder answers the response the command's own effect would have
/// produced. Only the commands whose *shape* the arm must reach are
/// answered specially; every other command answers a generic JSON
/// payload, which is enough for an arm that only has to issue it.
fn command_execute_response(command_id: &str) -> CoreResponse {
    let inner = match command_id {
        "schedules" => serde_json::json!({
            "type": "schedule_list",
            "schedules": [{
                "schedule_id": "schedule-1",
                "workspace_id": "workspace-1",
                "session_id": null,
                "kind": "loop",
                "state": "active",
                "overlap_policy": "skip",
                "missed_run_policy": "skip",
                "next_run_at_ms": null,
                "last_occurrence_at_ms": null,
                "created_at_ms": 0,
                "updated_at_ms": 0,
            }],
        }),
        "approval" => serde_json::json!({
            "type": "approval_preference",
            "preference": {
                "principal_id": "principal-1",
                "approval_mode": "interactive",
                "sandbox_profile": "read_only",
                "revision": 1,
                "updated_at_ms": 0,
            },
        }),
        _ => serde_json::json!({ "type": "json", "data": { "ok": true } }),
    };
    CoreResponse::CommandExecute {
        result: CommandExecuteResultDto {
            run_id: "run-1".to_string(),
            command_id: command_id.to_string(),
            status: codegg_protocol::core::CommandOutcomeStatusDto::Completed,
            summary: "ok".to_string(),
            lines: Vec::new(),
            report: None,
            started_at_ms: 0,
            completed_at_ms: 1,
            response: Some(inner),
        },
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

    // The schedule deletion resolves the user's id against the daemon's
    // list first, so the session must carry the workspace the list is
    // scoped to.
    let session = crate::session::Session {
        id: "session-1".to_string(),
        project_id: "project-1".to_string(),
        workspace_id: Some("workspace-1".to_string()),
        ..Default::default()
    };
    app.session_state.session = Some(session);
    crate::tui::commands::tasks::start_delete_task(&mut app, "schedule-1".to_string());
    wait_for_command(&recorder, "task_del").await;
    assert_eq!(
        recorded_for(&recorder, "task_del"),
        Some((
            "task_del".to_string(),
            String::new(),
            "schedule-1".to_string()
        )),
        "the schedule deletion must carry the resolved schedule id as its argument"
    );

    // The attach names the terminal it attaches to; a fresh view has no
    // retained cursor, so the argument is the handle alone.
    crate::tui::commands::interactive_terminal::start_terminal_attach(
        &mut app,
        "handle-1".to_string(),
    );
    wait_for_command(&recorder, "terminal_attach").await;
    assert_eq!(
        recorded_for(&recorder, "terminal_attach"),
        Some((
            "terminal_attach".to_string(),
            String::new(),
            "handle-1".to_string()
        )),
        "the attach must carry the terminal handle as its argument"
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

    // The unsubscribe names the subscription the daemon issued, so the
    // observer must be holding one.
    let request_id = app
        .observer
        .begin_observe("project-1", "session-1")
        .expect("the observer accepts a valid target");
    let epoch = app.observer.reconnect_epoch;
    assert!(
        app.observer.apply_subscribed(
            request_id,
            "project-1",
            "session-1",
            crate::protocol::projection::ProjectionSubscriptionId::new("subscription-1"),
            crate::protocol::projection::ProjectionCursor {
                stream_id: crate::protocol::projection::ProjectionStreamId::new("stream-1")
                    .expect("a non-empty stream id is valid"),
                event_seq: 0,
                projection_version: 1,
            },
            epoch,
        ),
        "the observer accepts its own subscribe completion"
    );
    crate::tui::commands::observe::stop_observing(&mut app);
    wait_for_command(&recorder, "stop_observing").await;
    assert_eq!(
        recorded_for(&recorder, "stop_observing"),
        Some((
            "stop_observing".to_string(),
            String::new(),
            "subscription-1".to_string()
        )),
        "the unsubscribe must carry the subscription id the daemon issued"
    );

    // The chat window's three commands address one channel, so the chat
    // state must be holding that channel.
    app.chat.set_capability(true);
    app.chat.backward_paging = true;
    let epoch = app.chat.reconnect_epoch;
    let ensure = app
        .chat
        .begin_ensure("project-1")
        .expect("the chat state accepts a valid project");
    assert!(
        app.chat.apply_channel_list(
            ensure,
            "project-1",
            &[chat_channel("project-1")],
            false,
            epoch,
        ),
        "the chat state accepts the daemon's channel list"
    );
    assert_eq!(
        app.chat
            .get("project-1")
            .and_then(|e| e.active_channel_id.clone()),
        Some("channel-1".to_string()),
        "the listed channel becomes the active one"
    );

    crate::tui::commands::chat::start_chat_history(&mut app, "project-1".to_string());
    wait_for_command(&recorder, "chat_history").await;
    assert_eq!(
        recorded_for(&recorder, "chat_history"),
        Some((
            "chat_history".to_string(),
            String::new(),
            "channel-1".to_string()
        )),
        "the newest window must carry the channel as its argument"
    );

    crate::tui::commands::chat::start_chat_sync(&mut app, "project-1".to_string());
    wait_for_command(&recorder, "chat_sync").await;
    assert_eq!(
        recorded_for(&recorder, "chat_sync"),
        Some((
            "chat_sync".to_string(),
            String::new(),
            "channel-1 0".to_string()
        )),
        "the incremental sync must carry the channel and the cursor as its argument"
    );

    crate::tui::commands::chat::start_chat_read(&mut app, "project-1".to_string());
    wait_for_command(&recorder, "chat_read").await;
    assert_eq!(
        recorded_for(&recorder, "chat_read"),
        Some((
            "chat_read".to_string(),
            String::new(),
            "channel-1 0".to_string()
        )),
        "the read marker must carry the channel and the sequence as its argument"
    );
}

/// An app with a project, a workspace and a session bound, so the
/// commands that act on a project have something to act on.
fn app_with_project(recorder: Arc<RecordingClient>) -> App {
    let mut app = app_with_recorder(recorder);
    app.project_tabs = crate::tui::app::state::ProjectTabs::from_compat(
        "/tmp/m007d",
        Some("project-1".to_string()),
        Some("workspace-1".to_string()),
        Some("session-1".to_string()),
        "",
        "",
    );
    app.session_state.session = Some(crate::session::Session {
        id: "session-1".to_string(),
        project_id: "project-1".to_string(),
        workspace_id: Some("workspace-1".to_string()),
        ..Default::default()
    });
    app
}

/// The policy reads are executed by the daemon, and neither carries a
/// session.
///
/// M007-D, decision shape one, on the TUI's side: the effective policy
/// is a global reading, so the command carries no argument and the daemon
/// reads no session out of the scope. The approval preference is a unit
/// read for the same reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_policy_reads_are_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::policy::start_snapshot_refresh(
        &mut app,
        crate::tui::commands::policy::PolicySnapshotReason::SelectorRefresh,
    );
    wait_for_command(&recorder, "approval").await;
    assert_eq!(
        recorded_for(&recorder, "approval"),
        Some(("approval".to_string(), String::new(), String::new())),
        "the approval preference read must be executed as the catalog's approval command"
    );
    wait_for_command(&recorder, "policy").await;
    assert_eq!(
        recorded_for(&recorder, "policy"),
        Some(("policy".to_string(), String::new(), String::new())),
        "the effective policy read must be executed as the catalog's policy command"
    );
}

/// The network write carries the intent, and the revision when the caller
/// holds one.
///
/// The revision is the compare-and-swap the policy read published, so
/// dropping it would turn a checked write into an unchecked one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_network_write_carries_the_intent_and_the_revision() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::policy::request_network_intent_change(
        &mut app,
        codegg_core::approval::NetworkIntent::OfflineRequired,
    );
    wait_for_command(&recorder, "network").await;
    assert_eq!(
        recorded_for(&recorder, "network"),
        Some((
            "network".to_string(),
            String::new(),
            "offline_required".to_string()
        )),
        "the network write must carry the intent as its argument"
    );
}

/// The share, unshare and export commands carry the session they act on.
///
/// M007-D, decision shape two, on the TUI's side: the session travels as
/// the argument, because the share surface can name a session other than
/// the attached one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_session_named_commands_carry_their_session_in_the_argument() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::sessions::start_share_session(&mut app, "session-1".to_string());
    wait_for_command(&recorder, "share").await;
    assert_eq!(
        recorded_for(&recorder, "share"),
        Some(("share".to_string(), String::new(), "session-1".to_string())),
        "the share must carry the session it acts on as its argument"
    );

    crate::tui::commands::sessions::start_unshare_session(&mut app, "session-1".to_string());
    wait_for_command(&recorder, "unshare").await;
    assert_eq!(
        recorded_for(&recorder, "unshare"),
        Some((
            "unshare".to_string(),
            String::new(),
            "session-1".to_string()
        )),
        "the unshare must carry the session it acts on as its argument"
    );

    crate::tui::commands::sessions::start_export_session(&mut app, "session-1".to_string());
    wait_for_command(&recorder, "export").await;
    assert_eq!(
        recorded_for(&recorder, "export"),
        Some(("export".to_string(), String::new(), "session-1".to_string())),
        "the export must carry the session it acts on as its argument"
    );
}

/// The observation carries the session it follows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_observe_command_carries_the_session_in_the_argument() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::observe::start_observe(
        &mut app,
        "project-1".to_string(),
        "session-1".to_string(),
    );
    wait_for_command(&recorder, "observe").await;
    assert_eq!(
        recorded_for(&recorder, "observe"),
        Some((
            "observe".to_string(),
            String::new(),
            "session-1".to_string()
        )),
        "the observation must carry the session it follows as its argument"
    );
}

/// The task listing carries the bound it asked for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_task_command_carries_its_bound_in_the_argument() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::work_orders::open_task_view(&mut app, "task");
    wait_for_command(&recorder, "task").await;
    assert_eq!(
        recorded_for(&recorder, "task"),
        Some(("task".to_string(), String::new(), "200".to_string())),
        "the task listing must carry the bound it asked for as its argument"
    );
}

/// The Agent family: the schedule listing is executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_agent_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::tasks::start_list_tasks(&mut app);
    wait_for_command(&recorder, "schedules").await;
    assert_eq!(
        recorded_for(&recorder, "schedules"),
        Some(("schedules".to_string(), String::new(), String::new())),
        "the schedule listing must be executed as the catalog's schedules command"
    );
}

/// The Provider family: the model refresh is executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_provider_family_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    app.refresh_models();
    wait_for_command(&recorder, "models_refresh").await;
    assert_eq!(
        recorded_for(&recorder, "models_refresh"),
        Some(("models_refresh".to_string(), String::new(), String::new())),
        "the model refresh must be executed as the catalog's models_refresh command"
    );
}

/// The remaining System-family commands are executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_project_and_team_commands_are_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::team::dispatch_team_command(&mut app, "");
    wait_for_command(&recorder, "team").await;
    assert_eq!(
        recorded_for(&recorder, "team"),
        Some(("team".to_string(), String::new(), String::new())),
        "the membership listing must be executed as the catalog's team command"
    );
}

/// The remaining Session-family commands are executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_remaining_session_commands_are_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::reasoning::start_preference_refresh(&mut app);
    wait_for_command(&recorder, "reasoning").await;
    assert_eq!(
        recorded_for(&recorder, "reasoning"),
        Some(("reasoning".to_string(), String::new(), String::new())),
        "the reasoning selection must be executed as the catalog's reasoning command"
    );

    crate::tui::commands::control::dispatch_control_command(&mut app, "");
    wait_for_command(&recorder, "control").await;
    assert_eq!(
        recorded_for(&recorder, "control"),
        Some(("control".to_string(), String::new(), String::new())),
        "the control surface must be executed as the catalog's control command"
    );

    crate::tui::runtime::command_dispatch::dispatch_tui_command(
        &mut app,
        crate::tui::app::TuiCommand::EditCheckpointList {
            session_id: "session-1".to_string(),
            workspace_id: "workspace-1".to_string(),
        },
    )
    .await;
    wait_for_command(&recorder, "edit_checkpoints").await;
    assert_eq!(
        recorded_for(&recorder, "edit_checkpoints"),
        Some(("edit_checkpoints".to_string(), String::new(), String::new())),
        "the checkpoint listing must be executed as the catalog's edit_checkpoints command"
    );
}

/// The principal listing is executed by the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_principal_listing_is_executed_by_the_daemon() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());

    crate::tui::commands::team::dispatch_team_command(&mut app, "principals");
    wait_for_command(&recorder, "collaborators").await;
    assert_eq!(
        recorded_for(&recorder, "collaborators"),
        Some(("collaborators".to_string(), String::new(), String::new())),
        "the principal listing must be executed as the catalog's collaborators command"
    );
}

/// The addressed reply is executed by the daemon, as the `subagent_send`
/// command.
///
/// M007-H moved `/subagent-send` behind the daemon's command executor. The
/// frontend names the command, the session and the text; the **daemon**
/// resolves the turn, because `TurnSteer` is keyed on a session turn and the
/// frontend holds no turn id it could name safely.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_addressed_reply_is_executed_by_the_daemon_as_the_subagent_send_command() {
    let recorder = Arc::new(RecordingClient::default());
    let mut app = app_with_project(recorder.clone());
    app.ui_state
        .subagents
        .set_rows(vec![crate::tui::app::state::SubagentRow {
            transcript: None,
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
    assert!(
        crate::tui::commands::subagent_steer::arm_addressee(&mut app, "task-1"),
        "the live run must be addressable"
    );

    crate::tui::commands::subagent_steer::start_subagent_reply(
        &mut app,
        "try the second retry first".to_string(),
    );
    wait_for_command(&recorder, "subagent_send").await;
    let (command_id, form, argument) =
        recorded_for(&recorder, "subagent_send").expect("the addressed reply is a command");
    assert_eq!(command_id, "subagent_send");
    assert_eq!(
        form, "",
        "the command's default form is the only form it has"
    );
    assert!(
        argument.contains("Sally"),
        "the addressee's name travels in the argument: {argument:?}"
    );
    assert!(
        argument.ends_with("try the second retry first"),
        "the user's own words are sent verbatim: {argument:?}"
    );
    assert!(
        !argument.contains("task-1"),
        "the subagent's task id is not the steer recipient, and it must not be \
         named as one: {argument:?}"
    );
}

// ── 3. The outcome ───────────────────────────────────────────────────/// The response the TUI's presentation receives is the daemon's inner
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
    assert_eq!(checked, 42, "42 forms are frontend effects");
}

// ── The projection consumption (M007-H, D3/D4) ─────────────────────────────

/// The derived plan and test state carry the daemon's published answer.
///
/// M007-H (D3, D4): before this, `session_state_derived.plan` was only ever
/// what the frontend's own `/plan add` wrote and `test_state` was
/// permanently `Unknown`.
#[test]
fn the_derived_state_carries_the_daemons_published_plan_and_test_state() {
    use crate::protocol::projection::dto::{
        TestStateProjection, WorkPlanItemProjection, WorkPlanProjection,
    };
    use crate::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use crate::protocol::projection::snapshot::SessionProjectionSnapshot;

    let mut app = App::new_for_testing("/tmp/m007h".to_string());
    assert!(
        app.session_state_derived.plan.is_none(),
        "the derived plan starts empty, with no daemon answer behind it"
    );
    assert_eq!(
        app.session_state_derived.test_state,
        crate::session::state::TestState::Unknown,
        "the derived test state starts at the local default"
    );

    let mut snapshot = SessionProjectionSnapshot::empty("sess", "proj", "ws");
    snapshot.set_work_plan(WorkPlanProjection {
        plan_id: "wp-1".to_string(),
        revision: 3,
        status: "active".to_string(),
        objective_preview: "land the daemon's plan".to_string(),
        current_phase: Some("implement".to_string()),
        current_item_id: Some("wi-1".to_string()),
        total_items: 1,
        actionable_count: 1,
        blocked_count: 0,
        in_progress_count: 0,
        assessment: "actionable".to_string(),
        updated_at_ms: 1_700_000_000_000,
        items: vec![WorkPlanItemProjection {
            item_id: "wi-1".to_string(),
            description: "publish the plan".to_string(),
            status: "in_progress".to_string(),
            blocker: None,
            next_action: Some("publish it".to_string()),
        }],
    });
    snapshot.set_test_state(TestStateProjection {
        status: "passed".to_string(),
        command: "cargo test".to_string(),
        summary: Some("12 passed".to_string()),
    });
    app.projection_client.controller_mut().install_subscription(
        ProjectionSubscriptionId::new("sub-1"),
        ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new("stream-1").expect("a valid stream id"),
            kind: ProjectionStreamKind::Session,
            project_id: "proj".to_string(),
            workspace_id: Some("ws".to_string()),
            session_id: Some("sess".to_string()),
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        },
        snapshot,
    );

    crate::tui::commands::primary_projection::sync_derived_from_projection(&mut app);

    let plan = app
        .session_state_derived
        .plan
        .clone()
        .expect("the daemon's published plan is carried into the derived state");
    assert_eq!(
        plan.items.len(),
        1,
        "the published plan's items are carried"
    );
    assert_eq!(plan.items[0].id, "wi-1", "the item is the daemon's item");
    assert_eq!(
        plan.items[0].text, "publish the plan",
        "the item's text is the daemon's description"
    );
    assert_eq!(
        plan.items[0].status,
        crate::session::events::PlanItemStatus::InProgress,
        "the daemon's own item status is carried"
    );
    assert_eq!(
        app.session_state_derived.test_state,
        crate::session::state::TestState::Passed {
            command: "cargo test".to_string(),
            duration_ms: None,
        },
        "the daemon's published test state is carried, not the local default"
    );
}
