//! C001: the parent turn's boundary injects the delegated run's notice.
//!
//! A delegated run that terminates while the parent's turn is over still
//! leaves a durable notice. The next parent turn's boundary drains that
//! notice and hands it to the model as a `Message::System` that names the
//! child, so the orchestrator learns the outcome without polling.

use std::sync::Arc;

use codegg::agent::job_notification_recovery::inject_recoverable_job_notifications;
use codegg::provider::{ChatEvent, ChatRequest, ContentPart, Message};
use codegg::scheduler::job_notifications::JobNotificationService;
use codegg::tool::ToolRegistry;
use codegg_core::jobs::{JobId, JobKind, JobPayload, JobPriority, JobRecord, JobSource, JobState};
use codegg_core::workspace::WorkspaceId;

use super::agent_loop_harness::build_test_agent_loop_with_pool;
use super::common;

const SESSION: &str = "sess-c001-boundary";

fn delegated_job_record(job_id: &str, run_id: &str, session: &str) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked(job_id),
        workspace_id: WorkspaceId::new_unchecked("ws-c001"),
        session_id: Some(session.to_string()),
        turn_id: Some("turn-c001".to_string()),
        kind: JobKind::Subagent,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::SubagentRun {
            prompt: "do the delegated work".into(),
            agent: "general".into(),
            model: None,
            parent_id: Some(session.to_string()),
            denied_tools: vec![],
            allowed_paths: vec![],
            max_tool_calls: None,
            task_id: codegg_core::identity::AgentTaskId::new(),
            run_id: codegg_core::identity::AgentRunId::parse(run_id).expect("a valid run id"),
            delegation_key: "agent-delegation-c001".into(),
            base_commit: None,
            placement: Default::default(),
            default_working_directory: None,
        },
        resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Subagent),
        state: JobState::Completed,
        current_attempt_id: None,
        attempt_count: 1,
        created_at: now,
        updated_at: now,
        terminal_at: Some(now),
        cancel_requested_at: None,
        cancel_reason: None,
        labels: Default::default(),
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: Default::default(),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
    }
}

/// `session_events.session_id` references `session(id)`, which references
/// `project(id)`. Hermetic pools need both rows before the boundary can
/// append the durable parent-session event.
async fn ensure_session(pool: &sqlx::SqlitePool, session_id: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) \
         VALUES ('proj-c001', '/tmp', 1, 1, '[]')",
    )
    .execute(pool)
    .await
    .expect("project row");
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, 'proj-c001', 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(session_id)
    .execute(pool)
    .await
    .expect("session row");
}

/// The boundary's own injection: the notice becomes one system message that
/// names the child, and one durable parent-session event.
#[tokio::test(flavor = "current_thread")]
async fn the_boundary_injects_the_delegated_notice_as_a_system_message() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let store = codegg_core::session::EventStore::new(pool.clone());
    ensure_session(&pool, SESSION).await;

    let run_id = "run-c001-boundary";
    service
        .subscribe(
            &JobId::new_unchecked("job-c001-boundary"),
            "subagent",
            SESSION,
            Some("turn-c001"),
            "inv-c001",
        )
        .await
        .expect("the delegated job is subscribed");
    service
        .record_terminal(
            &delegated_job_record("job-c001-boundary", run_id, SESSION),
            "the child finished its work",
            None,
        )
        .await
        .expect("record the terminal notice");

    // The boundary drains the notice and hands it to the model.
    let mut messages: Vec<Message> = vec![];
    let report = inject_recoverable_job_notifications(Some(&store), &service, SESSION, |text| {
        messages.push(Message::System {
            content: Arc::new(text),
        })
    })
    .await;
    assert_eq!(report.injected, 1, "the boundary injects the notice once");
    assert_eq!(messages.len(), 1);
    let content = match &messages[0] {
        Message::System { content } => content.to_string(),
        other => panic!("the notice must arrive as a system message, got {other:?}"),
    };
    assert!(
        content.contains(run_id),
        "the injected message must identify the delegated run, got: {content}"
    );
    assert!(
        content.contains("the child finished its work"),
        "the injected message must carry the recorded outcome, got: {content}"
    );

    let events = store
        .list_for_session(SESSION)
        .await
        .expect("session events");
    assert_eq!(events.len(), 1, "one durable event per logical terminal");
}

/// The same boundary, driven through a real turn: the model sees the notice
/// as a system message before its first provider call.
#[tokio::test(flavor = "current_thread")]
async fn the_next_parent_turn_hands_the_notice_to_the_model() {
    let pool = common::pool::isolated_pool().await;
    let service = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let store = codegg_core::session::EventStore::new(pool.clone());
    ensure_session(&pool, "test-session").await;

    let run_id = "run-c001-turn";
    service
        .subscribe(
            &JobId::new_unchecked("job-c001-turn"),
            "subagent",
            "test-session",
            Some("turn-c001"),
            "inv-c001",
        )
        .await
        .expect("the delegated job is subscribed");
    service
        .record_terminal(
            &delegated_job_record("job-c001-turn", run_id, "test-session"),
            "the child finished its work",
            None,
        )
        .await
        .expect("record the terminal notice");

    let provider = Box::new(common::scripted_provider::ScriptedProvider::new(vec![
        vec![
            ChatEvent::TextDelta("seen".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: codegg::provider::TokenUsage::default(),
            },
        ],
    ]));
    let mut agent_loop =
        build_test_agent_loop_with_pool(provider.clone(), ToolRegistry::new(), Some(pool)).await;
    agent_loop.set_job_notification_service(service.clone());

    let request = ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: "what did my child do?".to_string().into(),
            }],
        }],
        model: "test/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    };
    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;
    assert!(result.is_ok(), "the turn must succeed: {:?}", result.err());

    let requests = provider.get_requests().await;
    let first = requests.first().expect("the provider was called");
    let system = first.messages.iter().find_map(|message| match message {
        Message::System { content } => Some(content.to_string()),
        _ => None,
    });
    let system = system.expect("the notice must reach the model as a system message");
    assert!(
        system.contains(run_id),
        "the system message must identify the delegated run, got: {system}"
    );
    let events = store
        .list_for_session("test-session")
        .await
        .expect("session events");
    assert_eq!(
        events.len(),
        1,
        "the boundary appends one durable parent-session event"
    );
}
