//! C001 — native Linux scratch containment qualification.
//!
//! Scope: `plans/implementation/managed-agent-scratch-native-qualification-corrective/001-linux-native-scratch-containment-qualification.md`.
//!
//! This file is the **native Linux** half of the managed-scratch qualification
//! gap left open by M003. It is a whole-file `cfg(target_os = "linux")`
//! fixture on purpose: a qualification that cannot run on the target platform
//! must not exist as a *passing* test on another platform, so on macOS and
//! Windows this binary contains no tests at all.
//!
//! Two halves, joined the way production joins them:
//!
//! 1. **Admission** — a real `TaskTool` spawn through the real
//!    `JobSubmissionService` / `JobScheduler` freezes the shared-grant
//!    snapshot into the durable `AgentRunRecord`. The effective scope is then
//!    rebuilt from that record exactly as `src/agent/worker.rs` rebuilds it.
//! 2. **Containment** — the rebuilt scope travels on the real
//!    `ExecutionAuthorizationContext` into the real `BashTool` and the real
//!    `ManagedProcessService`, which launches the real `codegg-sandbox-helper`.
//!    The assertions are made against the *observed* behaviour of the child
//!    and the *obtained* backend status, never against a launch spec the test
//!    wrote by hand for the scratch roots.
//!
//! A host that cannot obtain Landlock ABI >= 3 **fails** this fixture instead
//! of skipping it. A skipped qualification is indistinguishable from a passed
//! one in a test report, and the plan forbids counting "unavailable" as
//! "qualified". The same is true off Linux: the fixture does not exist there
//! at all, so it can never be counted as satisfied.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessError, ManagedProcessRequest, ManagedProcessService,
    ProcessProvenance, SandboxExecutionOutcome, SandboxRequest,
};
use codegg::permission::PermissionDecisionReceipt;
use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::scratch::{
    agent_run_scratch_dir, session_scratch_dir, shared_scratch_dir, AgentScratchScope,
    AuthorizedSharedScratchRoot,
};
use codegg::security::sandbox::{
    platform_sandbox_capability, BackendId, SandboxCapability, SandboxConfig, SandboxMode,
};
use codegg::tool::authorization::ExecutionAuthorizationContext;
use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::bash::BashTool;
use codegg::tool::task::{TaskStore, TaskTool};
use codegg::tool::Tool;
use codegg_core::agent_run::{AgentRunRecord, AgentRunStore, InMemoryAgentRunStore};
use codegg_core::approval::{ApprovalMode, ExecutionPolicySnapshot, SandboxProfile};
use codegg_core::identity::ScratchId;
use codegg_core::jobs::{AttemptId, DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

/// The Landlock ABI this qualification requires.
///
/// ABI 3 is the first version that can express the "deny below a writable
/// root" carve-out the product relies on for scratch isolation. A host that
/// only offers ABI 1/2 cannot produce this evidence at all.
const REQUIRED_LANDLOCK_ABI: u32 = 3;

/// Serializes the process-global `XDG_CACHE_HOME` move.
///
/// `nextest` runs one process per test, but plain `cargo test` runs tests as
/// threads inside one process, where environment mutation is process-global.
/// The lock keeps the scratch home stable for the duration of each test.
fn scratch_home_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A scratch root that is private to this test process.
///
/// The managed scratch allocator derives every session root from
/// `dirs::cache_dir()`, so the qualification redirects `XDG_CACHE_HOME` at a
/// private, canonical, 0700 directory instead of writing into the developer's
/// real cache.
struct ScratchHome {
    _dir: tempfile::TempDir,
    previous: Option<std::ffi::OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl ScratchHome {
    fn install() -> Self {
        let guard = scratch_home_lock();
        let dir = tempfile::tempdir().expect("scratch home");
        // `tempfile` creates its directories with the *default* permissions, so
        // the mode depends on the operator's umask (0755 under umask 022, 0775
        // under umask 002). The allocator refuses a cache root another user can
        // write to, so the fixture has to be private on every host rather than
        // only on the hosts whose umask happens to produce a private mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private scratch home");
        }
        let canonical = std::fs::canonicalize(dir.path()).expect("canonical scratch home");
        let previous = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", &canonical);
        Self {
            _dir: dir,
            previous,
            _guard: guard,
        }
    }
}

impl Drop for ScratchHome {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var("XDG_CACHE_HOME", previous),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
    }
}

/// Fail loudly when this host cannot host the qualification.
///
/// The qualification is only meaningful where the product can actually
/// enforce containment, so a host that cannot obtain Landlock is a *failed*
/// qualification rather than a skipped one. Skipping would let an
/// unavailable qualification be reported as a satisfied one.
fn require_native_linux_landlock() -> SandboxCapability {
    let capability = platform_sandbox_capability();
    let SandboxCapability::Available { backend } = capability else {
        panic!(
            "native Linux scratch containment qualification requires an OS containment \
             backend, but this host has none: {}. Required containment cannot be qualified here.",
            capability.reason().unwrap_or("unknown reason")
        );
    };
    assert_eq!(
        backend,
        BackendId::LANDLOCK,
        "the native Linux qualification must be produced by Landlock, not another backend"
    );
    capability
}

/// One admitted delegated run plus every scratch root the qualification needs
/// to tell "granted" apart from "not granted".
struct Admitted {
    session_id: String,
    record: AgentRunRecord,
    scope: AgentScratchScope,
    /// The delegated run's own private root.
    private_root: PathBuf,
    /// The explicitly granted shared scratch root and its typed identity.
    granted_root: PathBuf,
    granted_id: ScratchId,
    /// A shared scratch root in the same session that the run was **not**
    /// granted.
    ungranted_root: PathBuf,
    ungranted_id: ScratchId,
    /// Another session's run-private root.
    foreign_private_root: PathBuf,
    /// The directory that contains every session root.
    scratch_parent: PathBuf,
    /// The durable store the admitted run lives in.
    runs: Arc<dyn AgentRunStore>,
    /// The task tool that owns the parent's scratch resources.
    task: TaskTool,
}

/// Admit a delegated run through the real task tool, submission service, and
/// scheduler, then rebuild the effective scope from the durable record.
async fn admit_delegated_run(session_id: &str, workspace: &Path) -> Admitted {
    let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    registry
        .get_or_register(workspace)
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );

    let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let generation = DaemonGeneration::new_unchecked("native-scratch-qualification");
    let scheduler = JobScheduler::new(
        job_store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        generation.clone(),
    );
    let submission = JobSubmissionService::new(job_store.clone(), scheduler, services, generation);
    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
    let task = TaskTool::new(
        Arc::new(AsyncMutex::new(TaskStore::new())),
        None,
        Some(session_id.to_string()),
        vec![],
    )
    .with_submission(submission, workspace.to_path_buf())
    .with_agent_run_store(runs.clone())
    .with_turn_owner(session_id.to_string(), "turn-native".to_string())
    .with_project_context(None, None, Some("turn-native".to_string()));

    // The parent run owns two shared scratch resources. Only one of them is
    // handed to the delegated run; the other stays ungranted so the
    // qualification can prove that an ungranted sibling is not writable.
    let granted = create_scratch(&task, session_id, "handoff", "scratch-create-granted").await;
    let ungranted =
        create_scratch(&task, session_id, "ungranted", "scratch-create-ungranted").await;

    task.execute_structured(
        serde_json::json!({
            "action": "spawn",
            "description": "consume the explicitly granted scratch",
            "prompt": "Read the explicitly granted artifact",
            "agent": "general",
            "idempotency_key": "spawn-native-qualification",
            "shared_scratch": [{ "id": granted.to_string(), "alias": "handoff" }]
        }),
        Some(task_context(session_id, "task-spawn")),
    )
    .await
    .expect("spawn submits a durable subagent job");

    let record = runs
        .list_by_session(session_id)
        .await
        .expect("list accepted child run")
        .into_iter()
        .next()
        .expect("child run persisted");

    // Rebuild the effective scope from the durable record, exactly the way
    // the delegated worker does before it starts a turn. The scope must come
    // from the frozen grant snapshot, never from the caller's request.
    let scope = AgentScratchScope::from_authorized_grants(
        session_id.to_string(),
        record.run_id.clone(),
        &record.shared_scratch,
    )
    .expect("effective delegated scope");

    let foreign_session = format!("{session_id}-foreign");
    let foreign_run = codegg_core::identity::AgentRunId::new();
    let foreign_private_root =
        agent_run_scratch_dir(&foreign_session, &foreign_run).expect("foreign run private scratch");
    let session_root = session_scratch_dir(session_id).expect("session scratch root");
    let scratch_parent = session_root.parent().expect("scratch parent").to_path_buf();

    Admitted {
        session_id: session_id.to_string(),
        private_root: scope.private_root.clone(),
        granted_root: scope
            .shared
            .first()
            .expect("one granted shared root")
            .path
            .clone(),
        granted_id: scope.shared[0].scratch_id.clone(),
        ungranted_root: shared_scratch_dir(session_id, &ungranted)
            .expect("ungranted shared scratch"),
        ungranted_id: ungranted,
        foreign_private_root,
        scratch_parent,
        record,
        scope,
        runs,
        task,
    }
}

async fn create_scratch(
    task: &TaskTool,
    session_id: &str,
    alias: &str,
    invocation_key: &str,
) -> ScratchId {
    let created = task
        .execute_structured(
            serde_json::json!({
                "action": "scratch_create",
                "scratch_alias": alias
            }),
            Some(task_context(session_id, invocation_key)),
        )
        .await
        .expect("create parent-owned scratch");
    created
        .output
        .lines()
        .find_map(|line| line.strip_prefix("ScratchId: "))
        .and_then(|id| ScratchId::parse(id).ok())
        .expect("opaque scratch id returned")
}

fn task_context(session_id: &str, invocation_key: &str) -> ToolExecutionContext {
    let mut context = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    context.session_id = Some(session_id.to_string());
    context.turn_id = Some("turn-native".to_string());
    context.invocation_key = Some(invocation_key.to_string());
    context
}

/// The accepted authorization for one delegated launch.
///
/// The snapshot is the product's own capture shape, and the scratch scope is
/// the scope rebuilt from the durable grant snapshot.
fn authorization(scope: AgentScratchScope) -> ExecutionAuthorizationContext {
    ExecutionAuthorizationContext {
        snapshot: ExecutionPolicySnapshot::capture(
            ApprovalMode::Interactive,
            SandboxProfile::WorkspaceWrite,
            Some("principal-native-qualification".to_string()),
            Some(scope.session_id.clone()),
            None,
            None,
            None,
        ),
        receipt: PermissionDecisionReceipt::allowed("native_linux_qualification", None),
        command_preflight: None,
        scratch_scope: Some(scope),
    }
}

/// The observation script the contained child runs.
///
/// It never decides anything itself: every line performs one real filesystem
/// operation and reports whether the kernel allowed it. The paths arrive as
/// positional arguments so the script needs no quoting rules and no ambient
/// environment. Every write attempt runs in a subshell so a refused
/// redirection cannot terminate the script.
const OBSERVATION_SCRIPT: &str = r#"#!/bin/sh
private="$1"
granted="$2"
sibling="$3"
foreign="$4"
parent="$5"
hosttmp="$6"
credential="$7"
workspace="$8"

probe() {
    # probe <name> <path-to-write>
    if ( printf 'x' > "$2" ) 2>/dev/null; then
        printf '%s=allowed\n' "$1"
    else
        printf '%s=denied\n' "$1"
    fi
}

probe PRIVATE_WRITE "$private/private.txt"
probe GRANTED_SHARED_WRITE "$granted/granted.txt"
probe UNGRANTED_SIBLING_WRITE "$sibling/sibling.txt"
probe FOREIGN_SESSION_WRITE "$foreign/foreign.txt"
probe SCRATCH_PARENT_WRITE "$parent/parent.txt"
probe HOST_TEMP_WRITE "$hosttmp/host.txt"
probe CREDENTIAL_WRITE "$credential"

# Truncating a pre-existing file outside the authorized write roots is a
# write, not a create, and must be denied for the same reason.
if ( printf 'overwritten' > "$hosttmp/truncate.txt" ) 2>/dev/null; then
    printf 'HOST_TEMP_TRUNCATE=allowed\n'
else
    printf 'HOST_TEMP_TRUNCATE=denied\n'
fi

# Hard-linking a workspace file out of the workspace must be denied: the
# workspace write root does not extend to a link target elsewhere.
( printf 'seed' > "$workspace/link-source" ) 2>/dev/null
if ( ln "$workspace/link-source" "$hosttmp/link-target" ) 2>/dev/null; then
    printf 'HARDLINK_OUT=allowed\n'
else
    printf 'HARDLINK_OUT=denied\n'
fi

# Renaming a workspace file out of the workspace must be denied for the same
# reason.
( printf 'seed' > "$workspace/rename-source" ) 2>/dev/null
if ( mv "$workspace/rename-source" "$hosttmp/renamed" ) 2>/dev/null; then
    printf 'RENAME_OUT=allowed\n'
else
    printf 'RENAME_OUT=denied\n'
fi

# A symlink planted inside the workspace must not become a way out of it: the
# kernel resolves the link and denies the write to its real target.
( ln -s "$hosttmp" "$workspace/escape-link" ) 2>/dev/null
if ( printf 'x' > "$workspace/escape-link/escape.txt" ) 2>/dev/null; then
    printf 'SYMLINK_ESCAPE=allowed\n'
else
    printf 'SYMLINK_ESCAPE=denied\n'
fi

# The delegated run's own private temporary variables must point at its
# private root, and the granted alias must point at the granted root.
if [ "$CODEGG_SCRATCH_DIR" = "$private" ] \
    && [ "$TMPDIR" = "$private" ] \
    && [ "$TMP" = "$private" ] \
    && [ "$TEMP" = "$private" ]; then
    printf 'PRIVATE_TEMP_VARS=correct\n'
else
    printf 'PRIVATE_TEMP_VARS=wrong\n'
fi
if [ "$CODEGG_SHARED_SCRATCH_HANDOFF" = "$granted" ]; then
    printf 'SHARED_ALIAS_VAR=correct\n'
else
    printf 'SHARED_ALIAS_VAR=wrong\n'
fi
"#;

/// Parse the observation script's `NAME=allowed|denied` lines.
fn parse_observations(stdout: &str) -> HashMap<String, String> {
    stdout
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .collect()
}

fn assert_denied(observations: &HashMap<String, String>, name: &str) {
    assert_eq!(
        observations.get(name).map(String::as_str),
        Some("denied"),
        "{name} must be denied by the kernel; observations were {observations:?}"
    );
}

fn assert_allowed(observations: &HashMap<String, String>, name: &str) {
    assert_eq!(
        observations.get(name).map(String::as_str),
        Some("allowed"),
        "{name} must be allowed by the kernel; observations were {observations:?}"
    );
}

/// Assert a probe that reports `correct`/`wrong` rather than
/// `allowed`/`denied`.
fn assert_correct(observations: &HashMap<String, String>, name: &str) {
    assert_eq!(
        observations.get(name).map(String::as_str),
        Some("correct"),
        "{name} must point at the authorized root; observations were {observations:?}"
    );
}

/// A workspace-rooted sandbox configuration for a `WorkspaceWrite` launch.
fn workspace_write_config(workspace: &Path) -> SandboxConfig {
    SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![workspace.to_string_lossy().into_owned()])
}

/// Build the observation script inside the workspace and return its path.
///
/// The script is made executable: the managed-process case hands it to `sh`
/// as an argument, but the model-owned `BashTool` case runs it as a program,
/// and a non-executable file would fail to start and yield no observations.
fn write_observation_script(workspace: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let script = workspace.join("observe-scratch.sh");
    std::fs::write(&script, OBSERVATION_SCRIPT).expect("write observation script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("make the observation script executable");
    script
}

/// The paths the observation script needs, in the order it expects them.
fn observation_arguments(
    admitted: &Admitted,
    host_temp: &Path,
    credential: &Path,
    workspace: &Path,
) -> Vec<String> {
    vec![
        admitted.private_root.to_string_lossy().into_owned(),
        admitted.granted_root.to_string_lossy().into_owned(),
        admitted.ungranted_root.to_string_lossy().into_owned(),
        admitted.foreign_private_root.to_string_lossy().into_owned(),
        admitted.scratch_parent.to_string_lossy().into_owned(),
        host_temp.to_string_lossy().into_owned(),
        credential.to_string_lossy().into_owned(),
        workspace.to_string_lossy().into_owned(),
    ]
}

/// Quote a path for a POSIX shell word.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// C001 WP-A — the host must be able to enforce Landlock at all, and the
/// qualification refuses to run anywhere else.
#[test]
fn native_linux_host_can_enforce_landlock() {
    let capability = require_native_linux_landlock();
    assert_eq!(capability.backend_id(), BackendId::LANDLOCK);
}

/// C001 WP-B — the admitted delegated run freezes exactly the granted scratch
/// into its durable record, and the effective scope is rebuilt from that
/// record.
#[tokio::test(flavor = "current_thread")]
async fn admitted_run_freezes_only_the_granted_scratch() {
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let session_id = "session-native-grant-freeze";
    let admitted = admit_delegated_run(session_id, workspace.path()).await;

    assert_eq!(
        admitted.record.shared_scratch.len(),
        1,
        "only the explicitly granted scratch may reach the delegated run"
    );
    assert_eq!(admitted.record.shared_scratch[0].alias, "handoff");
    assert_eq!(admitted.scope.shared.len(), 1);
    assert_eq!(admitted.scope.shared[0].alias, "handoff");
    assert_eq!(
        admitted.scope.shared[0].path, admitted.granted_root,
        "the effective scope must resolve the granted root from the session identity"
    );
    assert_eq!(admitted.scope.private_root, admitted.private_root);
    assert!(
        admitted
            .scope
            .private_root
            .starts_with(&admitted.scratch_parent),
        "the private root must live under the session scratch parent"
    );
    assert_ne!(
        admitted.scope.private_root, admitted.ungranted_root,
        "the ungranted sibling must not be confused with the private root"
    );

    // The accepted snapshot is immutable: a retry of the same accepted
    // invocation keeps the same run and the same frozen grant set, and it
    // does not acquire a second shared root.
    admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "spawn",
                "description": "consume the explicitly granted scratch",
                "prompt": "Read the explicitly granted artifact",
                "agent": "general",
                "idempotency_key": "spawn-native-qualification",
                "shared_scratch": [{ "id": admitted.granted_id.to_string(), "alias": "handoff" }]
            }),
            Some(task_context(session_id, "task-spawn")),
        )
        .await
        .expect("retrying an accepted spawn is idempotent");
    let runs = admitted
        .runs
        .list_by_session(session_id)
        .await
        .expect("list accepted child runs");
    assert_eq!(
        runs.len(),
        1,
        "a retry must not create a second delegated run"
    );
    assert_eq!(runs[0].run_id, admitted.record.run_id);
    assert_eq!(runs[0].shared_scratch, admitted.record.shared_scratch);

    // Widening the grant set on the same accepted invocation is refused: the
    // frozen snapshot cannot be widened after acceptance.
    let error = admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "spawn",
                "description": "consume the explicitly granted scratch",
                "prompt": "Read the explicitly granted artifact",
                "agent": "general",
                "idempotency_key": "spawn-native-qualification",
                "shared_scratch": [
                    { "id": admitted.granted_id.to_string(), "alias": "handoff" },
                    { "id": admitted.ungranted_id.to_string(), "alias": "ungranted" }
                ]
            }),
            Some(task_context(session_id, "task-spawn")),
        )
        .await
        .expect_err("an accepted run may not widen its frozen grant set");
    assert!(
        error.to_string().contains("delegation key already belongs"),
        "unexpected widening refusal: {error}"
    );
    let runs = admitted
        .runs
        .list_by_session(session_id)
        .await
        .expect("list accepted child runs");
    assert_eq!(
        runs.len(),
        1,
        "a refused widening must not create a second delegated run"
    );
    assert_eq!(runs[0].shared_scratch, admitted.record.shared_scratch);
}

/// C001 WP-C — a grant that the caller does not own, or that names an unknown
/// resource, must be refused at admission.
#[tokio::test(flavor = "current_thread")]
async fn an_unowned_or_unknown_scratch_grant_is_refused_at_admission() {
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let session_id = "session-native-foreign-grant";
    let admitted = admit_delegated_run(session_id, workspace.path()).await;

    // An unknown resource: the typed id names a scratch this session never
    // created.
    let unknown = ScratchId::new();
    let error = admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "spawn",
                "description": "claim an unknown scratch",
                "prompt": "Read the unknown resource",
                "agent": "general",
                "idempotency_key": "spawn-native-unknown-grant",
                "shared_scratch": [{ "id": unknown.to_string(), "alias": "unknown" }]
            }),
            Some(task_context(session_id, "task-spawn-unknown")),
        )
        .await
        .expect_err("an unknown scratch grant must be refused");
    let message = error.to_string();
    assert!(
        message.contains("unavailable") || message.contains("ownership"),
        "unexpected refusal message: {message}"
    );

    // A path-like grant: the typed id is identity only, so a caller cannot
    // smuggle a filesystem path in as a scratch locator.
    let error = admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "spawn",
                "description": "claim a path as scratch",
                "prompt": "Read the path",
                "agent": "general",
                "idempotency_key": "spawn-native-path-grant",
                "shared_scratch": [{ "id": "/tmp/escape-scratch", "alias": "escape" }]
            }),
            Some(task_context(session_id, "task-spawn-path")),
        )
        .await
        .expect_err("a path-like scratch id must be refused");
    let message = error.to_string();
    assert!(
        message.contains("scratch_id"),
        "unexpected refusal message: {message}"
    );

    // A resource this session does not own: another session's scratch.
    let other_session = "session-native-foreign-owner";
    let foreign = {
        let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        registry
            .get_or_register(workspace.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let generation = DaemonGeneration::new_unchecked("native-scratch-qualification");
        let scheduler = JobScheduler::new(
            job_store.clone(),
            services.clone(),
            ResolvedSchedulerConfig::default(),
            generation.clone(),
        );
        let submission =
            JobSubmissionService::new(job_store.clone(), scheduler, services, generation);
        let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
        let task = TaskTool::new(
            Arc::new(AsyncMutex::new(TaskStore::new())),
            None,
            Some(other_session.to_string()),
            vec![],
        )
        .with_submission(submission, workspace.path().to_path_buf())
        .with_agent_run_store(runs.clone())
        .with_turn_owner(other_session.to_string(), "turn-native".to_string())
        .with_project_context(None, None, Some("turn-native".to_string()));
        create_scratch(&task, other_session, "foreign", "scratch-create-foreign").await
    };

    let error = admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "spawn",
                "description": "claim another session's scratch",
                "prompt": "Read the foreign resource",
                "agent": "general",
                "idempotency_key": "spawn-native-foreign-owner-grant",
                "shared_scratch": [{ "id": foreign.to_string(), "alias": "foreign" }]
            }),
            Some(task_context(session_id, "task-spawn-foreign")),
        )
        .await
        .expect_err("a scratch owned by another session must be refused");
    let message = error.to_string();
    assert!(
        message.contains("unavailable") || message.contains("ownership"),
        "unexpected refusal message: {message}"
    );
}

/// C001 WP-B/WP-C — the core native evidence.
///
/// The launch spec the test hands to the managed process names **only the
/// workspace**. Every scratch write root is added by the production owner
/// from the authorization scope, so a child that can write its private root
/// and its granted shared root proves the owner composed the scope, and a
/// child that cannot write an ungranted sibling proves the owner did not
/// over-grant.
#[tokio::test(flavor = "current_thread")]
async fn required_launch_confines_scratch_to_the_granted_roots() {
    let capability = require_native_linux_landlock();
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let host_temp = tempfile::tempdir().expect("host temp");
    let credential_dir = tempfile::tempdir().expect("credential dir");
    let credential = credential_dir.path().join("credential");
    std::fs::write(&credential, "secret").expect("credential fixture");
    // A pre-existing file makes the truncation probe a truncation rather than
    // a create.
    std::fs::write(host_temp.path().join("truncate.txt"), "seed").expect("truncate fixture");

    let session_id = "session-native-containment";
    let admitted = admit_delegated_run(session_id, workspace.path()).await;

    let script = write_observation_script(workspace.path());
    let mut spec_args = vec![script.to_string_lossy().into_owned()];
    spec_args.extend(observation_arguments(
        &admitted,
        host_temp.path(),
        &credential,
        workspace.path(),
    ));

    let config = workspace_write_config(workspace.path());
    let spec = config
        .launch_spec("sh", &spec_args, Some(workspace.path()))
        .expect("build the launch spec");

    let mut argv: Vec<OsString> = vec![OsString::from("sh")];
    argv.extend(spec_args.iter().map(OsString::from));

    let mut request = ManagedProcessRequest::new(
        argv,
        workspace.path().to_path_buf(),
        ProcessProvenance::new("job-native", "attempt-native"),
    );
    request.sandbox = SandboxRequest::Required(spec);
    request.environment_policy = EnvironmentPolicy::sanitized();
    request.execution_authorization = Some(authorization(admitted.scope.clone()));

    let result = ManagedProcessService::run(request)
        .await
        .expect("the contained launch must succeed");

    // The obtained status, not the requested one.
    let SandboxExecutionOutcome::Enforced {
        backend,
        abi,
        guarantees,
        ..
    } = &result.sandbox
    else {
        panic!(
            "the launch must report obtained containment, got {:?}",
            result.sandbox
        );
    };
    assert_eq!(*backend, BackendId::LANDLOCK);
    let abi = abi.expect("Landlock reports an ABI");
    assert!(
        abi >= REQUIRED_LANDLOCK_ABI,
        "Landlock ABI {abi} is below the required ABI {REQUIRED_LANDLOCK_ABI}"
    );
    assert!(
        guarantees
            .iter()
            .any(|guarantee| guarantee.contains("no_new_privs")),
        "the obtained guarantees must include no_new_privs, got {guarantees:?}"
    );
    assert!(
        capability.is_available(),
        "the host must be able to enforce containment"
    );

    let raw = result.stdout.to_string_lossy();
    assert!(
        raw.contains("PRIVATE_WRITE="),
        "the contained child did not report its observations: {raw:?}"
    );
    let observations = parse_observations(&raw);
    assert_allowed(&observations, "PRIVATE_WRITE");
    assert_allowed(&observations, "GRANTED_SHARED_WRITE");
    assert_correct(&observations, "PRIVATE_TEMP_VARS");
    assert_correct(&observations, "SHARED_ALIAS_VAR");

    assert_denied(&observations, "UNGRANTED_SIBLING_WRITE");
    assert_denied(&observations, "FOREIGN_SESSION_WRITE");
    assert_denied(&observations, "SCRATCH_PARENT_WRITE");
    assert_denied(&observations, "HOST_TEMP_WRITE");
    assert_denied(&observations, "CREDENTIAL_WRITE");
    assert_denied(&observations, "HOST_TEMP_TRUNCATE");
    assert_denied(&observations, "HARDLINK_OUT");
    assert_denied(&observations, "RENAME_OUT");
    assert_denied(&observations, "SYMLINK_ESCAPE");

    // The host-side view agrees with the child's view.
    assert!(admitted.private_root.join("private.txt").exists());
    assert!(admitted.granted_root.join("granted.txt").exists());
    assert!(!admitted.ungranted_root.join("sibling.txt").exists());
    assert!(!admitted.foreign_private_root.join("foreign.txt").exists());
    assert!(!host_temp.path().join("host.txt").exists());
}

/// C001 WP-C — the same containment, observed at the model-owned tool
/// boundary through the real `BashTool`.
///
/// This is the path a model's shell call actually takes, so it proves the
/// scratch scope is honoured by the tool owner and not only by the managed
/// process owner.
#[tokio::test(flavor = "current_thread")]
async fn model_owned_bash_tool_confines_scratch_to_the_granted_roots() {
    require_native_linux_landlock();
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let host_temp = tempfile::tempdir().expect("host temp");
    let credential_dir = tempfile::tempdir().expect("credential dir");
    let credential = credential_dir.path().join("credential");
    std::fs::write(&credential, "secret").expect("credential fixture");
    std::fs::write(host_temp.path().join("truncate.txt"), "seed").expect("truncate fixture");

    let session_id = "session-native-tool-boundary";
    let admitted = admit_delegated_run(session_id, workspace.path()).await;

    let script = write_observation_script(workspace.path());
    let mut command = shell_quote(&script.to_string_lossy());
    for argument in
        observation_arguments(&admitted, host_temp.path(), &credential, workspace.path())
    {
        command.push(' ');
        command.push_str(&shell_quote(&argument));
    }

    let bash = BashTool::new()
        .with_workspace_root(workspace.path().to_path_buf())
        .with_sandbox_profile(SandboxProfile::WorkspaceWrite, workspace.path());

    let mut context = task_context(&admitted.session_id, "bash-native-qualification");
    context.execution_authorization = Some(authorization(admitted.scope.clone()));

    let output = bash
        .execute_structured(serde_json::json!({ "command": command }), Some(context))
        .await
        .expect("the contained shell call must succeed");

    assert!(
        output.output.contains("PRIVATE_WRITE="),
        "the contained shell call did not report its observations: {:?}",
        output.output
    );
    let observations = parse_observations(&output.output);
    assert_allowed(&observations, "PRIVATE_WRITE");
    assert_allowed(&observations, "GRANTED_SHARED_WRITE");
    assert_correct(&observations, "PRIVATE_TEMP_VARS");
    assert_correct(&observations, "SHARED_ALIAS_VAR");

    assert_denied(&observations, "UNGRANTED_SIBLING_WRITE");
    assert_denied(&observations, "FOREIGN_SESSION_WRITE");
    assert_denied(&observations, "SCRATCH_PARENT_WRITE");
    assert_denied(&observations, "HOST_TEMP_WRITE");
    assert_denied(&observations, "CREDENTIAL_WRITE");
    assert_denied(&observations, "HOST_TEMP_TRUNCATE");
    assert_denied(&observations, "HARDLINK_OUT");
    assert_denied(&observations, "RENAME_OUT");
    assert_denied(&observations, "SYMLINK_ESCAPE");

    assert!(!admitted.ungranted_root.join("sibling.txt").exists());
    assert!(!host_temp.path().join("host.txt").exists());
}

/// C001 WP-C — the negative observations above must not be vacuous.
///
/// This is the defect-injection control: a deliberately over-broad grant must
/// make the very same ungranted sibling writable. If the control cannot
/// observe the difference, the denials in the qualification prove nothing.
#[tokio::test(flavor = "current_thread")]
async fn an_over_broad_grant_is_observable_by_the_same_probe() {
    require_native_linux_landlock();
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let host_temp = tempfile::tempdir().expect("host temp");

    let session_id = "session-native-broad-grant";
    let admitted = admit_delegated_run(session_id, workspace.path()).await;

    // The mutation: the ungranted sibling is added to the scope as if it had
    // been granted. Nothing else changes.
    let mut mutated = admitted.scope.clone();
    mutated.shared.push(AuthorizedSharedScratchRoot {
        scratch_id: ScratchId::new(),
        alias: "ungranted".to_string(),
        path: admitted.ungranted_root.clone(),
    });

    let script = write_observation_script(workspace.path());
    let mut spec_args = vec![script.to_string_lossy().into_owned()];
    spec_args.extend(observation_arguments(
        &admitted,
        host_temp.path(),
        &PathBuf::from("/dev/null"),
        workspace.path(),
    ));

    let config = workspace_write_config(workspace.path());
    let spec = config
        .launch_spec("sh", &spec_args, Some(workspace.path()))
        .expect("build the launch spec");

    let mut argv: Vec<OsString> = vec![OsString::from("sh")];
    argv.extend(spec_args.iter().map(OsString::from));

    let mut request = ManagedProcessRequest::new(
        argv,
        workspace.path().to_path_buf(),
        ProcessProvenance::new("job-native-broad", "attempt-native-broad"),
    );
    request.sandbox = SandboxRequest::Required(spec);
    request.environment_policy = EnvironmentPolicy::sanitized();
    request.execution_authorization = Some(authorization(mutated));

    let result = ManagedProcessService::run(request)
        .await
        .expect("the contained launch must succeed");
    let raw = result.stdout.to_string_lossy();
    assert!(
        raw.contains("PRIVATE_WRITE="),
        "the contained child did not report its observations: {raw:?}"
    );
    let observations = parse_observations(&raw);
    assert_allowed(&observations, "UNGRANTED_SIBLING_WRITE");
}

/// C001 WP-C — a sandbox setup failure must leave the target unexecuted.
#[tokio::test(flavor = "current_thread")]
async fn a_sandbox_setup_failure_leaves_the_target_unexecuted() {
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let credential_dir = tempfile::tempdir().expect("credential dir");
    let denied = credential_dir.path().join("credential");
    std::fs::write(&denied, "secret").expect("credential fixture");
    // A hard-linked alias makes the denied path unrepresentable: the policy
    // refuses it rather than silently dropping the exclusion.
    std::fs::hard_link(&denied, credential_dir.path().join("alias")).expect("hardlink alias");

    let marker = workspace.path().join("target-ran");
    let config = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![workspace.path().to_string_lossy().into_owned()])
        .with_deny_paths(vec![denied.to_string_lossy().into_owned()]);
    let spec = config
        .launch_spec(
            "sh",
            &["-c".to_string(), format!("touch {}", marker.display())],
            Some(workspace.path()),
        )
        .expect("build the launch spec");

    let mut request = ManagedProcessRequest::new(
        vec![
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(format!("touch {}", marker.display())),
        ],
        workspace.path().to_path_buf(),
        ProcessProvenance::new("job-native-setup", "attempt-native-setup"),
    );
    request.sandbox = SandboxRequest::Required(spec);
    request.environment_policy = EnvironmentPolicy::sanitized();

    let error = ManagedProcessService::run(request)
        .await
        .expect_err("the setup failure must fail the launch");
    assert!(
        matches!(error, ManagedProcessError::SandboxFailed(_)),
        "unexpected failure shape: {error:?}"
    );
    assert!(
        !marker.exists(),
        "the target must not run when the sandbox setup fails"
    );
}

/// C001 WP-C — an accepted constrained policy may not be silently downgraded
/// to an uncontained launch.
#[tokio::test(flavor = "current_thread")]
async fn a_required_policy_refuses_a_containment_free_launch() {
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let marker = workspace.path().join("target-ran");

    let mut request = ManagedProcessRequest::new(
        vec![
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(format!("touch {}", marker.display())),
        ],
        workspace.path().to_path_buf(),
        ProcessProvenance::new("job-native-downgrade", "attempt-native-downgrade"),
    );
    request.sandbox = SandboxRequest::Disabled;
    request.execution_authorization = Some(ExecutionAuthorizationContext {
        snapshot: ExecutionPolicySnapshot::capture(
            ApprovalMode::Interactive,
            SandboxProfile::WorkspaceWrite,
            None,
            Some("session-native-downgrade".to_string()),
            None,
            None,
            None,
        ),
        receipt: PermissionDecisionReceipt::allowed("native_linux_qualification", None),
        command_preflight: None,
        scratch_scope: None,
    });

    let error = ManagedProcessService::run(request)
        .await
        .expect_err("a constrained policy may not run without containment");
    assert!(
        matches!(error, ManagedProcessError::SandboxFailed(_)),
        "unexpected failure shape: {error:?}"
    );
    assert!(
        !marker.exists(),
        "the target must not run when containment was required but not requested"
    );
}

/// C001 WP-C — an attempt cancelled before it starts must never execute the
/// target, so a cancelled attempt leaves no side effect behind.
#[tokio::test(flavor = "current_thread")]
async fn a_cancelled_attempt_never_executes_the_target() {
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let marker = workspace.path().join("target-ran");

    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let mut request = ManagedProcessRequest::new(
        vec![
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(format!("touch {}", marker.display())),
        ],
        workspace.path().to_path_buf(),
        ProcessProvenance::new("job-native-cancel", "attempt-native-cancel"),
    );
    request.cancellation = cancellation;
    request.environment_policy = EnvironmentPolicy::sanitized();

    let error = ManagedProcessService::run(request)
        .await
        .expect_err("a cancelled attempt must not launch");
    assert!(
        matches!(error, ManagedProcessError::CancelledBeforeSpawn),
        "unexpected failure shape: {error:?}"
    );
    assert!(
        !marker.exists(),
        "a cancelled attempt must not run the target"
    );
}

/// C001 WP-C — the attempt lease must outlive the parent that took it, and
/// the shared scratch may only be reclaimed after the attempt is reaped.
#[tokio::test(flavor = "current_thread")]
async fn the_attempt_lease_outlives_its_parent_and_blocks_reclaim_until_release() {
    let _home = ScratchHome::install();
    let workspace = tempfile::tempdir().expect("workspace");
    let session_id = "session-native-lease";
    let admitted = admit_delegated_run(session_id, workspace.path()).await;

    let job_id = admitted
        .record
        .job_id
        .clone()
        .expect("the admitted run carries its job");
    let attempt_id = AttemptId::new_unchecked("attempt-native-lease");
    let run_id = admitted.record.run_id.clone();

    let leases = admitted
        .runs
        .acquire_scratch_attempt_leases(&run_id, &job_id, &attempt_id)
        .await
        .expect("the attempt takes a lease on its private and granted scratch");
    assert_eq!(
        leases.len(),
        2,
        "the lease covers the private root and the one granted shared root"
    );
    assert!(
        admitted
            .runs
            .has_scratch_attempt_lease(&admitted.granted_id, &run_id)
            .await
            .expect("lease lookup"),
        "the granted scratch must stay leased for the running attempt"
    );

    // Revoking while the attempt is still leased must not reclaim the
    // directory: a running child may still be writing there.
    admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "scratch_revoke",
                "scratch_id": admitted.granted_id.to_string()
            }),
            Some(task_context(session_id, "task-revoke-while-leased")),
        )
        .await
        .expect("revoking a leased scratch is allowed");
    assert!(
        admitted.granted_root.exists(),
        "a leased scratch must not be reclaimed while the attempt can still use it"
    );

    // Reaping the attempt releases the lease, and only then may the revoked
    // scratch be reclaimed.
    admitted
        .runs
        .release_scratch_attempt_leases(&job_id, &attempt_id)
        .await
        .expect("release the attempt leases");
    assert!(
        !admitted
            .runs
            .has_scratch_leases(&admitted.granted_id)
            .await
            .expect("lease lookup"),
        "the lease must be gone once the attempt is reaped"
    );

    admitted
        .task
        .execute_structured(
            serde_json::json!({
                "action": "scratch_revoke",
                "scratch_id": admitted.granted_id.to_string()
            }),
            Some(task_context(session_id, "task-revoke-after-reap")),
        )
        .await
        .expect("revoking a released scratch is allowed");
    assert!(
        !admitted.granted_root.exists(),
        "a revoked and unleased scratch must be reclaimed"
    );
}
