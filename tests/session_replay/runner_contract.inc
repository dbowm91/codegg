//! Work package B: the runner's contract, and the failure modes §8 names.
//!
//! These members do not add corpus scenarios. They prove the tier can fail —
//! a perturbed expectation, a removed tool declaration, a script that runs out
//! — and that its reads are durable rather than in-memory: a scenario reads its
//! session back through a freshly opened store handle, and two scenarios in one
//! process cannot observe each other's requests.

use std::sync::Arc;

use codegg::provider::{ChatEvent, Message, TokenUsage};
use codegg::session::{CreateSession, MessageStore, SessionStore};
use serde_json::json;

use super::common::isolated_env::env_lock;
use super::common::scripted_provider::{Exhaustion, ScriptedProvider};
use super::common::session_replay::{
    chat_request, read_durable_rows, replay_pools, run_fixture, run_plan, ReplayFixture,
    ReplayPlan, ReplayProfile, SCENARIO_TIMEOUT,
};

fn text(body: &str) -> ChatEvent {
    ChatEvent::TextDelta(Arc::new(body.to_string()))
}

fn finish(reason: &str) -> ChatEvent {
    ChatEvent::Finish {
        stop_reason: Arc::new(reason.to_string()),
        usage: TokenUsage::default(),
    }
}

/// The prompt a request's last user message carries.
fn user_prompt(request: &codegg::provider::ChatRequest) -> String {
    request
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|part| match part {
                        codegg::provider::ContentPart::Text { text } => Some(text.to_string()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn perturbed_expectation_fails_and_names_the_field() {
    let mut fixture = ReplayFixture::named("s1_plain_turn").expect("load the plain-turn fixture");
    let run = tokio::time::timeout(SCENARIO_TIMEOUT, run_fixture(&fixture))
        .await
        .expect("the turn completes")
        .expect("the turn runs");

    // Perturb one expectation field only.
    fixture.expect.assistant_text = "A deliberately different answer.".to_string();
    let divergence = fixture
        .compare(&run.observed)
        .expect_err("a perturbed expectation must fail the comparison");

    assert_eq!(
        divergence.field, "assistant_text",
        "the divergence must name the field that moved"
    );
    let rendered = divergence.to_string();
    assert!(
        rendered.contains("A deliberately different answer.")
            && rendered.contains("Recorded hello."),
        "the failure must print the expected and actual values: {rendered}"
    );
    assert!(
        rendered.contains("expected rows") && rendered.contains("actual rows"),
        "the failure must print the rows side by side: {rendered}"
    );
}

#[tokio::test]
async fn removed_tool_declaration_fails_the_scenario() {
    let mut fixture = ReplayFixture::named("s2_tool_call").expect("load the tool-call fixture");
    // The tool declaration is what the fixture's profile admits. Dropping it
    // is the admission perturbation: the model still asks for `read`, and the
    // loop must refuse it rather than silently succeed.
    fixture.profile = ReplayProfile::Plain;
    let run = tokio::time::timeout(SCENARIO_TIMEOUT, run_fixture(&fixture))
        .await
        .expect("the turn completes")
        .expect("the turn runs");

    let divergence = fixture
        .compare(&run.observed)
        .expect_err("a removed tool declaration must fail the scenario");
    assert_eq!(
        divergence.field, "tool_results",
        "the divergence must name the tool outcome that moved"
    );
    assert!(
        run.observed
            .tool_results
            .iter()
            .any(|result| result.outcome == "failure"),
        "the refused call must be recorded as a failure, not a success"
    );
}

#[tokio::test]
async fn fixture_exhaustion_fails_with_the_request_index() {
    let plan = ReplayPlan {
        session_id: "replay_exhaustion".to_string(),
        user_input: "Read the replay target.".to_string(),
        profile: ReplayProfile::Read,
    };
    let scripted = vec![vec![
        ChatEvent::ToolCall(codegg::provider::ToolCall {
            id: Arc::new("call_read_1".to_string()),
            name: Arc::new("read".to_string()),
            arguments: json!({"path": "tests/session_replay/fixtures/read_target.txt"}),
        }),
        finish("tool_calls"),
    ]];
    let provider = ScriptedProvider::new(scripted).with_exhaustion(Exhaustion::Fail);

    let run = tokio::time::timeout(SCENARIO_TIMEOUT, run_plan(&plan, Box::new(provider)))
        .await
        .expect("an exhausted script must fail, not hang")
        .expect("the runner returns a failed turn rather than an error");

    assert_eq!(run.observed.terminal, "failed:provider");
    let error = run
        .observed
        .error
        .clone()
        .expect("a failed turn renders its error");
    assert!(
        error.contains("request index 1"),
        "the failure must name the request index: {error}"
    );
}

#[tokio::test]
async fn a_scenario_reads_its_session_through_a_reopened_handle() {
    let (primary, reopened) = replay_pools().await.expect("two handles on one database");
    SessionStore::new(primary.clone())
        .create_with_id(
            "reopen-read",
            CreateSession {
                project_id: "session-replay".to_string(),
                directory: ".".to_string(),
                title: None,
                parent_id: None,
                workspace_id: None,
                agent: None,
                model: None,
                tags: None,
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            },
        )
        .await
        .expect("create the session");
    MessageStore::new(primary.clone())
        .create(
            "reopen-read",
            json!({
                "id": "msg_1",
                "sessionID": "reopen-read",
                "messageID": "msg_1",
                "role": "user",
                "parts": [{
                    "id": "msg_1_0",
                    "sessionID": "reopen-read",
                    "messageID": "msg_1",
                    "type": "text",
                    "text": "written before the reopen"
                }]
            }),
        )
        .await
        .expect("write one durable row");

    let rows = read_durable_rows(&reopened, "reopen-read")
        .await
        .expect("the reopened handle reads the row");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "written before the reopen");

    // A different database must not see it: the reopen is a second handle on
    // the same store, not a process-wide cache.
    let (other, _) = replay_pools().await.expect("an unrelated store");
    let elsewhere = read_durable_rows(&other, "reopen-read")
        .await
        .expect("the unrelated store answers");
    assert!(
        elsewhere.is_empty(),
        "an unrelated scenario's store must not see this row"
    );
}

#[tokio::test]
async fn two_scenarios_do_not_share_requests() {
    let first_plan = ReplayPlan {
        session_id: "replay_isolation_first".to_string(),
        user_input: "the first scenario's prompt".to_string(),
        profile: ReplayProfile::Plain,
    };
    let second_plan = ReplayPlan {
        session_id: "replay_isolation_second".to_string(),
        user_input: "the second scenario's prompt".to_string(),
        profile: ReplayProfile::Plain,
    };

    let first = run_plan(
        &first_plan,
        Box::new(ScriptedProvider::new(vec![vec![
            text("first answer"),
            finish("stop"),
        ]])),
    )
    .await
    .expect("first scenario");
    let second = run_plan(
        &second_plan,
        Box::new(ScriptedProvider::new(vec![vec![
            text("second answer"),
            finish("stop"),
        ]])),
    )
    .await
    .expect("second scenario");

    assert_eq!(first.requests.len(), 1);
    assert_eq!(second.requests.len(), 1);
    assert_eq!(
        user_prompt(&first.requests[0].request),
        first_plan.user_input
    );
    assert_eq!(
        user_prompt(&second.requests[0].request),
        second_plan.user_input
    );
    assert_ne!(first.observed.user_text, second.observed.user_text);
}

// The process-global environment is held stable for exactly the duration of
// the turn, so the guard is deliberately held across the await: releasing it
// first would let another member's `remove_var` race this test's premise.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn a_present_provider_credential_does_not_change_the_outcome() {
    let guard = env_lock();
    let previous = std::env::var("ANTHROPIC_API_KEY").ok();
    std::env::set_var(
        "ANTHROPIC_API_KEY",
        "sk-replay-tier-never-uses-this-value-000000",
    );

    let plan = ReplayPlan {
        session_id: "replay_keyless".to_string(),
        user_input: "Answer from the fixture alone.".to_string(),
        profile: ReplayProfile::Plain,
    };
    let run = run_plan(
        &plan,
        Box::new(ScriptedProvider::new(vec![vec![
            text("answered without a provider"),
            finish("stop"),
        ]])),
    )
    .await
    .expect("the scripted turn runs");

    match previous {
        Some(value) => std::env::set_var("ANTHROPIC_API_KEY", value),
        None => std::env::remove_var("ANTHROPIC_API_KEY"),
    }
    drop(guard);

    assert_eq!(run.observed.terminal, "completed");
    assert_eq!(
        run.observed.request_count, 1,
        "no credential was consulted, so no request was retried"
    );
    assert_eq!(
        run.observed.assistant_text, "answered without a provider",
        "the answer came from the fixture"
    );
}

// Keep `chat_request` referenced from this member so the shared helper's own
// request shape is exercised by more than the runner's internal call.
#[test]
fn the_shared_request_shape_carries_one_user_message() {
    let request = chat_request("a prompt");
    assert_eq!(request.messages.len(), 1);
    assert_eq!(user_prompt(&request), "a prompt");
    assert!(
        request.tools.is_none() && request.system.is_none(),
        "the replay request declares no tools and no caller system prompt"
    );
}
