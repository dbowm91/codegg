//! Recorded-session replay: the fixture format, its test-local DTO, and the
//! scenario runner (harness-practice-adoption M001).
//!
//! A **fixture** is one recorded turn: a schema version, a scenario identity,
//! the user input, the ordered scripted responses a model produced, and the
//! durable outcome the turn must reproduce. A **scenario** is one fixture
//! driven through the shipped agent loop with the network unavailable.
//!
//! Three properties hold here by construction:
//!
//! - **The model adapter is the only mocked boundary.** The scripted provider
//!   lives in this shared tree and answers from the fixture. The tool
//!   registry, the tool broker, the session store and the projection are the
//!   real implementations, built the same way a turn builds them.
//! - **Nothing here ships.** This module is compiled only into integration
//!   test binaries, so no production target gains a feature, a configuration
//!   key, a protocol variant or a dependency. `scripts/check_session_replay_fixtures.py`
//!   fails if a non-test path ever names it.
//! - **The production `ChatEvent` gains no serde contract.** [`ScriptedEvent`]
//!   is a mirror that lowers into `Vec<Vec<ChatEvent>>`; the production type
//!   is untouched.
//!
//! The comparison is over semantic fields only: role, text, tool name,
//! normalized arguments, tool outcome class, terminal state, request count,
//! the message-role shape of every request, and the tool names the request
//! declared. Timestamps, row ids that the fixture did not supply, and
//! durations are excluded on purpose — see `architecture/testing.md`.

#![allow(dead_code)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use codegg::agent::r#loop::AgentLoop;
use codegg::agent::transcript_capture::{AgentRunTranscriptCapture, TranscriptCheckpoint};
use codegg::agent::Agent;
use codegg::config::schema::Config;
use codegg::context::InMemoryArtifactStore;
use codegg::error::AppError;
use codegg::permission::{PermissionChecker, PermissionLevel, PermissionRuleset};
use codegg::provider::{ChatEvent, ChatRequest, ContentPart, Message, Provider, ToolCall};
use codegg::session::message::{
    Message as SessionMessage, MessageRole as SessionMessageRole, PartData as SessionPartData,
    ToolStatus as SessionToolStatus,
};
use codegg::session::{CreateSession, MessageStore, SessionStore};
use codegg::tool::read::ReadTool;
use codegg::tool::ToolRegistry;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// The fixture-format version this reader understands.
///
/// A fixture-format version is **not** a protocol version: it describes this
/// document shape only, and a later version adds a successor rather than
/// rewriting an existing fixture in place.
pub const FORMAT_VERSION: u32 = 1;

/// The most bytes one fixture may occupy.
///
/// One turn, text only. A fixture that needs more has grown into a different
/// artifact with a different bound, and the answer is a smaller scenario.
pub const FIXTURE_SIZE_BOUND: usize = 8 * 1024;

/// Fixture directory, relative to the root package.
pub const FIXTURE_DIR: &str = "tests/session_replay/fixtures";

/// The file the `read` profile reads, relative to the root package.
///
/// Committed, tiny and stable: a scenario that read a workspace file whose
/// contents move would be recording the repository, not the turn.
pub const READ_TARGET: &str = "tests/session_replay/fixtures/read_target.txt";

/// The root package directory, resolved once.
pub fn package_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The absolute path of the fixture directory.
pub fn fixture_dir() -> PathBuf {
    package_root().join(FIXTURE_DIR)
}

// =============================================================================
// The fixture DTO
// =============================================================================

/// One event in a scripted response.
///
/// The tag is the `type` field, and the set is closed: an unknown variant is
/// a load failure that names it, never a skipped event. `ChatEvent::Error` is
/// deliberately unsupported — the loop only logs it, so a fixture that
/// scripted one could not be compared to any durable outcome.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScriptedEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
    },
    Finish {
        stop_reason: String,
    },
}

impl ScriptedEvent {
    /// Lower one fixture event into the production event the provider serves.
    fn to_chat_event(&self) -> ChatEvent {
        match self {
            Self::TextDelta { text } => ChatEvent::TextDelta(Arc::new(text.clone())),
            Self::ReasoningDelta { text } => ChatEvent::ReasoningDelta(Arc::new(text.clone())),
            Self::ToolCall {
                id,
                name,
                arguments,
            } => ChatEvent::ToolCall(ToolCall {
                id: Arc::new(id.clone()),
                name: Arc::new(name.clone()),
                arguments: arguments.clone(),
            }),
            Self::ToolResult {
                tool_call_id,
                content,
            } => ChatEvent::ToolResult {
                tool_call_id: Arc::new(tool_call_id.clone()),
                content: Arc::new(content.clone()),
            },
            Self::Finish { stop_reason } => ChatEvent::Finish {
                stop_reason: Arc::new(stop_reason.clone()),
                usage: Default::default(),
            },
        }
    }

    /// Mirror a production event back into the fixture vocabulary.
    ///
    /// `ChatEvent::Error` has no fixture spelling: the loop only logs it, so
    /// a recorder that met one refuses the write rather than dropping it.
    pub(super) fn from_chat_event(event: &ChatEvent) -> Option<Self> {
        match event {
            ChatEvent::TextDelta(text) => Some(Self::TextDelta {
                text: text.to_string(),
            }),
            ChatEvent::ReasoningDelta(text) => Some(Self::ReasoningDelta {
                text: text.to_string(),
            }),
            ChatEvent::ToolCall(call) => Some(Self::ToolCall {
                id: call.id.to_string(),
                name: call.name.to_string(),
                arguments: call.arguments.clone(),
            }),
            ChatEvent::ToolResult {
                tool_call_id,
                content,
            } => Some(Self::ToolResult {
                tool_call_id: tool_call_id.to_string(),
                content: content.to_string(),
            }),
            ChatEvent::Finish { stop_reason, .. } => Some(Self::Finish {
                stop_reason: stop_reason.to_string(),
            }),
            ChatEvent::Error(_) => None,
        }
    }
}

/// Which real tools the turn's registry holds.
///
/// The profile names an **admission** decision, not an implementation: both
/// arms register the production tool types, and the fixture chooses which of
/// them the turn may see.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayProfile {
    /// No model-facing tools.
    #[default]
    Plain,
    /// The real `read` tool, rooted at the package directory.
    Read,
}

impl ReplayProfile {
    /// The ordered, sorted names the profile admits.
    pub fn tool_names(self) -> Vec<String> {
        match self {
            Self::Plain => vec![],
            Self::Read => vec!["read".to_string()],
        }
    }
}

/// One expected tool call, as the durable transcript records it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExpectedToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// One expected tool result, as an outcome class.
///
/// The class is `success` or `failure`; the output itself is not compared
/// here, because a fixture that pinned a tool's whole output would be a
/// fixture about that tool. S3's own test asserts that the model's next
/// request carries the real result.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExpectedToolResult {
    pub tool: String,
    pub outcome: String,
}

/// The durable outcome a scenario must reproduce.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplayExpectation {
    /// The assistant text the durable transcript must carry.
    pub assistant_text: String,
    /// Tool calls, in durable order.
    #[serde(default)]
    pub tool_calls: Vec<ExpectedToolCall>,
    /// Tool results, as outcome classes, in execution order.
    #[serde(default)]
    pub tool_results: Vec<ExpectedToolResult>,
    /// `completed`, or `failed:<class>` naming the failure class.
    pub terminal: String,
    /// How many model requests the turn made.
    pub request_count: usize,
    /// The message-role shape of each request, in order.
    ///
    /// This is the prompt-assembly surface: a change that appends, drops or
    /// reorders a message changes this list and fails the scenario.
    #[serde(default)]
    pub request_roles: Vec<Vec<String>>,
    /// The tool names the first request declared, as an unordered set.
    ///
    /// A change to tool admission changes this list and fails the scenario.
    #[serde(default)]
    pub tools_declared: Vec<String>,
}

/// What a fixture document is for.
///
/// A **scenario** is run by exactly one family member against the shipped
/// loop. A **decode** fixture exists to pin the document format against the
/// production event type; it is loaded by exactly one member and is never
/// driven, because its expectation describes a decode, not a turn.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FixtureKind {
    /// A recorded turn the family runs.
    #[default]
    Scenario,
    /// A document-format fixture the family decodes.
    Decode,
}

/// One recorded turn.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplayFixture {
    /// Fixture-format version. Not a protocol version.
    pub format: u32,
    /// Scenario identity; must equal the fixture's file stem.
    pub id: String,
    /// Whether this document is a scenario or a decode fixture.
    #[serde(default, skip_serializing_if = "is_default_kind")]
    pub kind: FixtureKind,
    /// Free-form provenance for a human.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// The session the turn writes into.
    pub session_id: String,
    /// The user input the turn is opened with.
    pub user_input: String,
    /// Which real tools the turn's registry admits.
    #[serde(default)]
    pub profile: ReplayProfile,
    /// The scripted responses, one list per model request.
    pub responses: Vec<Vec<ScriptedEvent>>,
    /// The durable outcome the turn must reproduce.
    pub expect: ReplayExpectation,
}

fn is_default_kind(kind: &FixtureKind) -> bool {
    *kind == FixtureKind::Scenario
}

impl ReplayFixture {
    /// Load `<FIXTURE_DIR>/<id>.json`.
    ///
    /// Every failure names the path and the reason, and a partially loaded
    /// fixture is never returned.
    pub fn named(id: &str) -> Result<Self, ReplayError> {
        let path = fixture_dir().join(format!("{id}.json"));
        Self::load_path(&path)
    }

    /// Load one fixture file, enforcing the size bound and the format version.
    pub fn load_path(path: &Path) -> Result<Self, ReplayError> {
        let bytes = std::fs::read(path)
            .map_err(|error| ReplayError::Fixture(format!("{}: {error}", path.display())))?;
        if bytes.len() > FIXTURE_SIZE_BOUND {
            return Err(ReplayError::Fixture(format!(
                "{}: {} bytes exceeds the {FIXTURE_SIZE_BOUND}-byte fixture bound",
                path.display(),
                bytes.len()
            )));
        }
        let text = String::from_utf8(bytes).map_err(|error| {
            ReplayError::Fixture(format!("{}: not UTF-8: {error}", path.display()))
        })?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        Self::parse(&text, &path.display().to_string(), &stem)
    }

    /// Parse a fixture document. `origin` is used only in error messages.
    pub fn parse(text: &str, origin: &str, expected_id: &str) -> Result<Self, ReplayError> {
        let fixture: Self = serde_json::from_str(text)
            .map_err(|error| ReplayError::Fixture(format!("{origin}: {error}")))?;
        if fixture.format != FORMAT_VERSION {
            return Err(ReplayError::Fixture(format!(
                "{origin}: unsupported fixture format {}; this reader understands format \
                 {FORMAT_VERSION}",
                fixture.format
            )));
        }
        if fixture.id != expected_id {
            return Err(ReplayError::Fixture(format!(
                "{origin}: fixture id `{}` does not match `{expected_id}`",
                fixture.id
            )));
        }
        Ok(fixture)
    }

    /// The scripted turns, lowered into the shape the provider serves.
    pub fn lowered_responses(&self) -> Vec<Vec<ChatEvent>> {
        self.responses
            .iter()
            .map(|turn| turn.iter().map(ScriptedEvent::to_chat_event).collect())
            .collect()
    }

    /// The request spec a runner needs, independent of the scripted answers.
    pub fn plan(&self) -> ReplayPlan {
        ReplayPlan {
            session_id: self.session_id.clone(),
            user_input: self.user_input.clone(),
            profile: self.profile,
        }
    }

    /// Compare an observed turn against this fixture's expectation.
    ///
    /// The first divergence is named by field, with the expected and actual
    /// rows printed beside it.
    pub fn compare(&self, observed: &ObservedTurn) -> Result<(), ReplayDivergence> {
        let base = || ReplayDivergence {
            fixture_id: self.id.clone(),
            expected_rows: render_expected_rows(&self.user_input, &self.expect),
            actual_rows: observed.render_actual(),
            field: String::new(),
            expected: String::new(),
            actual: String::new(),
        };

        if observed.user_text != self.user_input {
            return Err(ReplayDivergence {
                field: "user_text".to_string(),
                expected: self.user_input.clone(),
                actual: observed.user_text.clone(),
                ..base()
            });
        }
        if observed.terminal != self.expect.terminal {
            return Err(ReplayDivergence {
                field: "terminal".to_string(),
                expected: self.expect.terminal.clone(),
                actual: observed.terminal.clone(),
                ..base()
            });
        }
        if observed.assistant_text != self.expect.assistant_text {
            return Err(ReplayDivergence {
                field: "assistant_text".to_string(),
                expected: self.expect.assistant_text.clone(),
                actual: observed.assistant_text.clone(),
                ..base()
            });
        }
        if observed.tool_calls != self.expect.tool_calls {
            return Err(ReplayDivergence {
                field: "tool_calls".to_string(),
                expected: render_tool_calls(&self.expect.tool_calls),
                actual: render_tool_calls(&observed.tool_calls),
                ..base()
            });
        }
        if observed.tool_results != self.expect.tool_results {
            return Err(ReplayDivergence {
                field: "tool_results".to_string(),
                expected: render_tool_results(&self.expect.tool_results),
                actual: render_tool_results(&observed.tool_results),
                ..base()
            });
        }
        if observed.request_count != self.expect.request_count {
            return Err(ReplayDivergence {
                field: "request_count".to_string(),
                expected: self.expect.request_count.to_string(),
                actual: observed.request_count.to_string(),
                ..base()
            });
        }
        if observed.request_roles != self.expect.request_roles {
            return Err(ReplayDivergence {
                field: "request_roles".to_string(),
                expected: render_roles(&self.expect.request_roles),
                actual: render_roles(&observed.request_roles),
                ..base()
            });
        }
        // Declaration order is a registry implementation detail, so the
        // comparison is over the set, not the sequence.
        let mut expected_tools = self.expect.tools_declared.clone();
        expected_tools.sort();
        let mut actual_tools = observed.tools_declared.clone();
        actual_tools.sort();
        if actual_tools != expected_tools {
            return Err(ReplayDivergence {
                field: "tools_declared".to_string(),
                expected: expected_tools.join(", "),
                actual: actual_tools.join(", "),
                ..base()
            });
        }
        Ok(())
    }
}

// =============================================================================
// What a run observes
// =============================================================================

/// One tool call the durable transcript carries.
pub type ObservedToolCall = ExpectedToolCall;

/// One tool outcome the loop reported.
pub type ObservedToolResult = ExpectedToolResult;

/// The semantic result of one replayed turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedTurn {
    /// The prompt the durable transcript carries.
    pub user_text: String,
    /// The assistant text the durable transcript carries.
    pub assistant_text: String,
    /// Tool calls, in durable order.
    pub tool_calls: Vec<ObservedToolCall>,
    /// Tool outcomes, in the order the loop delivered them.
    pub tool_results: Vec<ObservedToolResult>,
    /// `completed`, or `failed:<class>`.
    pub terminal: String,
    /// How many model requests the turn made.
    pub request_count: usize,
    /// The message-role shape of every request.
    pub request_roles: Vec<Vec<String>>,
    /// The tool names the first request declared.
    pub tools_declared: Vec<String>,
    /// Every durable row, rendered for a failure message.
    rows: Vec<String>,
    /// The rendered failure, when the turn failed.
    pub error: Option<String>,
    /// The number of durable tool-call parts whose status was not `Pending`.
    ///
    /// The shipped loop persists a tool call with `status: Pending` and no
    /// output: its durable transcript builder reads `ChatEvent::ToolResult`,
    /// which the loop never emits. Tool outcomes are therefore compared from
    /// the loop's own capture checkpoints, and this count pins the durable
    /// fact as it is rather than pretending it is richer.
    pub durable_tool_statuses_resolved: usize,
}

impl ObservedTurn {
    fn render_actual(&self) -> String {
        format!("[{}]", self.rows.join(", "))
    }
}

/// The durable rows the fixture declares, rendered for a failure message.
fn render_expected_rows(user_input: &str, expect: &ReplayExpectation) -> String {
    let mut rows = vec![format!("user: {user_input:?}")];
    rows.push(format!("assistant: {:?}", expect.assistant_text));
    for call in &expect.tool_calls {
        rows.push(format!(
            "tool_call({}, {})",
            call.name,
            normalize(&call.arguments)
        ));
    }
    format!("[{}]", rows.join(", "))
}

/// A request the runner's decorator observed.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    /// The message-role shape.
    pub roles: Vec<String>,
    /// The tool names the request declared.
    pub tools: Vec<String>,
    /// The verbatim request, for a scenario that asserts what the model saw.
    pub request: ChatRequest,
}

/// Everything one scenario produced.
#[derive(Debug, Clone)]
pub struct ScenarioRun {
    /// The comparable observation.
    pub observed: ObservedTurn,
    /// Every model request the turn made, in order.
    pub requests: Vec<RecordedRequest>,
}

/// The request specification for one turn, independent of who answers it.
#[derive(Debug, Clone)]
pub struct ReplayPlan {
    pub session_id: String,
    pub user_input: String,
    pub profile: ReplayProfile,
}

// =============================================================================
// The runner
// =============================================================================

/// Load the named fixture and run it through the shipped agent loop.
///
/// This is the call the guard counts: a fixture must be run by exactly one
/// family member, so the corpus cannot grow a fixture nothing exercises.
pub async fn run_scenario(id: &str) -> Result<(ReplayFixture, ScenarioRun), ReplayError> {
    let fixture = ReplayFixture::named(id)?;
    let run = run_fixture(&fixture).await?;
    Ok((fixture, run))
}

/// Run one fixture's scripted turns through the shipped agent loop.
pub async fn run_fixture(fixture: &ReplayFixture) -> Result<ScenarioRun, ReplayError> {
    use crate::common::scripted_provider::{Exhaustion, ScriptedProvider};

    let provider =
        ScriptedProvider::new(fixture.lowered_responses()).with_exhaustion(Exhaustion::Fail);
    run_plan(&fixture.plan(), Box::new(provider)).await
}

/// Drive one turn with the supplied provider.
///
/// The provider is wrapped in the request recorder, so the returned run owns
/// its own observation and two scenarios in one process cannot read each
/// other's requests.
pub async fn run_plan(
    plan: &ReplayPlan,
    provider: Box<dyn Provider>,
) -> Result<ScenarioRun, ReplayError> {
    use crate::common::replay_recorder::RecordingProvider;

    let recorder = RecordingProvider::new(provider);
    let handle = recorder.handle();
    let workspace_root = package_root().to_path_buf();
    let (pool, reopened_pool) = replay_pools().await?;

    SessionStore::new(pool.clone())
        .create_with_id(
            &plan.session_id,
            CreateSession {
                project_id: "session-replay".to_string(),
                directory: workspace_root.display().to_string(),
                title: Some(format!("session replay {}", plan.session_id)),
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
        .map_err(|error| ReplayError::Run(format!("create replay session: {error}")))?;

    let checkpoints = CheckpointLog::default();
    let mut agent_loop = build_loop(Box::new(recorder), plan, &workspace_root, pool).await;
    agent_loop.set_transcript_capture(Some(Arc::new(checkpoints.clone())));

    let result = agent_loop.run(chat_request(&plan.user_input)).await;
    let terminal = terminal_of(&result);
    let error = result.as_ref().err().map(|error| error.to_string());

    // Read the turn back through a **freshly opened handle**: a row that only
    // exists in the loop's memory is absent here, and the comparison fails.
    let rows = read_durable_rows(&reopened_pool, &plan.session_id).await?;

    let requests = handle.requests();
    let observed = observe(&rows, &requests, &checkpoints, terminal, error);
    Ok(ScenarioRun { observed, requests })
}

async fn build_loop(
    provider: Box<dyn Provider>,
    plan: &ReplayPlan,
    workspace_root: &Path,
    pool: SqlitePool,
) -> AgentLoop {
    let agents = vec![Agent {
        git_read_only: false,
        name: "build".to_string(),
        role: None,
        description: "Replay agent".to_string(),
        mode: codegg::agent::AgentMode::Primary,
        mode_name: None,
        model: None,
        variant: None,
        temperature: None,
        top_p: None,
        color: None,
        steps: None,
        system_prompt: None,
        permissions: std::collections::HashMap::new(),
        hidden: false,
        thinking_budget: None,
        reasoning_effort: None,
        fallback_model: None,
        runtime_kind: None,
    }];
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        });

    let mut agent_loop = AgentLoop::new(
        agents,
        provider,
        permission_checker,
        build_registry(plan.profile, workspace_root),
        Config::default(),
        None,
        Some(pool),
        Arc::new(InMemoryArtifactStore::new()),
        workspace_root.to_path_buf(),
        plan.session_id.clone(),
        None,
        None,
    );
    // The broker's authority stage refuses a workspace-scoped tool when the
    // turn holds no verified workspace path policy, so a harness that skipped
    // this would be measuring a denial rather than a tool. The registry is the
    // same in-memory one the agent-loop harness builds; only the identity is
    // registered, no daemon is involved.
    attach_workspace_authority(&mut agent_loop, workspace_root)
        .await
        .expect("replay workspace authority");
    agent_loop
}

async fn attach_workspace_authority(
    agent_loop: &mut AgentLoop,
    workspace_root: &Path,
) -> Result<(), ReplayError> {
    let registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .map_err(|error| ReplayError::Run(format!("replay workspace registry: {error}")))?;
    let workspace = registry
        .get_or_register(workspace_root)
        .await
        .map_err(|error| ReplayError::Run(format!("register replay workspace: {error}")))?;
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let lease = services
        .acquire(&workspace.id)
        .await
        .map_err(|error| ReplayError::Run(format!("replay workspace lease: {error}")))?;
    agent_loop.set_workspace_id(workspace.id.clone());
    agent_loop.set_workspace_services_lease(lease);
    Ok(())
}

fn build_registry(profile: ReplayProfile, workspace_root: &Path) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    match profile {
        ReplayProfile::Plain => {}
        ReplayProfile::Read => {
            registry.register(ReadTool::new().with_allowed_root(workspace_root.to_path_buf()));
        }
    }
    registry
}

pub fn chat_request(user_input: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: Arc::new(user_input.to_string()),
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
    }
}

/// One durable transcript row, as the loop wrote it.
#[derive(Debug, Clone)]
pub struct DurableRow {
    pub role: String,
    pub text: String,
    pub tool_calls: Vec<ObservedToolCall>,
    /// How many tool-call parts carried an outcome.
    ///
    /// The shipped loop leaves this at zero: it persists a tool call with
    /// `status: pending` and no output, because its transcript builder reads
    /// `ChatEvent::ToolResult` and the loop never emits one. Tool outcomes are
    /// therefore compared from the loop's own capture checkpoints, and this
    /// count keeps the durable fact visible instead of implying it is richer.
    pub resolved_tool_statuses: usize,
}

impl DurableRow {
    fn render(&self) -> Vec<String> {
        let mut rendered = Vec::new();
        if !self.text.is_empty() {
            rendered.push(format!("{}: {:?}", self.role, self.text));
        }
        for call in &self.tool_calls {
            rendered.push(format!(
                "tool_call({}, {})",
                call.name,
                normalize(&call.arguments)
            ));
        }
        rendered
    }
}

/// Read the durable transcript back through the **production reader**.
///
/// `MessageStore::list` is the shipped decode path and the canonical row order
/// (`ORDER BY time_created ASC, id ASC`), so a row the reader cannot decode
/// fails the scenario instead of being papered over by a decoder of this
/// module's own. An earlier revision duplicated the decode here because
/// `MessageData` could not decode a tool-call part at all: `PartInfo` and
/// `PartData::ToolCall` shared the wire name `id` under `#[serde(flatten)]`,
/// which serialized to a duplicate key. The tool-call id now has its own wire
/// name (`toolCallID`), the duplicate decoder is gone, and a decode regression
/// is a scenario failure.
pub async fn read_durable_rows(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<DurableRow>, ReplayError> {
    let messages = MessageStore::new(pool.clone())
        .list(session_id)
        .await
        .map_err(|error| ReplayError::Run(format!("read replay session back: {error}")))?;
    Ok(messages.iter().map(durable_row).collect())
}

/// Project one decoded message onto the semantic row the fixtures compare.
fn durable_row(message: &SessionMessage) -> DurableRow {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut resolved_tool_statuses = 0usize;
    for part in &message.data.parts {
        match &part.data {
            SessionPartData::Text { text: body } => text.push_str(body),
            SessionPartData::ToolCall {
                name,
                input,
                output,
                status,
                ..
            } => {
                tool_calls.push(ObservedToolCall {
                    name: name.clone(),
                    arguments: input.clone(),
                });
                if output.is_some() || *status != SessionToolStatus::Pending {
                    resolved_tool_statuses += 1;
                }
            }
            _ => {}
        }
    }
    DurableRow {
        role: match message.data.role {
            Some(SessionMessageRole::User) => "user",
            Some(SessionMessageRole::Assistant) => "assistant",
            None => "unknown",
        }
        .to_string(),
        text,
        tool_calls,
        resolved_tool_statuses,
    }
}

/// Turn the raw surfaces into the comparable observation.
fn observe(
    rows: &[DurableRow],
    requests: &[RecordedRequest],
    checkpoints: &CheckpointLog,
    terminal: String,
    error: Option<String>,
) -> ObservedTurn {
    let mut user_text = String::new();
    let mut assistant_text = String::new();
    let mut tool_calls = Vec::new();
    let mut rendered_rows = Vec::new();
    let mut durable_tool_statuses_resolved = 0usize;

    for row in rows {
        rendered_rows.extend(row.render());
        for call in &row.tool_calls {
            tool_calls.push(call.clone());
        }
        durable_tool_statuses_resolved += row.resolved_tool_statuses;
        match row.role.as_str() {
            "user" if user_text.is_empty() => user_text = row.text.clone(),
            "assistant" => assistant_text.push_str(&row.text),
            _ => {}
        }
    }

    let tool_results = checkpoints.tool_results();
    let request_roles: Vec<Vec<String>> = requests.iter().map(|r| r.roles.clone()).collect();
    let tools_declared = requests
        .first()
        .map(|r| r.tools.clone())
        .unwrap_or_default();

    ObservedTurn {
        user_text,
        assistant_text,
        tool_calls,
        tool_results,
        terminal,
        error,
        request_count: requests.len(),
        request_roles,
        tools_declared,
        rows: rendered_rows,
        durable_tool_statuses_resolved,
    }
}

fn terminal_of(result: &Result<Vec<ChatEvent>, AppError>) -> String {
    match result {
        Ok(_) => "completed".to_string(),
        Err(error) => format!("failed:{}", failure_class(error)),
    }
}

/// The stable failure class a scenario declares.
fn failure_class(error: &AppError) -> &'static str {
    match error {
        AppError::Provider(_) => "provider",
        AppError::Agent(_) => "agent",
        AppError::Tool(_) => "tool",
        AppError::Permission(_) => "permission",
        AppError::Storage(_) => "storage",
        AppError::Config(_) => "config",
        AppError::Mcp(_) => "mcp",
        AppError::Plugin(_) => "plugin",
        AppError::Lsp(_) => "lsp",
        AppError::Io(_) => "io",
        AppError::Json(_) => "json",
        AppError::Http(_) => "http",
        AppError::Worktree(_) => "worktree",
        AppError::Upgrade(_) => "upgrade",
        AppError::Clipboard(_) => "clipboard",
        AppError::Tui(_) => "tui",
        AppError::RunStore(_) => "run_store",
        AppError::Cancelled => "cancelled",
        AppError::Other(_) => "other",
    }
}

// =============================================================================
// Capture checkpoints
// =============================================================================

/// The loop's own record of what it delivered back to the model.
///
/// This is the production seam (`AgentLoop::set_transcript_capture`), not a
/// stub: the delegated-run path consumes the same checkpoints, so a tool
/// outcome read here is the outcome the loop actually reported.
#[derive(Clone, Default)]
pub struct CheckpointLog {
    entries: Arc<Mutex<Vec<TranscriptCheckpoint>>>,
}

impl CheckpointLog {
    /// The tool outcomes the loop delivered, in order.
    fn tool_results(&self) -> Vec<ObservedToolResult> {
        self.entries
            .lock()
            .expect("checkpoint log")
            .iter()
            .filter_map(|checkpoint| match checkpoint {
                TranscriptCheckpoint::ToolResult {
                    tool_name, success, ..
                } => Some(ObservedToolResult {
                    tool: tool_name.clone(),
                    outcome: if *success { "success" } else { "failure" }.to_string(),
                }),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl AgentRunTranscriptCapture for CheckpointLog {
    async fn record(&self, checkpoint: TranscriptCheckpoint) {
        self.entries
            .lock()
            .expect("checkpoint log")
            .push(checkpoint);
    }
}

// =============================================================================
// Errors, divergence, helpers
// =============================================================================

/// A fixture could not be loaded, or a scenario could not be run.
#[derive(Debug, Clone)]
pub enum ReplayError {
    /// The fixture document is malformed, oversized or unsupported.
    Fixture(String),
    /// The turn could not be driven.
    Run(String),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixture(message) => write!(formatter, "fixture: {message}"),
            Self::Run(message) => write!(formatter, "scenario: {message}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// The first field that diverged, with both rows printed beside it.
#[derive(Debug, Clone)]
pub struct ReplayDivergence {
    pub fixture_id: String,
    pub field: String,
    pub expected: String,
    pub actual: String,
    pub expected_rows: String,
    pub actual_rows: String,
}

impl fmt::Display for ReplayDivergence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "scenario `{}` diverged at `{}`\n  expected: {}\n  actual:   {}\n  \
             expected rows: {}\n  actual rows:   {}",
            self.fixture_id,
            self.field,
            self.expected,
            self.actual,
            self.expected_rows,
            self.actual_rows
        )
    }
}

impl std::error::Error for ReplayDivergence {}

fn normalize(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unencodable>".to_string())
}

fn render_tool_calls(calls: &[ObservedToolCall]) -> String {
    let rendered: Vec<String> = calls
        .iter()
        .map(|call| format!("{}({})", call.name, normalize(&call.arguments)))
        .collect();
    format!("[{}]", rendered.join(", "))
}

fn render_tool_results(results: &[ObservedToolResult]) -> String {
    let rendered: Vec<String> = results
        .iter()
        .map(|result| format!("{}:{}", result.tool, result.outcome))
        .collect();
    format!("[{}]", rendered.join(", "))
}

fn render_roles(roles: &[Vec<String>]) -> String {
    let rendered: Vec<String> = roles
        .iter()
        .map(|request| format!("[{}]", request.join(" > ")))
        .collect();
    format!("[{}]", rendered.join(", "))
}

/// Two pools over one named in-memory database.
///
/// The second is the **reopened handle** a scenario reads through; it runs no
/// migration, so the rows it sees are the ones the first pool's turn wrote.
pub async fn replay_pools() -> Result<(SqlitePool, SqlitePool), ReplayError> {
    let name = format!("codegg_replay_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let primary = connect(&url)
        .await
        .map_err(|error| ReplayError::Run(format!("replay store: {error}")))?;
    codegg::session::schema::migrate(&primary)
        .await
        .map_err(|error| ReplayError::Run(format!("replay store migration: {error}")))?;
    let reopened = connect(&url)
        .await
        .map_err(|error| ReplayError::Run(format!("replay store reopen: {error}")))?;
    Ok((primary, reopened))
}

async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

/// The longest a scenario may take before the test calls it a hang.
pub const SCENARIO_TIMEOUT: Duration = Duration::from_secs(30);
