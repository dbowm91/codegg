//! Work package C: the three recorded scenarios.
//!
//! Each member runs one fixture through the shipped agent loop and requires the
//! durable result to reproduce the fixture's expectation. No provider
//! credential is present and no network call is possible: the only model input
//! is the fixture's scripted responses.
//!
//! The family is one member per fixture so the guard can require that every
//! fixture is run by exactly one member, and so a failing scenario names itself
//! in the test id.

use codegg::provider::Message;

use std::future::Future;

use super::common::session_replay::{
    run_scenario, ReplayError, ReplayFixture, ScenarioRun, SCENARIO_TIMEOUT,
};

/// Await one scenario, bound it to a deadline, and require it to reproduce its
/// recorded expectation.
///
/// Each member passes its own `run_scenario("…")` future rather than a name,
/// so the binding stays a literal in the member body: the fixture-corpus guard
/// counts the members that bind a fixture, and an indirection here would hide
/// the binding from it.
async fn assert_scenario(
    scenario: impl Future<Output = Result<(ReplayFixture, ScenarioRun), ReplayError>>,
) -> ScenarioRun {
    let (fixture, run) = tokio::time::timeout(SCENARIO_TIMEOUT, scenario)
        .await
        .expect("a scenario does not hang")
        .unwrap_or_else(|error| panic!("scenario could not run: {error}"));
    if let Err(divergence) = fixture.compare(&run.observed) {
        panic!("{divergence}");
    }
    run
}

#[tokio::test]
async fn s1_plain_turn_matches_its_expectation() {
    let run = assert_scenario(run_scenario("s1_plain_turn")).await;

    assert_eq!(run.observed.terminal, "completed");
    assert_eq!(
        run.observed.request_count, 1,
        "a plain turn asks the model exactly once"
    );
    assert!(
        run.observed.tool_calls.is_empty() && run.observed.tool_results.is_empty(),
        "a plain turn calls no tool"
    );
}

#[tokio::test]
async fn s2_tool_call_matches_its_expectation() {
    let run = assert_scenario(run_scenario("s2_tool_call")).await;

    assert_eq!(run.observed.terminal, "completed");
    assert_eq!(
        run.observed.request_count, 3,
        "one tool iteration, the model's answer, and the loop's post-tool continuation"
    );
    assert_eq!(
        run.observed.tool_results,
        vec![super::common::session_replay::ExpectedToolResult {
            tool: "read".to_string(),
            outcome: "success".to_string(),
        }],
        "the real read tool succeeded on the committed target"
    );
}

#[tokio::test]
async fn s3_multi_step_turn_matches_its_expectation() {
    let run = assert_scenario(run_scenario("s3_multi_step_turn")).await;

    assert_eq!(run.observed.terminal, "completed");
    assert_eq!(
        run.observed.request_count, 4,
        "two tool iterations, the model's answer, and the post-tool continuation"
    );

    // The loop's second request is its second iteration. It must carry the
    // **first** tool's real output, so a scenario cannot pass by ignoring the
    // tool: a stubbed or dropped result would leave this message empty.
    let second = &run.requests[1].request;
    let tool_outputs: Vec<&str> = second
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Tool { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        tool_outputs.len(),
        1,
        "the second request carries exactly one tool result"
    );
    assert!(
        tool_outputs[0].contains("The recording marker is alpha-7731."),
        "the second request must carry the first tool's real output: {}",
        tool_outputs[0]
    );
}
