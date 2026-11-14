//! The recording decorator for the replay tier (harness-practice-adoption
//! M001, work package D).
//!
//! A fixture should be **recorded** from a real run rather than invented, so
//! this module tees a real provider's stream while the shipped loop consumes
//! it. Two rules bound it:
//!
//! - **It is local-only.** Nothing in the routine sweep or in CI calls
//!   [`record_fixture`]; it requires an explicit environment opt-in *and* a
//!   provider credential, and it refuses rather than inventing a transcript.
//! - **It redacts at write.** A fixture whose content matches a credential
//!   shape, or carries an absolute home path, is refused with the reason —
//!   the same two rules `scripts/check_session_replay_fixtures.py` enforces
//!   over the committed corpus.
//!
//! The decorator itself is used by the **runner** as well: it is how a
//! scenario knows which requests the turn actually made, so two scenarios in
//! one process cannot observe each other's requests.

#![allow(dead_code)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::provider::{ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError};
use futures_util::StreamExt;

use super::session_replay::{
    FixtureKind, ObservedTurn, RecordedRequest, ReplayExpectation, ReplayFixture, ReplayPlan,
    ReplayProfile, ScenarioRun, ScriptedEvent, FIXTURE_SIZE_BOUND, FORMAT_VERSION,
};

/// The environment variable that selects recording.
pub const RECORD_ENV: &str = "CODEGG_SESSION_REPLAY_RECORD";

/// The credential variables a recording run may use, in order.
///
/// The recorder never reads the *value*: it only needs to know that a real
/// provider has something to authenticate with, so a run with no credential
/// records nothing instead of writing an empty transcript.
pub const CREDENTIAL_ENV: &[&str] = &[
    "CODEGG_REPLAY_RECORD_CREDENTIAL",
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "DEEPSEEK_API_KEY",
];

/// A provider that forwards to another one and records both directions.
pub struct RecordingProvider {
    inner: Box<dyn Provider>,
    state: Arc<Mutex<Recorded>>,
}

#[derive(Default)]
struct Recorded {
    requests: Vec<RecordedRequest>,
    responses: Vec<Vec<ChatEvent>>,
    /// The first event that had no fixture spelling, if any.
    unmappable: Option<String>,
}

/// A handle on what the decorator saw.
#[derive(Clone)]
pub struct RecordingHandle {
    state: Arc<Mutex<Recorded>>,
}

impl Clone for RecordingProvider {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
            state: Arc::clone(&self.state),
        }
    }
}

impl RecordingProvider {
    /// Wrap a provider.
    pub fn new(inner: Box<dyn Provider>) -> Self {
        Self {
            inner,
            state: Arc::new(Mutex::new(Recorded::default())),
        }
    }

    /// A handle that outlives the boxed provider.
    pub fn handle(&self) -> RecordingHandle {
        RecordingHandle {
            state: Arc::clone(&self.state),
        }
    }
}

impl RecordingHandle {
    /// Every request forwarded, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.lock().expect("recording state").requests.clone()
    }

    /// Every response event list forwarded, in order.
    pub fn responses(&self) -> Vec<Vec<ChatEvent>> {
        self.state
            .lock()
            .expect("recording state")
            .responses
            .clone()
    }

    /// The first event that could not be written back as a fixture event.
    pub fn unmappable(&self) -> Option<String> {
        self.state
            .lock()
            .expect("recording state")
            .unmappable
            .clone()
    }
}

#[async_trait]
impl Provider for RecordingProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        {
            let mut state = self.state.lock().expect("recording state");
            state.requests.push(RecordedRequest {
                roles: request.messages.iter().map(role_of).collect(),
                tools: request
                    .tools
                    .as_ref()
                    .map(|tools| tools.iter().map(|tool| tool.name.clone()).collect())
                    .unwrap_or_default(),
                request: request.clone(),
            });
        }

        let inner = self.inner.stream(request).await?;
        let state = Arc::clone(&self.state);
        let forwarded: Vec<ChatEvent> = Vec::new();
        // A true tee: each event is recorded as it is polled, so the loop sees
        // exactly the stream the inner provider produced rather than a
        // buffered replay of it.
        let stream = futures_util::stream::unfold(
            (inner, state, forwarded),
            |(mut inner, state, mut forwarded)| async move {
                match inner.next().await {
                    Some(item) => {
                        if let Ok(event) = &item {
                            if ScriptedEvent::from_chat_event(event).is_none() {
                                let mut guard = state.lock().expect("recording state");
                                if guard.unmappable.is_none() {
                                    guard.unmappable = Some(format!("{event:?}"));
                                }
                            }
                            forwarded.push(event.clone());
                        }
                        Some((item, (inner, state, forwarded)))
                    }
                    None => {
                        if !forwarded.is_empty() {
                            let mut guard = state.lock().expect("recording state");
                            guard.responses.push(std::mem::take(&mut forwarded));
                        }
                        None
                    }
                }
            },
        );
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.models().await
    }
}

fn role_of(message: &codegg::provider::Message) -> String {
    use codegg::provider::Message;
    match message {
        Message::System { .. } => "system",
        Message::User { .. } => "user",
        Message::Assistant { .. } => "assistant",
        Message::Tool { .. } => "tool",
    }
    .to_string()
}

// =============================================================================
// Writing a fixture
// =============================================================================

/// Why a recording could not become a fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// Recording was not requested.
    NotEnabled,
    /// Recording was requested but no credential is present.
    NoCredential,
    /// The run produced an event with no fixture spelling.
    Unmappable(String),
    /// The content matches a credential shape.
    CredentialShape(String),
    /// The content carries an absolute home path.
    HomePath(String),
    /// The serialized fixture exceeds the size bound.
    Oversized(usize),
    /// The write itself failed.
    Write(String),
}

impl fmt::Display for RecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnabled => write!(
                formatter,
                "recording is not enabled: set {RECORD_ENV}=1 to record a fixture"
            ),
            Self::NoCredential => write!(
                formatter,
                "recording refused: no provider credential is present ({})",
                CREDENTIAL_ENV.join(", ")
            ),
            Self::Unmappable(event) => write!(
                formatter,
                "recording refused: the run produced an event with no fixture spelling: {event}"
            ),
            Self::CredentialShape(reason) => write!(
                formatter,
                "recording refused: the content matches a credential shape ({reason})"
            ),
            Self::HomePath(reason) => write!(
                formatter,
                "recording refused: the content carries an absolute home path ({reason})"
            ),
            Self::Oversized(bytes) => write!(
                formatter,
                "recording refused: {bytes} bytes exceeds the {FIXTURE_SIZE_BOUND}-byte bound"
            ),
            Self::Write(message) => write!(formatter, "recording failed: {message}"),
        }
    }
}

impl std::error::Error for RecordError {}

/// Whether the environment asks for a recording run.
pub fn recording_enabled() -> bool {
    std::env::var(RECORD_ENV).is_ok_and(|value| value == "1")
}

/// The first credential variable that is present and non-empty.
pub fn credential_variable() -> Option<&'static str> {
    CREDENTIAL_ENV.iter().copied().find(|name| {
        std::env::var(name)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

/// Refuse a recording run that has no credential.
pub fn require_credential() -> Result<(), RecordError> {
    if credential_variable().is_some() {
        Ok(())
    } else {
        Err(RecordError::NoCredential)
    }
}

/// Record one turn with a real provider and write the fixture.
///
/// This is the local workflow, not a CI path: it refuses unless
/// `CODEGG_SESSION_REPLAY_RECORD=1` and a credential are both present.
pub async fn record_fixture(
    plan: &ReplayPlan,
    provider: Box<dyn Provider>,
    notes: Option<String>,
    path: &Path,
) -> Result<ReplayFixture, RecordError> {
    if !recording_enabled() {
        return Err(RecordError::NotEnabled);
    }
    require_credential()?;

    let recorder = RecordingProvider::new(provider);
    let handle = recorder.handle();
    let ScenarioRun { observed, .. } = super::session_replay::run_plan(plan, Box::new(recorder))
        .await
        .map_err(|error| RecordError::Write(error.to_string()))?;

    if let Some(event) = handle.unmappable() {
        return Err(RecordError::Unmappable(event));
    }

    let fixture = fixture_from_observation(plan, &observed, handle.responses(), notes);
    write_fixture(&fixture, path)?;
    Ok(fixture)
}

/// Build a fixture document from what a run actually produced.
pub fn fixture_from_observation(
    plan: &ReplayPlan,
    observed: &ObservedTurn,
    responses: Vec<Vec<ChatEvent>>,
    notes: Option<String>,
) -> ReplayFixture {
    let expect = ReplayExpectation::from_observed(observed);
    ReplayFixture {
        format: FORMAT_VERSION,
        id: plan.session_id.clone(),
        kind: FixtureKind::Scenario,
        notes,
        session_id: plan.session_id.clone(),
        user_input: plan.user_input.clone(),
        profile: plan.profile,
        responses: responses
            .iter()
            .map(|turn| {
                turn.iter()
                    .filter_map(ScriptedEvent::from_chat_event)
                    .collect()
            })
            .collect(),
        expect,
    }
}

impl ReplayExpectation {
    /// The expectation an observed turn satisfies.
    ///
    /// Recording is how a fixture is *produced*: the comparison then re-runs
    /// the same turn and requires it to agree, so a fixture recorded from a
    /// wrong turn fails its own scenario rather than blessing it.
    pub fn from_observed(observed: &ObservedTurn) -> Self {
        Self {
            assistant_text: observed.assistant_text.clone(),
            tool_calls: observed.tool_calls.clone(),
            tool_results: observed.tool_results.clone(),
            terminal: observed.terminal.clone(),
            request_count: observed.request_count,
            request_roles: observed.request_roles.clone(),
            tools_declared: observed.tools_declared.clone(),
        }
    }
}

/// Write a fixture, refusing credential-shaped content and home paths.
pub fn write_fixture(fixture: &ReplayFixture, path: &Path) -> Result<usize, RecordError> {
    let rendered = serde_json::to_string_pretty(fixture)
        .map_err(|error| RecordError::Write(error.to_string()))?;
    if let Some(reason) = credential_shape(&rendered) {
        return Err(RecordError::CredentialShape(reason));
    }
    if let Some(reason) = home_path(&rendered) {
        return Err(RecordError::HomePath(reason));
    }
    if rendered.len() > FIXTURE_SIZE_BOUND {
        return Err(RecordError::Oversized(rendered.len()));
    }
    let text = format!("{rendered}\n");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| RecordError::Write(format!("{}: {error}", parent.display())))?;
    }
    std::fs::write(path, &text)
        .map_err(|error| RecordError::Write(format!("{}: {error}", path.display())))?;
    Ok(text.len())
}

/// The credential shapes the recorder refuses.
///
/// Deliberately narrow: this is the *writer's* guard, and the committed
/// corpus is checked by `scripts/check_session_replay_fixtures.py`, which is
/// the authority.
pub fn credential_shape(text: &str) -> Option<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("sk-", r"sk-[A-Za-z0-9_-]{16,}"),
        ("aws-access-key", r"AKIA[0-9A-Z]{16}"),
        ("github-token", r"gh[pousr]_[A-Za-z0-9]{20,}"),
        ("slack-token", r"xox[baprs]-[A-Za-z0-9-]{10,}"),
        ("private-key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
        (
            "assigned-secret",
            r#"(?i)(?:api[_-]?key|secret|access[_-]?token|password)"\s*:\s*"[A-Za-z0-9_\-]{16,}""#,
        ),
    ];
    first_match(PATTERNS, text)
}

/// The absolute home paths the recorder refuses.
pub fn home_path(text: &str) -> Option<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("unix-home", r"/Users/[A-Za-z0-9._-]+/"),
        ("linux-home", r"/home/[A-Za-z0-9._-]+/"),
        ("windows-home", r"[A-Za-z]:\\\\Users\\\\[A-Za-z0-9._-]+"),
    ];
    first_match(PATTERNS, text)
}

fn first_match(patterns: &[(&str, &str)], text: &str) -> Option<String> {
    for (label, pattern) in patterns {
        let compiled = regex::Regex::new(pattern).expect("recorder pattern compiles");
        if compiled.is_match(text) {
            return Some((*label).to_string());
        }
    }
    None
}

/// The fixture directory a caller should write into.
pub fn fixture_path(id: &str) -> PathBuf {
    super::session_replay::fixture_dir().join(format!("{id}.json"))
}

/// The tool names a profile declares, for a recorder that needs to check its
/// own output against the format.
pub fn declared_tools(profile: ReplayProfile) -> Vec<String> {
    profile.tool_names()
}
