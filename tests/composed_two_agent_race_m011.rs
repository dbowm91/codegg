//! M011 — the composed two-agent daemon race.
//!
//! M005's last remaining non-claim is that two bounded agents sharing **one**
//! checkout cannot write it at the same time. C006 proved the writer permit
//! refuses a second writer while the first holds it — but it drove the fence
//! **directly**, so it proved the mechanism and not the composition. This
//! suite drives the composition:
//!
//! * one process, one group, one shared checkout;
//! * the checkout is allocated through the **real** `WorktreeService::create`
//!   path and is a real linked Git worktree — Git itself reports it on the
//!   recorded branch, so the worktree service's own validation accepts it;
//! * two **live** agents, each with its own name and its own model, each
//!   issuing a real mutating broker operation — the production `write` tool,
//!   through the production broker, into the shared checkout;
//! * the writer fence is the production `DaemonWriterFence`, one per run,
//!   resolved from the run's durable record.
//!
//! What it proves:
//!
//! * the two operations **never overlapped** — the permit timeline shows one
//!   permit released before the other was taken;
//! * the race was **fair** — both agents were inside their turn at the same
//!   time, so the pass is not an artifact of one agent running to completion
//!   before the other started;
//! * both writes landed, so the serialization did not come from a refusal;
//! * a run outside the group may not write the shared checkout at all.
//!
//! What it does **not** claim:
//!
//! * Nothing about containment. The checkout is created by a fixture that runs
//!   Git directly; the C008 containment contract is proved by its own suite.
//! * Nothing about merging. Integration remains an explicit, typed step that
//!   CodeGG never performs on its own.

// The shared helper tree is bound **publicly**. A private `mod common;`
// makes its `pub` items unreachable from this crate root — and
// `tests/common/secret_scan.rs` carries no `allow(dead_code)` of its own,
// so a private binding reports every one of its items as dead code under
// `-D warnings`. The families bind it the same way.
pub mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Instant;

use codegg::agent::r#loop::AgentLoop;
use codegg::agent::{Agent, AgentMode};
use codegg::config::schema::Config;
use codegg::context::InMemoryArtifactStore;
use codegg::core::writer_fence::{local_principal, DaemonWriterFence};
use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
use codegg::permission::{PermissionChecker, PermissionLevel, PermissionRuleset};
use codegg::provider::{ChatEvent, ChatRequest, TokenUsage, ToolCall};
use codegg::tool::writer_fence::SharedWriterFence;
use codegg::tool::{ToolRegistry, ToolRegistryOptions};
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
};
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
};
use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use codegg_core::process_admission::{PersistentProcessRegistry, ProcessAdmissionService};
use codegg_core::shared_checkout::{
    SharedCheckoutCoordinator, SharedCheckoutOutcome, SharedCheckoutRequest,
};
use codegg_core::worktree_service::{CreateWorktreeRequest, WorktreeService};
use codegg_core::writer_permit::WriterPermitStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tracing::field::{Field, Visit};
use tracing_subscriber::prelude::*;

use common::linked_worktree::LinkedWorktreeCreator;
use common::scripted_provider::ScriptedProvider;

// ---------------------------------------------------------------------------
// The permit timeline
// ---------------------------------------------------------------------------

/// Which side of a permit an event describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PermitPhase {
    Acquired,
    Released,
}

/// One permit transition, as the daemon's fence reported it.
#[derive(Clone, Debug)]
struct PermitEvent {
    at: Instant,
    phase: PermitPhase,
    agent_run_id: String,
    worktree_id: String,
}

/// Collects the fields of a tracing event.
#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

/// The tracing layer that reads the fence's permit transitions.
struct PermitTimelineLayer {
    sink: Arc<Mutex<Vec<PermitEvent>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for PermitTimelineLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let Some(message) = visitor.fields.get("message").cloned() else {
            return;
        };
        let phase = if message.contains("took the checkout's writer permit") {
            PermitPhase::Acquired
        } else if message.contains("released the checkout's writer permit") {
            PermitPhase::Released
        } else {
            return;
        };
        let agent_run_id = visitor
            .fields
            .get("agent_run_id")
            .map(|value| unquote(value).to_string())
            .unwrap_or_default();
        let worktree_id = visitor
            .fields
            .get("worktree_id")
            .map(|value| unquote(value).to_string())
            .unwrap_or_default();
        self.sink
            .lock()
            .expect("the permit timeline")
            .push(PermitEvent {
                at: Instant::now(),
                phase,
                agent_run_id,
                worktree_id,
            });
    }
}

/// A `&str` field is recorded raw, a display field is recorded through its
/// `Debug` impl, which quotes it. The run id is compared either way.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
}

static TIMELINE: OnceLock<Arc<Mutex<Vec<PermitEvent>>>> = OnceLock::new();
static INSTALL_TIMELINE: Once = Once::new();

/// The process-wide permit timeline.
///
/// The fence reports every permit transition through `tracing`, and those
/// reports are the daemon's own account of what it did — the suite reads them
/// rather than asking the fence to behave differently for a test.
fn timeline() -> Arc<Mutex<Vec<PermitEvent>>> {
    INSTALL_TIMELINE.call_once(|| {
        let sink = Arc::new(Mutex::new(Vec::new()));
        let _ = tracing_subscriber::registry()
            .with(PermitTimelineLayer {
                sink: Arc::clone(&sink),
            })
            .try_init();
        let _ = TIMELINE.set(sink);
    });
    TIMELINE.get().expect("the permit timeline").clone()
}

/// Forget everything the timeline has seen so far.
fn drain_timeline() {
    timeline().lock().expect("the permit timeline").clear();
}

/// The permit transitions recorded for one checkout, in order.
fn timeline_for(worktree_id: &WorktreeId) -> Vec<PermitEvent> {
    timeline()
        .lock()
        .expect("the permit timeline")
        .iter()
        .filter(|event| event.worktree_id == worktree_id.as_str())
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------

/// A temporary directory that removes itself when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codegg-m011-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).expect("create the temp directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh in-memory database for one scenario.
///
/// The shared helper's single connection is not enough here: the permit
/// store holds a connection for the duration of its write transaction while
/// the scenario reads the same row to check the permit's end state, so the
/// pool carries more than one connection.
async fn isolated_pool() -> SqlitePool {
    let name = format!("composed_race_m011_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .expect("connect to the in-memory database");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate the database");
    pool
}

/// The composed scenario: one group, one shared checkout, three runs.
struct Race {
    pool: SqlitePool,
    repository_root: TempDir,
    _managed_root: TempDir,
    worktrees: Arc<WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Arc<AgentRunGroupService>,
    permits: Arc<WriterPermitService>,
    coordinator: Arc<SharedCheckoutCoordinator>,
    group_id: AgentRunGroupId,
    owner_run: AgentRunId,
    member_run: AgentRunId,
    outsider_run: AgentRunId,
    holder_run: AgentRunId,
    project_id: ProjectId,
    repository_id: RepositoryId,
    workspace_id: WorkspaceId,
    session_id: String,
}

impl Race {
    async fn new(label: &str) -> Self {
        let pool = isolated_pool().await;
        let repository_root = TempDir::new(&format!("{label}-repository"));
        common::blackboard::init_repository(repository_root.path());
        let managed_root = TempDir::new(&format!("{label}-managed"));

        let project_id = ProjectId::new();
        let repository_id = RepositoryId::new();
        let workspace_id = WorkspaceId::new();
        let owner_run = AgentRunId::new();
        let member_run = AgentRunId::new();
        let outsider_run = AgentRunId::new();
        let holder_run = AgentRunId::new();
        let group_id = AgentRunGroupId::new();
        let session_id = format!("session-{}", uuid::Uuid::new_v4().simple());
        let turn_id = format!("turn-{}", uuid::Uuid::new_v4().simple());

        // The worktree service is the real one, with a creator that produces a
        // real linked worktree. The checkout the coordinator allocates below
        // therefore goes through `WorktreeService::create` and is validated by
        // the service's own `git worktree list` check.
        let worktrees = WorktreeService::sqlite(
            pool.clone(),
            managed_root.path().to_path_buf(),
            Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
        )
        .with_contained_creator(Arc::new(LinkedWorktreeCreator::new()));

        let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
        let groups = AgentRunGroupService::in_memory(runs.clone());
        let permits = WriterPermitService::new(
            WriterPermitStore::new(pool.clone()),
            WorktreePermitAuthority::new(worktrees.clone()).with_groups(groups.clone()),
        );

        let reservations = codegg_core::checkout_reservation::CheckoutReservationService::new(
            worktrees.clone(),
            Some(groups.clone()),
        );
        let processes = PersistentProcessRegistry::new();
        let admission = ProcessAdmissionService::new(Some(reservations), processes.clone());
        let coordinator = SharedCheckoutCoordinator::new(
            Some(groups.clone()),
            Some(runs.clone()),
            Some(worktrees.clone()),
            Some(admission),
        );

        // Three real delegated runs under one turn. Two of them are the
        // group's members; the third is outside the group and holds a checkout
        // of its own.
        for (index, run_id) in [
            owner_run.clone(),
            member_run.clone(),
            outsider_run.clone(),
            holder_run.clone(),
        ]
        .into_iter()
        .enumerate()
        {
            let task = NewAgentTask {
                task_id: AgentTaskId::new(),
                parent_task_id: None,
                originating_session_id: session_id.clone(),
                originating_turn_id: Some(turn_id.clone()),
                project_id: project_id.clone(),
                repository_id: Some(repository_id.clone()),
                workspace_id: workspace_id.clone(),
                requested_agent: "build".to_owned(),
                delegation_key: format!("delegation-{index}-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: format!("fingerprint-{index}"),
                description: "m011 composed race".to_owned(),
            };
            let run = NewAgentRun {
                run_id,
                parent_run_id: None,
                depth: 1,
                workspace_id: workspace_id.clone(),
                agent_name: "build".to_owned(),
                agent_digest: None,
                provider: "scripted".to_owned(),
                model: "test".to_owned(),
                authority_digest: format!("authority-{index}"),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            };
            runs.create_or_get(task, run)
                .await
                .expect("create the delegated run");
        }

        groups
            .create(NewAgentRunGroup {
                group_id: group_id.clone(),
                root_run_id: owner_run.clone(),
                owner_run_id: owner_run.clone(),
                owner: AgentRunGroupOwner::Turn {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                },
                member_run_ids: vec![owner_run.clone(), member_run.clone()],
                join_policy: RunJoinPolicy::All,
                cancel_remaining_on_satisfaction: false,
                idempotency_key: format!("group-{}", uuid::Uuid::new_v4().simple()),
            })
            .await
            .expect("create the live group");

        Self {
            pool,
            repository_root,
            _managed_root: managed_root,
            worktrees,
            runs,
            groups,
            permits,
            coordinator,
            group_id,
            owner_run,
            member_run,
            outsider_run,
            holder_run,
            project_id,
            repository_id,
            workspace_id,
            session_id,
        }
    }

    /// The allocation request for a run's own checkout.
    fn allocation(&self, owner_run_id: &AgentRunId) -> CreateWorktreeRequest {
        CreateWorktreeRequest {
            project_id: self.project_id.clone(),
            repository_id: self.repository_id.clone(),
            workspace_id: self.workspace_id.clone(),
            node_id: None,
            repository_root: self.repository_root.path().to_path_buf(),
            base_commit: None,
            base_ref: None,
            target_branch: None,
            base_path: Some(self.repository_root.path().to_path_buf()),
            owner_run_id: owner_run_id.clone(),
        }
    }

    /// Place one run on the group's checkout, the way the executor does.
    async fn place(&self, run_id: &AgentRunId) -> SharedCheckoutOutcome {
        let outcome = self
            .coordinator
            .acquire(SharedCheckoutRequest {
                group_id: self.group_id.clone(),
                member_run_id: run_id.clone(),
                allocation: self.allocation(run_id),
            })
            .await
            .expect("the placement succeeds for a member of a live group");
        self.runs
            .attach_worktree(run_id, outcome.checkout().worktree_id.clone())
            .await
            .expect("attach the run to the group's checkout");
        outcome
    }

    /// Give the outsider a checkout of its own, so its fence resolves against
    /// a lease it does not hold rather than against no checkout at all.
    async fn place_outsider(&self) -> codegg_core::worktree_service::WorktreeRecord {
        let outcome = self
            .coordinator
            .acquire(SharedCheckoutRequest {
                group_id: self.group_id.clone(),
                member_run_id: self.outsider_run.clone(),
                allocation: self.allocation(&self.outsider_run),
            })
            .await;
        assert!(
            outcome.is_err(),
            "a run outside the group must not be placed on the group's checkout"
        );

        // Its own checkout, through the same real service path — and held by
        // a **different** run, so the outsider resolves against a lease it
        // does not hold rather than against no checkout at all.
        let (record, _lease) = self
            .worktrees
            .create(&self.allocation(&self.holder_run))
            .await
            .expect("allocate the outsider's own checkout");
        self.runs
            .attach_worktree(&self.outsider_run, record.worktree_id.clone())
            .await
            .expect("attach the outsider to its own checkout");
        record
    }

    /// The fence for one run, resolved the way the daemon resolves it.
    fn fence(
        &self,
        run_id: &AgentRunId,
        worktree_id: &WorktreeId,
        root: &Path,
    ) -> Arc<DaemonWriterFence> {
        Arc::new(
            DaemonWriterFence::new(
                codegg::tool::writer_fence::CheckoutWriterScope {
                    worktree_id: worktree_id.as_str().to_owned(),
                    checkout_root: root.to_path_buf(),
                    agent_run_id: Some(run_id.as_str().to_owned()),
                    group_id: Some(self.group_id.as_str().to_owned()),
                },
                self.permits.clone(),
                self.worktrees.clone(),
                self.runs.clone(),
                Some(self.groups.clone()),
                Some(run_id.clone()),
                Some("attempt-m011".to_owned()),
                None,
            )
            .with_principal_fallback(local_principal()),
        )
    }

    /// The agent loop for one of the two racing agents.
    ///
    /// Both loops carry the **same two named agents**, so both names resolve
    /// in both loops, and each loop selects the one it is running.
    fn agent_loop(
        &self,
        agent_name: &str,
        provider: ScriptedProvider,
        fence: Arc<DaemonWriterFence>,
        checkout_root: &Path,
    ) -> AgentLoop {
        let registry = ToolRegistry::with_options(ToolRegistryOptions {
            workspace_root: Some(checkout_root.to_path_buf()),
            ..Default::default()
        });
        let permission_checker =
            PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
                default: PermissionLevel::Allow,
                tool_rules: vec![],
                path_rules: vec![],
            });
        let mut agent_loop = AgentLoop::new(
            vec![named_agent("alpha"), named_agent("beta")],
            Box::new(provider),
            permission_checker,
            registry,
            Config::default(),
            None,
            None,
            Arc::new(InMemoryArtifactStore::new()),
            checkout_root.to_path_buf(),
            self.session_id.clone(),
            Some(fence as SharedWriterFence),
            None,
        );
        agent_loop
            .set_agent(agent_name)
            .expect("the named agent resolves");
        // A delegated child holds no workspace services lease of its own, so
        // it inherits the identity the scheduler bound the run to and installs
        // the policy for the root the executor bound it to. Without them the
        // broker refuses every tool call with "verified tool authority is
        // missing workspace ...". This is the production child-loop pattern.
        agent_loop.set_workspace_id(self.workspace_id.clone());
        agent_loop.set_workspace_path_policy(
            codegg_core::workspace_services::WorkspacePathPolicy::for_root(
                checkout_root.to_path_buf(),
            ),
        );
        agent_loop
    }
}

/// An agent with a name of its own.
fn named_agent(name: &str) -> Agent {
    Agent {
        git_read_only: false,
        name: name.to_string(),
        role: None,
        description: format!("the {name} agent"),
        mode: AgentMode::Primary,
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
    }
}

/// The request that starts one agent's turn.
fn chat_request(prompt: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![codegg::provider::Message::User {
            content: vec![codegg::provider::ContentPart::Text {
                text: prompt.to_string().into(),
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

/// What the agent saw from its tools, so a failure says what the tool said
/// rather than only that a file is missing.
fn tool_results(requests: &[ChatRequest]) -> Vec<String> {
    requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter_map(|message| match message {
            codegg::provider::Message::Tool {
                tool_call_id,
                content,
            } => Some(format!("{tool_call_id}: {content}")),
            _ => None,
        })
        .collect()
}

/// One scripted turn: a write into the shared checkout, then a stop.
fn write_turn(call_id: &str, path: &str, content: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::ToolCall(ToolCall {
            id: call_id.to_string().into(),
            name: "write".to_string().into(),
            arguments: serde_json::json!({
                "path": path,
                "content": content,
            }),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ]
}

// ---------------------------------------------------------------------------
// The composed race
// ---------------------------------------------------------------------------

/// Two live agents, one shared checkout, one write each — and the two writes
/// never overlap.
///
/// The race is real: both agents' first model request is held at a barrier
/// until both have arrived, so neither agent can finish its turn before the
/// other has started one. The suite then checks that the two tool-call
/// windows overlapped, so a pass cannot come from the agents having run one
/// after the other.
///
/// The serialization is the daemon's own: the production `write` tool asks the
/// production `DaemonWriterFence` for a permit, and the permit store admits
/// one writer at a time. The timeline the fence reports is the evidence.
#[tokio::test(flavor = "current_thread")]
async fn two_live_agents_racing_on_one_shared_checkout_never_overlap() {
    let race = Race::new("race").await;
    let placed = race.place(&race.owner_run).await;
    race.place(&race.member_run).await;
    let checkout = placed.checkout().clone();

    drain_timeline();

    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let alpha_provider =
        ScriptedProvider::new(vec![write_turn("m011-alpha", "alpha.txt", "alpha\n")])
            .with_barrier(Arc::clone(&barrier));
    let beta_provider = ScriptedProvider::new(vec![write_turn("m011-beta", "beta.txt", "beta\n")])
        .with_barrier(Arc::clone(&barrier));

    let alpha_fence = race.fence(&race.owner_run, &checkout.worktree_id, &checkout.path);
    let beta_fence = race.fence(&race.member_run, &checkout.worktree_id, &checkout.path);
    let mut alpha = race.agent_loop("alpha", alpha_provider.clone(), alpha_fence, &checkout.path);
    let mut beta = race.agent_loop("beta", beta_provider.clone(), beta_fence, &checkout.path);

    let (alpha_outcome, beta_outcome) = tokio::join!(
        alpha.run(chat_request("write your file")),
        beta.run(chat_request("write your file"))
    );
    alpha_outcome.expect("the alpha agent's turn completes");
    beta_outcome.expect("the beta agent's turn completes");

    // Both agents really ran, and both really issued their operation.
    let alpha_requests = alpha_provider.request_instants().await;
    let beta_requests = beta_provider.request_instants().await;
    assert!(
        alpha_requests.len() >= 2,
        "the alpha agent must have asked the model again after its write: {alpha_requests:?}"
    );
    assert!(
        beta_requests.len() >= 2,
        "the beta agent must have asked the model again after its write: {beta_requests:?}"
    );

    // The race was fair: both agents were inside their turn at the same time.
    // The first request opens the tool call and the second closes it, so the
    // two windows overlapping means the two operations were in flight
    // together — a pass cannot be an artifact of a sequential run.
    let firsts = [alpha_requests[0], beta_requests[0]];
    let seconds = [alpha_requests[1], beta_requests[1]];
    let latest_first = firsts.iter().max().expect("two first requests");
    let earliest_second = seconds.iter().min().expect("two second requests");
    assert!(
        latest_first < earliest_second,
        "the two agents were not live at the same time: alpha {alpha_requests:?}, beta {beta_requests:?}"
    );

    // Both writes landed. A refusal would have produced no file, and the
    // permit timeline would have no second acquire.
    let alpha_results = tool_results(&alpha_provider.get_requests().await);
    let beta_results = tool_results(&beta_provider.get_requests().await);
    assert_eq!(
        std::fs::read_to_string(checkout.path.join("alpha.txt"))
            .unwrap_or_else(|error| panic!("the alpha write landed: {error}; {alpha_results:?}")),
        "alpha\n"
    );
    assert_eq!(
        std::fs::read_to_string(checkout.path.join("beta.txt"))
            .unwrap_or_else(|error| panic!("the beta write landed: {error}; {beta_results:?}")),
        "beta\n"
    );

    // The permit timeline: one permit was released before the other was
    // taken, so the two operations never overlapped.
    let events = timeline_for(&checkout.worktree_id);
    assert_eq!(
        events.len(),
        4,
        "each writer must take and release exactly one permit: {events:?}"
    );
    assert_eq!(events[0].phase, PermitPhase::Acquired, "{events:?}");
    assert_eq!(events[1].phase, PermitPhase::Released, "{events:?}");
    assert_eq!(
        events[0].agent_run_id, events[1].agent_run_id,
        "a permit must be released by the run that took it: {events:?}"
    );
    assert_eq!(events[2].phase, PermitPhase::Acquired, "{events:?}");
    assert_eq!(events[3].phase, PermitPhase::Released, "{events:?}");
    assert_eq!(
        events[2].agent_run_id, events[3].agent_run_id,
        "a permit must be released by the run that took it: {events:?}"
    );
    assert_ne!(
        events[0].agent_run_id, events[2].agent_run_id,
        "the two writers must be two different runs: {events:?}"
    );
    assert!(
        events[1].at < events[2].at,
        "one permit must be released before the other is taken: {events:?}"
    );

    // The permit store agrees: the checkout is free again, and it went
    // through four state changes — held, releasing, available, and again.
    let row: (String, Option<String>, i64) = sqlx::query_as(
        "SELECT state, owner_agent_run_id, revision FROM writer_permit WHERE worktree_id = ?",
    )
    .bind(checkout.worktree_id.as_str())
    .fetch_one(&race.pool)
    .await
    .expect("the permit row");
    assert_eq!(row.0, "available", "the checkout's permit must be free");
    assert!(
        row.1.is_none(),
        "the checkout's permit must have no owner once both writers are done"
    );
    assert!(
        row.2 >= 6,
        "both writers must have run the permit through its whole state machine: {row:?}"
    );
}

/// The outsider holds a checkout of its own, so its fence resolves against a
/// lease it does not hold. The scope it was handed names the shared checkout,
/// and the run's own durable record wins over that scope — a caller cannot
/// borrow another checkout's authority by naming it.
#[tokio::test(flavor = "current_thread")]
async fn a_run_outside_the_group_may_not_write_the_shared_checkout() {
    let race = Race::new("outsider").await;
    let placed = race.place(&race.owner_run).await;
    let checkout = placed.checkout().clone();
    race.place_outsider().await;

    drain_timeline();

    // The outsider's agent is rooted at the **shared** checkout, so its write
    // lands inside the shared checkout's grant — and is still refused.
    let outsider_fence = race.fence(&race.outsider_run, &checkout.worktree_id, &checkout.path);
    let provider = ScriptedProvider::new(vec![write_turn(
        "m011-outsider",
        "outsider.txt",
        "outsider\n",
    )]);
    let mut outsider = race.agent_loop("beta", provider, outsider_fence, &checkout.path);

    let outcome = outsider
        .run(chat_request("write your file"))
        .await
        .expect("the outsider's turn completes");
    assert!(
        !outcome.is_empty(),
        "the outsider's turn must have produced events"
    );

    assert!(
        !checkout.path.join("outsider.txt").exists(),
        "a run outside the group must not write the shared checkout"
    );
    let events = timeline_for(&checkout.worktree_id);
    assert!(
        events.is_empty(),
        "the outsider must not have taken the shared checkout's permit: {events:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_shared_checkout_is_a_real_linked_worktree() {
    let race = Race::new("linked").await;
    let placed = race.place(&race.owner_run).await;
    let checkout = placed.checkout().clone();
    let record = race
        .worktrees
        .get(&checkout.worktree_id)
        .await
        .expect("the shared checkout's record");
    let branch = record.branch.clone().expect("the branch was recorded");

    // Git reports the checkout on the branch the service recorded.
    let listing = common::blackboard::git_stdout(
        &["worktree", "list", "--porcelain"],
        race.repository_root.path(),
    );
    assert!(
        listing.contains(&checkout.path.to_string_lossy().to_string()),
        "git must report the shared checkout: {listing}"
    );
    assert!(
        listing.contains(&format!("branch refs/heads/{branch}")),
        "git must report the shared checkout on its recorded branch: {listing}"
    );

    // And the checkout is a linked worktree, not a repository of its own.
    assert!(
        checkout.path.join(".git").is_file(),
        "a linked worktree's `.git` is a pointer file, not a directory"
    );
}
