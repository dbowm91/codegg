//! Work package D: the recorder.
//!
//! A fixture should be **recorded** from a real run rather than invented. The
//! recorder is a decorator that tees a provider's stream, and its writer
//! refuses content that must never reach a repository. It is local-only: the
//! tests here drive it with a scripted provider, so they need no credential and
//! no network, and nothing in the routine sweep records anything.
//!
//! The live workflow — a real provider, a key, a human — is documented in
//! `architecture/testing.md`; the closure record states that it was not
//! executed here.

use std::sync::Arc;

use codegg::provider::{ChatEvent, Provider, TokenUsage};
use futures_util::StreamExt;

use super::common::isolated_env::env_lock;
use super::common::replay_recorder::{
    credential_shape, fixture_from_observation, home_path, require_credential, write_fixture,
    RecordError, RecordingProvider, CREDENTIAL_ENV,
};
use super::common::scripted_provider::ScriptedProvider;
use super::common::session_replay::{
    chat_request, run_fixture, run_plan, ReplayFixture, ReplayPlan, ReplayProfile,
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

#[tokio::test]
async fn recording_provider_tees_every_event_and_request() {
    let inner = ScriptedProvider::new(vec![vec![text("alpha "), text("beta"), finish("stop")]]);
    let recorder = RecordingProvider::new(Box::new(inner));
    let handle = recorder.handle();

    let mut stream = recorder
        .stream(&chat_request("record this turn"))
        .await
        .expect("the inner provider answers");
    let mut forwarded = 0usize;
    while let Some(item) = stream.next().await {
        item.expect("the inner provider yields no error");
        forwarded += 1;
    }

    assert_eq!(forwarded, 3, "every event reaches the consumer");
    assert_eq!(
        handle.requests().len(),
        1,
        "the forwarded request is recorded"
    );
    let responses = handle.responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].len(), 3);
    assert!(
        handle.unmappable().is_none(),
        "every forwarded event has a fixture spelling"
    );
}

#[tokio::test]
async fn recorder_refuses_a_credential_shaped_fixture() {
    let mut fixture = ReplayFixture::named("s1_plain_turn").expect("load a text-only fixture");
    fixture.expect.assistant_text = "sk-proj-0123456789abcdefghijklmn".to_string();
    let directory = tempfile::tempdir().expect("scratch directory");
    let path = directory.path().join("credential.json");

    let error = write_fixture(&fixture, &path).expect_err("the writer must refuse this content");

    assert!(
        matches!(error, RecordError::CredentialShape(_)),
        "the refusal must be the credential class: {error}"
    );
    assert!(
        error.to_string().contains("credential shape"),
        "the refusal must name the reason: {error}"
    );
    assert!(
        !path.exists(),
        "a refused fixture must not be written at all"
    );
}

#[tokio::test]
async fn recorder_refuses_an_absolute_home_path() {
    let mut fixture = ReplayFixture::named("s1_plain_turn").expect("load a text-only fixture");
    fixture.expect.assistant_text =
        "the answer lives in /Users/example/private/notes.txt".to_string();
    let directory = tempfile::tempdir().expect("scratch directory");
    let path = directory.path().join("home_path.json");

    let error = write_fixture(&fixture, &path).expect_err("the writer must refuse this content");

    assert!(
        matches!(error, RecordError::HomePath(_)),
        "the refusal must be the home-path class: {error}"
    );
    assert!(
        error.to_string().contains("absolute home path"),
        "the refusal must name the reason: {error}"
    );
    assert!(!path.exists());
}

#[tokio::test]
async fn recorder_refuses_a_run_without_a_credential() {
    let guard = env_lock();
    let previous: Vec<(&str, Option<String>)> = CREDENTIAL_ENV
        .iter()
        .map(|name| (*name, std::env::var(name).ok()))
        .collect();
    for name in CREDENTIAL_ENV {
        std::env::remove_var(name);
    }

    let outcome = require_credential();

    for (name, value) in previous {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    drop(guard);

    let error = outcome.expect_err("no credential means no recording");
    assert_eq!(error, RecordError::NoCredential);
    assert!(
        error.to_string().contains("no provider credential"),
        "the refusal must say what is missing: {error}"
    );
}

#[tokio::test]
async fn recorded_fixture_loads_and_passes_without_a_key() {
    let plan = ReplayPlan {
        session_id: "recorded_round_trip".to_string(),
        user_input: "Record this turn and replay it.".to_string(),
        profile: ReplayProfile::Plain,
    };
    let recorder = RecordingProvider::new(Box::new(ScriptedProvider::new(vec![vec![
        text("recorded answer"),
        finish("stop"),
    ]])));
    let handle = recorder.handle();
    let run = run_plan(&plan, Box::new(recorder))
        .await
        .expect("the recorded turn runs");

    let fixture = fixture_from_observation(
        &plan,
        &run.observed,
        handle.responses(),
        Some("recorded from a scripted provider".to_string()),
    );
    let directory = tempfile::tempdir().expect("scratch directory");
    let path = directory.path().join(format!("{}.json", plan.session_id));
    write_fixture(&fixture, &path).expect("the recorded fixture is written");

    // Replayed with no provider and no key: the fixture is the only input.
    let loaded = ReplayFixture::load_path(&path).expect("the recorded fixture loads");
    let replayed = run_fixture(&loaded)
        .await
        .expect("the recorded fixture replays");
    if let Err(divergence) = loaded.compare(&replayed.observed) {
        panic!("a recorded fixture must replay to its own expectation: {divergence}");
    }
    assert_eq!(replayed.observed.terminal, "completed");
    assert_eq!(replayed.observed.request_count, plan_requests(&loaded));
}

/// The request count a recorded fixture declares.
fn plan_requests(fixture: &ReplayFixture) -> usize {
    fixture.expect.request_count
}

#[test]
fn recorder_patterns_match_the_shapes_they_refuse() {
    assert_eq!(
        credential_shape("sk-AbCdEfGhIjKlMnOpQrSt"),
        Some("sk-".to_string())
    );
    assert!(home_path("C:\\\\Users\\\\example\\\\notes.txt").is_some());
    assert!(
        credential_shape("Replay recorded hello with no secret").is_none(),
        "ordinary transcript text is not a credential"
    );
    assert!(
        home_path("tests/session_replay/fixtures/read_target.txt").is_none(),
        "a repository-relative path is not a home path"
    );
}
