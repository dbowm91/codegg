# Single-Daemon Phase 2: Workspace Registry and Execution Context

## Status

Proposed second implementation phase for the single-daemon multi-project orchestration roadmap.

This phase makes workspace identity a first-class daemon concept and propagates an immutable, daemon-resolved execution context through every existing execution path that can run against project files. It is the prerequisite for safe multi-project orchestration.

## 1. Problem statement

The current daemon can store a session's `project_id` and `directory`, but the active execution path does not consistently use them:

- `SessionRuntime` stores `project_id` and `directory`;
- `SessionLoad`/`SessionAttach` populate runtime state from persisted session metadata;
- `TurnSubmit`, `AgentSelect`, and `ModelSelect` can create fallback runtimes using `session_id` as project identity and `.` as directory;
- `TurnRunInput` does not carry workspace identity or a canonical root;
- turn-time LSP and Git context use `std::env::current_dir()`;
- `build_session_tool_registry` creates RunStore from process cwd;
- subagent requests carry parent session identity but no canonical workspace root;
- Bash, TestRunner, Python, Git, managed argv, and raw shell can receive ad hoc working directories rather than one daemon-enforced root.

A long-lived daemon launched from project A can therefore execute a session for project B using project A's cwd-derived services. This is incompatible with safe multi-project ownership.

## 2. Goals

### 2.1 Functional goals

- Introduce a durable, typed `WorkspaceId`.
- Register and deduplicate canonical project roots in a daemon-owned `WorkspaceRegistry`.
- Bind every persisted session to exactly one workspace.
- Resolve workspace context inside the daemon before turn or job execution.
- Propagate workspace root, workspace ID, session ID, cancellation, and path policy through turns, tools, subagents, tests, Python, Git, LSP, RunStore, and process execution.
- Eliminate process-global cwd as an execution identity source in daemon-owned paths.
- Preserve explicit per-command subdirectories only when they resolve inside the workspace policy.
- Expose workspace metadata through protocol snapshots and daemon status.

### 2.2 Safety goals

- Canonicalize roots before registration and reject nonexistent or disallowed roots.
- Prevent symlink aliases from producing duplicate active workspace identities.
- Prevent a client from rebinding an existing session to another workspace through a turn request.
- Prevent tools from escaping the workspace via `..`, symlink traversal, or arbitrary absolute cwd overrides.
- Keep read/write root policy explicit and immutable for an execution attempt.
- Preserve existing permission, sandbox, Git, and command-routing checks as additional layers.

### 2.3 Maintainability goals

- Use one context object rather than adding independent `workspace_root` parameters throughout the codebase.
- Resolve session/workspace context once per turn/job, not once per tool call.
- Centralize path canonicalization and relative-cwd validation.
- Add static checks that prevent new daemon execution code from calling `std::env::current_dir()`.

## 3. Non-goals

This phase does not:

- implement machine-wide admission control;
- introduce durable jobs or schedules;
- move RunStore/LSP ownership into a reusable workspace service cache beyond what is required for context propagation;
- redesign project configuration merge semantics;
- support one session spanning multiple workspaces;
- permit arbitrary remote filesystem roots;
- add distributed workspace identity across machines;
- remove all uses of `current_dir()` from CLI-only or standalone code.

## 4. Domain model

### 4.1 Typed workspace identity

Add workspace types to `codegg-core` or another UI-independent core crate:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: WorkspaceId,
    pub canonical_root: PathBuf,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}
```

ID generation should be stable for the persisted record but not derived solely from path text. Canonical root uniqueness should be enforced in storage.

### 4.2 Workspace registry

Introduce a daemon-owned registry:

```rust
pub struct WorkspaceRegistry {
    store: Arc<dyn WorkspaceStore>,
    active: DashMap<WorkspaceId, Arc<WorkspaceRuntime>>,
    by_root: DashMap<PathBuf, WorkspaceId>,
}
```

Required operations:

```rust
register(root) -> WorkspaceRecord
resolve(id) -> WorkspaceRecord
resolve_root(root) -> WorkspaceRecord
get_or_register(root) -> WorkspaceRecord
archive(id)
list(include_archived)
```

Registration must canonicalize the root, verify directory accessibility, and reject paths that are files or resolve through unsupported indirection.

### 4.3 Execution context

Add an immutable context passed by `Arc`:

```rust
pub struct ExecutionContext {
    pub workspace_id: WorkspaceId,
    pub workspace_root: PathBuf,
    pub session_id: Option<String>,
    pub project_config: Arc<ResolvedProjectConfig>,
    pub allowed_read_roots: Arc<[PathBuf]>,
    pub allowed_write_roots: Arc<[PathBuf]>,
    pub cancellation: CancellationToken,
}
```

RunStore and workspace services may be added in Phase 3. In this phase, include them only if needed to avoid temporary duplicate context types.

The context must expose helpers rather than requiring every caller to implement path checks:

```rust
impl ExecutionContext {
    pub fn resolve_relative_cwd(&self, requested: Option<&Path>) -> Result<PathBuf, PathPolicyError>;
    pub fn resolve_read_path(&self, requested: &Path) -> Result<PathBuf, PathPolicyError>;
    pub fn resolve_write_path(&self, requested: &Path) -> Result<PathBuf, PathPolicyError>;
}
```

Relative paths are resolved against `workspace_root`. Absolute paths are accepted only when they fall under an allowed root. Canonicalization must account for paths that do not yet exist by canonicalizing the nearest existing ancestor and validating the remaining suffix.

### 4.4 Session binding

The authoritative relation is:

```text
Session -> WorkspaceId -> canonical root
```

Do not continue treating `project_id` as an arbitrary directory string in new daemon APIs. Preserve compatibility fields in storage/DTOs during migration, but add a real `workspace_id` column or typed mapping.

## 5. Storage and migration

### 5.1 Workspace table

Add a migration resembling:

```sql
CREATE TABLE workspace (
    id TEXT PRIMARY KEY,
    canonical_root TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_last_opened INTEGER NOT NULL,
    time_archived INTEGER
);
```

Add `workspace_id` to sessions:

```sql
ALTER TABLE session ADD COLUMN workspace_id TEXT;
CREATE INDEX idx_session_workspace ON session(workspace_id);
```

Migration behavior:

1. For each existing session, derive a candidate root from `directory`, falling back to `project_id` only when valid.
2. Canonicalize existing roots where possible.
3. Create one workspace record per canonical root.
4. Populate `session.workspace_id`.
5. Leave invalid/missing roots unbound with explicit migration diagnostics rather than binding to `.`.
6. New daemon execution must reject unbound sessions until repaired or explicitly rebound through a workspace command.

Do not destroy `project_id` or `directory` fields in this phase. They remain compatibility projections.

### 5.2 User-scoped versus project-local database

If Phase 1 still uses one current project database, introduce the workspace schema in a way that can migrate to the user-scoped daemon catalog in Phase 3. Avoid foreign-key assumptions tied to a project-local path that would make later relocation difficult.

## 6. Protocol changes

Extend `codegg-protocol` with typed workspace DTOs and requests:

```rust
WorkspaceRegister { root: String }
WorkspaceList { include_archived: bool }
WorkspaceOpen { workspace_id: String }
WorkspaceArchive { workspace_id: String }
SnapshotWorkspace { workspace_id: String }
```

Update session creation:

```rust
SessionCreate {
    workspace_id: String,
    title: Option<String>,
}
```

During compatibility, accept `directory` only through a legacy variant or version-gated field. The daemon must resolve it to a workspace before creating a session.

Extend snapshots:

```rust
pub struct WorkspaceSnapshot {
    pub workspace_id: String,
    pub canonical_root: String,
    pub display_name: String,
    pub active_sessions: usize,
}
```

`SessionSnapshot` should include `workspace_id` and may retain `project_id` as a compatibility field.

Bump protocol version only if wire compatibility cannot be maintained through additive fields and unknown-variant tolerance. Add capability flags for workspace registration and workspace snapshots.

## 7. Execution propagation

### 7.1 `CoreDaemon`

In `src/core/daemon.rs`:

- add `workspaces: Arc<WorkspaceRegistry>`;
- resolve `session_id` through `SessionStore` before any turn submission;
- reject missing/unbound sessions instead of creating runtime state with `.`;
- construct `ExecutionContext` from the session's workspace;
- pass the context into `TurnRunInput`;
- update `AgentSelect` and `ModelSelect` to require an existing session/runtime binding rather than creating synthetic roots;
- include workspace metadata in daemon/session snapshots.

### 7.2 `SessionRuntime`

Replace free-form project/directory fields with typed workspace identity plus a cached canonical root projection:

```rust
pub struct SessionRuntime {
    pub session_id: String,
    pub workspace_id: WorkspaceId,
    pub workspace_root: PathBuf,
    // existing fields...
}
```

`get_or_create` must accept a validated binding object. Remove callers that provide `session_id` and `.` placeholders.

### 7.3 `TurnRunInput` and turn runtime

Add:

```rust
pub execution: Arc<ExecutionContext>,
```

Use `execution.workspace_root` for:

- Git context construction;
- LSP context assembly;
- project instructions and prompt loading;
- tool registry construction;
- goal checkpoint path resolution;
- plugin lifecycle metadata.

Remove daemon-mode calls to `std::env::current_dir()` in `src/agent/turn_runtime.rs`.

### 7.4 Tool registry and tools

Extend `ToolRegistryOptions` with `execution_context`.

Tools with filesystem or process effects must receive either the context or a narrow derived capability:

- Bash;
- read/edit/write/glob/grep/list;
- ApplyPatch/diff/replace/review where paths are used;
- TestTool;
- PythonScriptTool;
- GitTool;
- terminal/managed process tools;
- LSP tool;
- research/local source tools where repository paths are consumed.

Do not rely on the model supplying `workdir`. The tool should default to the immutable workspace root. A user/model-supplied `workdir` is a relative subdirectory request validated by `ExecutionContext`.

### 7.5 Bash and managed process execution

Refactor Bash construction so it owns a workspace root and path policy:

```rust
BashTool::new(execution: Arc<ExecutionContext>)
```

Behavior:

- absent `workdir`: use workspace root;
- relative `workdir`: resolve under workspace root;
- absolute `workdir`: require explicit allowed-root membership;
- raw shell, managed argv, TestRunner delegation, Python delegation, and Git delegation all receive the same resolved cwd;
- no fallback path may replace the cwd with process-global cwd.

### 7.6 TestRunner

Require `TestRunRequest.workdir` to be daemon-resolved before entry. Add workspace/session identity if not already represented in RunStore persistence and events.

The resolver should not call `current_dir()` for daemon-originated requests. Standalone APIs may provide an explicit compatibility constructor.

### 7.7 Python and Git

- Python script requests receive canonical workspace root and path policy.
- Git repo root resolution starts from the execution context, not process cwd.
- `git -C` and tool `workdir` remain subject to workspace policy.
- Git RunStore records include workspace ID.
- Python snapshots and changed-file detection remain workspace-scoped.

### 7.8 Subagents

Extend `SubAgentRequest` with workspace identity/context or a serializable execution-context reference:

```rust
pub workspace_id: WorkspaceId,
pub workspace_root: PathBuf,
```

The preferred in-process path is to pass `Arc<ExecutionContext>` through the worker request. If serialization is needed later, use a context ID resolved by the daemon.

Subagents must build their tool registry through the same session/workspace factory as parent turns. Remove `ToolRegistry::with_config(&config)` from the subagent execution path when it bypasses workspace binding.

### 7.9 Background tasks

Until durable jobs land, every existing scheduled task must resolve its parent session to a workspace before dispatch. Tasks with missing or invalid session bindings should be marked failed/interrupted and not run in daemon cwd.

## 8. Static and runtime enforcement

### Static check

Add a script such as `scripts/check-daemon-cwd-usage.sh` or Python equivalent that rejects new uses of:

```rust
std::env::current_dir()
std::env::set_current_dir(...)
```

inside daemon execution modules, with a small explicit allowlist for CLI bootstrap and standalone compatibility code.

Suggested protected paths:

- `src/core/`;
- `src/agent/turn_runtime.rs`;
- `src/agent/worker.rs`;
- `src/tool/` process/filesystem executors;
- `src/test_runner/`;
- `src/python_script/`;
- Git execution services.

### Runtime assertions

In debug/test builds, attach workspace ID/root to run and event metadata and assert that executor cwd is within the configured roots before process spawn.

## 9. Configuration resolution

Introduce `ResolvedProjectConfig` if one does not already exist as a stable type. Resolution order should be explicit:

1. hardcoded defaults;
2. user-level configuration;
3. workspace-local configuration;
4. session-safe overrides;
5. execution-specific bounded overrides.

The resolved config should be tied to the workspace context for the duration of a turn/job. Do not reload arbitrary process cwd configuration inside an executor.

Configuration file discovery must begin at `workspace_root`.

## 10. Testing plan

Use `--test-threads=1` for Rust tests.

### Workspace registry tests

- canonical root registration;
- duplicate registration through symlink/relative aliases;
- nonexistent path rejection;
- file path rejection;
- archive/list/open behavior;
- concurrent `get_or_register` calls return one workspace;
- migration from several sessions sharing one root;
- invalid legacy session remains explicitly unbound.

### Path policy tests

- absent cwd resolves to workspace root;
- relative cwd resolves inside workspace;
- `..` escape is rejected;
- absolute outside path is rejected;
- allowed additional root is accepted;
- symlink escape is rejected;
- nonexisting write target with escaping parent is rejected;
- Unicode and case-normalization behavior is documented/tested per platform.

### Multi-project integration tests

Create temporary projects A and B, start one daemon, create one session per workspace, and submit operations concurrently.

Assert:

- each Bash command writes only inside its own workspace;
- Git context reports the correct repository;
- LSP root belongs to the correct workspace;
- test artifacts are written under the correct workspace;
- Python changed-file snapshots do not include the other project;
- subagents inherit the parent workspace;
- a malicious `workdir` cannot cross into the other workspace.

Use marker files to make cwd attribution deterministic.

### Protocol tests

- workspace register/list/snapshot round trips;
- session creation requires valid workspace ID;
- legacy directory-based create resolves once and returns workspace ID;
- stale clients cannot rebind session workspace;
- snapshots include correct workspace identity.

### Static check tests

Add the cwd-use check to CI and include a fixture proving that prohibited use fails the script.

## 11. Acceptance criteria

Phase 2 is complete when:

- every active session is bound to one persisted `WorkspaceId`;
- every daemon-owned turn receives an immutable `ExecutionContext`;
- turns, tools, subagents, tests, Python, Git, LSP, and process execution use the session workspace root;
- daemon execution paths no longer infer project identity from process-global cwd;
- arbitrary cwd overrides cannot escape workspace policy;
- fallback runtime creation with `session_id` and `.` is removed;
- workspace and session snapshots expose typed workspace identity;
- two-project integration tests prove correct isolation and attribution;
- existing standalone APIs remain explicit and are documented as requiring caller-provided context;
- the static cwd-use guard is active in validation/CI.

## 12. Handoff checklist

- [ ] Add workspace domain types and storage migration.
- [ ] Implement `WorkspaceRegistry` and canonical root deduplication.
- [ ] Add session `workspace_id` binding and migration diagnostics.
- [ ] Add workspace protocol requests, responses, snapshots, and capabilities.
- [ ] Add `ExecutionContext` and path-resolution helpers.
- [ ] Propagate context through `CoreDaemon`, session runtime, and `TurnRunInput`.
- [ ] Bind tool registry and all path/process tools to workspace context.
- [ ] Propagate workspace context to TestRunner, Python, Git, LSP, and subagents.
- [ ] Remove fallback runtime roots and daemon-mode cwd inference.
- [ ] Add static cwd-use validation and multi-project isolation tests.
- [ ] Update architecture and migration documentation.
