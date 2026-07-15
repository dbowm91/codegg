# Single-Daemon Phase 3: Daemon-Owned Workspace Services and Storage

## Status

Proposed third implementation phase for the single-daemon multi-project orchestration roadmap.

Phase 2 establishes typed workspace identity and propagates an immutable execution context. This phase turns each workspace into a daemon-owned service domain so expensive, stateful, and concurrency-sensitive components are created once and shared across every session and frontend attached to that workspace.

## 1. Problem statement

The current repository creates several workspace-related services at turn, tool-registry, TUI, or launch-mode boundaries:

- `build_session_tool_registry` constructs `FsRunStore` from a cwd-derived `.codegg/runs` root;
- the TUI may separately construct a RunStore for run-detail rendering;
- LSP services can be constructed through tool defaults or injected inconsistently;
- configuration is reloaded in daemon request paths and turn runtime paths;
- Git repository context and service helpers are resolved per operation;
- plugin/search/provider bootstrapping can be repeated from several runtime paths;
- per-workspace mutation serialization is distributed across individual executor implementations;
- daemon storage is selected from one startup directory, which conflates daemon catalog state with project-local artifacts.

In a multi-project daemon, these patterns create three classes of defects:

1. separate service instances can target the same on-disk state without sharing locks;
2. a service can be rooted in the daemon process cwd rather than the session workspace;
3. global daemon catalog data and workspace-local artifacts do not have a deliberate ownership split.

## 2. Goals

### 2.1 Functional goals

- Add one `WorkspaceServices` bundle per active `WorkspaceId`.
- Make the daemon registry the sole production constructor for workspace-scoped RunStore, configuration, LSP, Git, path policy, and lock services.
- Share one workspace service bundle across all sessions and clients for the same workspace.
- Separate user-scoped daemon catalog persistence from workspace-local artifacts and caches.
- Provide bounded service activation and idle eviction without destroying durable state.
- Make service health, activation state, and last-use visible in daemon snapshots.
- Preserve compatibility adapters for existing tool and TUI APIs during migration.

### 2.2 Correctness goals

- Ensure one in-process synchronization domain protects each workspace RunStore index.
- Ensure LSP servers and semantic caches are keyed by workspace, language, and configuration rather than process cwd.
- Ensure Git mutation locks are workspace/repository scoped and shared by native and Bash-translated operations.
- Ensure configuration is resolved from the canonical workspace root and held stable for an execution attempt.
- Ensure artifacts for workspace A cannot be written under workspace B.
- Ensure service eviction cannot occur while an execution holds a lease.

### 2.3 Maintainability goals

- Centralize service construction and teardown.
- Prevent individual tools from constructing filesystem-backed services.
- Use narrow service traits so `codegg-core` does not acquire UI/server/plugin dependencies.
- Keep daemon catalog and workspace artifact schemas independently evolvable.

## 3. Non-goals

This phase does not:

- implement the durable scheduler itself;
- route all subprocesses through global admission;
- distribute workspace services across processes or machines;
- replace SQLite with a client-server database;
- run one separate daemon per workspace;
- create containers or virtual machines per workspace;
- automatically delete inactive workspace artifacts;
- redesign every existing service API before establishing the registry boundary.

## 4. Target service model

### 4.1 Workspace service bundle

Add a type under `src/core/workspace_services.rs` or an extracted core/runtime crate if dependency boundaries permit:

```rust
pub struct WorkspaceServices {
    pub workspace: WorkspaceRecord,
    pub config: Arc<ResolvedProjectConfig>,
    pub run_store: Arc<dyn RunStore>,
    pub lsp: Arc<LspService>,
    pub git: Arc<GitWorkspaceService>,
    pub path_policy: Arc<WorkspacePathPolicy>,
    pub locks: Arc<WorkspaceLockTable>,
    pub artifact_root: PathBuf,
    pub activated_at: DateTime<Utc>,
    pub last_used_at: AtomicI64,
    active_leases: AtomicUsize,
    shutdown: CancellationToken,
}
```

Not every field must be public. The important invariant is that service ownership is keyed by validated workspace identity rather than session or frontend lifetime.

### 4.2 Lease-based access

The registry should return a lease:

```rust
pub struct WorkspaceServicesLease {
    services: Arc<WorkspaceServices>,
}
```

Creating a lease increments active-use accounting; dropping it decrements accounting. Idle eviction must require zero active leases and no workspace-pinned services.

### 4.3 Registry

```rust
pub struct WorkspaceServiceRegistry {
    workspaces: Arc<WorkspaceRegistry>,
    active: DashMap<WorkspaceId, Arc<WorkspaceServices>>,
    activation_locks: DashMap<WorkspaceId, Arc<tokio::sync::Mutex<()>>>,
    factory: Arc<dyn WorkspaceServicesFactory>,
    policy: WorkspaceServicePolicy,
}
```

Required operations:

```rust
acquire(workspace_id) -> WorkspaceServicesLease
peek(workspace_id) -> Option<WorkspaceServiceSnapshot>
list_active() -> Vec<WorkspaceServiceSnapshot>
evict_idle(now) -> EvictionReport
shutdown_all(deadline) -> ShutdownReport
reload_config(workspace_id) -> ReloadResult
```

Concurrent first acquisition must produce exactly one service bundle. Use a per-workspace activation mutex or equivalent single-flight mechanism.

## 5. Storage ownership split

### 5.1 User-scoped daemon catalog

Move or establish the daemon database in the user-scoped Codegg data directory rather than the daemon launch project.

The daemon catalog should own:

- workspace registry;
- session catalog and messages, unless a later portability design explicitly moves them;
- client/control lease metadata where durability is needed;
- durable jobs, attempts, schedules, dependencies, and leases from Phase 4;
- daemon event-log persistence or event checkpoints;
- notification history;
- global user preferences and provider/model discovery metadata;
- migration/version metadata.

Recommended path shape:

```text
macOS: ~/Library/Application Support/codegg/codegg.db
Linux: $XDG_DATA_HOME/codegg/codegg.db
fallback: ~/.local/share/codegg/codegg.db
```

The exact existing path helper conventions should be reused rather than duplicating platform logic.

### 5.2 Workspace-local state

Workspace-local `.codegg/` remains appropriate for project-derived artifacts:

```text
<workspace>/.codegg/
  runs/
  test-runs/
  research/
  snapshots/
  checkpoints/
  caches/
  tmp/
```

These artifacts must be recreatable or inspectable with the workspace. They should not contain daemon-global coordination state whose absence could cause duplicate execution.

### 5.3 Storage registry and compatibility

Refactor `crates/codegg-core/src/storage/mod.rs` so callers can explicitly initialize:

```rust
init_daemon_catalog(paths: &DaemonPaths) -> SqlitePool
init_legacy_project_store(project_root: &Path) -> SqlitePool
```

Do not leave a generic `init(project_dir: &str)` as the ambiguous production entry point. Retain it temporarily as a deprecated wrapper for standalone/tests.

Add a `StorageLayoutVersion` or migration marker so existing project-local session databases can be discovered and imported deliberately.

## 6. RunStore ownership

### 6.1 Single instance per workspace

`WorkspaceServicesFactory` constructs one `FsRunStore` rooted at:

```text
<workspace_root>/.codegg/runs
```

All tools, TestRunner, Python, Git, TUI run-detail requests, and protocol artifact reads must use the same `Arc<dyn RunStore>` from the workspace services lease.

Remove production RunStore construction from:

- `src/tool/factory.rs`;
- TUI app initialization;
- individual TestTool/Python/Git/Bash constructors;
- any helper that derives `.codegg/runs` from `current_dir()`.

### 6.2 Concurrency and durability

The current filesystem RunStore synchronization must be reviewed under shared multi-session use:

- one workspace instance must serialize index rewrite/append according to existing invariants;
- repeated `FsRunStore::new` against the same root must not occur in production;
- artifact writes and manifest completion must remain atomic;
- cleanup/retention must coordinate with active writes;
- read APIs must remain available to several clients concurrently;
- pinned and failed-run retention behavior must remain intact.

Add workspace ID to run manifests if not already present. If schema compatibility requires an optional field, make new daemon writes populate it unconditionally.

## 7. Configuration service

### 7.1 Stable resolved configuration

Add a workspace configuration service that loads user and project configuration from explicit paths and produces an immutable `ResolvedProjectConfig`.

The service should track:

```rust
pub struct WorkspaceConfigSnapshot {
    pub revision: u64,
    pub loaded_at: DateTime<Utc>,
    pub source_files: Vec<PathBuf>,
    pub config: Arc<ResolvedProjectConfig>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}
```

Executions acquire a snapshot revision. Configuration reloads affect future jobs/turns but do not mutate the policy of an already admitted/running attempt.

### 7.2 Reload behavior

- file watcher or explicit command may request reload;
- parse/validation failure retains the previous valid snapshot and exposes diagnostics;
- reload is serialized per workspace;
- provider credentials and other user-global secret resolution remain in the appropriate global service;
- project configuration cannot widen hard user/daemon security policy beyond allowed merge semantics.

## 8. LSP service ownership

One daemon may serve several projects, each with several language servers. LSP ownership must therefore be keyed at least by workspace and server/language configuration.

Required behavior:

- construct LSP service from the workspace root and resolved config;
- reuse servers across sessions in the same workspace;
- do not share language-server processes across unrelated roots unless the server explicitly supports a multi-root model and Codegg models that safely;
- apply existing restart policy per workspace/server;
- stop idle LSP processes during workspace service eviction;
- expose server health and restart state through workspace snapshots;
- keep LSP context assembly rooted in the execution context from Phase 2.

The LSP tool should receive the workspace-owned service through `ToolRegistryOptions`, never instantiate a default service in production when none is supplied.

## 9. Git and mutation lock ownership

Add `GitWorkspaceService` or reuse existing Git service abstractions behind the workspace bundle.

It should own:

- canonical repository discovery from workspace root;
- cached read service state where safe;
- `GitEnvPolicy` configuration;
- mutation lock table keyed by canonical repository/worktree root;
- worktree metadata and invalidation;
- integration with RunStore and workspace ID;
- optional repository snapshots needed by permission prompts.

Native Git tool calls and Bash-translated Git operations must acquire the same mutation lock. Do not permit separate lock domains based on execution origin.

Locks should be narrow:

- read-only Git operations may run concurrently unless an executor-specific limitation exists;
- worktree/index mutations acquire a repository mutation key;
- network reads may use network permits later but need not block unrelated local reads;
- destructive operations retain typed permission/preflight policy in addition to locking.

## 10. Workspace path policy

Move path-policy derivation from individual tools into the workspace bundle.

`WorkspacePathPolicy` should represent:

- canonical workspace root;
- approved additional read roots;
- approved additional write roots;
- hard-denied sensitive paths;
- symlink policy;
- temporary directory policy;
- sandbox mode and platform capabilities.

Tools receive a cloneable capability object or `Arc<WorkspacePathPolicy>`. They should not independently call global helpers that produce broad default paths without workspace context.

## 11. Core and protocol integration

### `CoreDaemon`

Add `workspace_services: Arc<WorkspaceServiceRegistry>` and acquire a lease when handling:

- turn submission;
- workspace snapshots;
- artifact/run queries;
- config reload;
- workspace service health requests.

The turn runtime receives the lease or the service references necessary for execution. The daemon should avoid holding a lease for purely catalog-level session list operations.

### `CoreRuntimeDeps`

Move workspace-scoped objects out of global dependency fields. `CoreRuntimeDeps` should retain global services and the workspace registry/factory, not one LSP/RunStore tied to daemon cwd.

### Protocol additions

Add request/response families such as:

```rust
WorkspaceServicesSnapshot { workspace_id }
WorkspaceConfigReload { workspace_id }
RunList { workspace_id, query }
RunGet { workspace_id, run_id }
RunArtifactRead { workspace_id, artifact_id, range }
```

These requests let remote/socket TUIs render run details without opening workspace filesystem state directly.

Add service health fields to daemon/workspace snapshots:

- active/inactive;
- active lease count;
- config revision;
- LSP server count and health summary;
- RunStore root and health status without exposing sensitive paths to unauthorized remote clients;
- last-used timestamp.

## 12. Service lifecycle and eviction

### Activation

Workspace service activation must be lazy and single-flight. A workspace may remain registered without active services.

### Idle eviction

Add conservative policy fields:

```toml
[daemon.workspace_services]
max_active_workspaces = 16
idle_evict_secs = 1800
keep_lsp_warm_secs = 600
```

Eviction requirements:

- zero active leases;
- no running/admitted job referencing the workspace;
- no explicit pin;
- flush/finish RunStore operations;
- stop language servers with a bounded timeout;
- remove the active map entry only after shutdown succeeds or is force-aborted according to policy.

Eviction must not archive or delete the workspace record or artifacts.

### Shutdown

Daemon shutdown drains service users, then calls `shutdown_all` with a deadline. Report services that required forced termination.

## 13. Migration strategy

### Project-local databases

Provide a migration/import command or startup discovery path:

```text
codegg daemon migrate-project <workspace-root>
```

Migration should:

- discover `<workspace>/.codegg/sessions.db`;
- verify schema/version;
- import sessions/messages/tasks into the daemon catalog with workspace binding;
- preserve source database until explicit cleanup;
- record migration provenance and idempotency marker;
- avoid duplicate imports on repeated execution.

Automatic migration may be offered after explicit user confirmation, but silent destructive relocation is out of scope.

### Existing RunStore

Existing `<workspace>/.codegg/runs` data remains in place. The workspace service adopts it. Add schema/version validation and health diagnostics rather than moving the directory.

### TUI compatibility

During migration, the TUI may retain a compatibility RunStore reference only when running explicit standalone mode. Daemon-client mode must use protocol requests to inspect runs.

## 14. Testing plan

Use `--test-threads=1` for Rust tests.

### Registry tests

- concurrent acquisition creates one service bundle;
- different workspaces create distinct bundles;
- lease count increments/decrements through RAII;
- idle eviction refuses active services;
- idle eviction shuts down and removes inactive services;
- failed activation does not leave a poisoned active entry;
- config reload swaps future snapshots but not existing attempt snapshots.

### RunStore tests

- two sessions in one workspace share the same store instance;
- concurrent run writes remain consistent;
- TUI/protocol reads observe runs written by TestRunner/Bash/Python/Git;
- cleanup cannot delete an active run;
- artifacts are rooted in the correct workspace;
- workspace ID is present in new manifests;
- no per-tool constructor creates another store.

### LSP tests

- sessions in one workspace share LSP state;
- separate workspaces receive separate roots/processes;
- eviction stops workspace LSP processes;
- reload changes future server configuration safely;
- absent injected LSP in daemon mode is an error/diagnostic, not silent default construction.

### Git/lock tests

- native and Bash-translated mutation requests contend on one lock;
- two repositories in separate workspaces mutate concurrently when resource policy allows;
- read-only operations are not unnecessarily serialized;
- lock release occurs on timeout, cancellation, permission denial, and process error.

### Storage migration tests

- import one legacy project database;
- import several project databases into one catalog;
- repeated import is idempotent;
- invalid schema is rejected without modifying source;
- sessions retain correct workspace mapping;
- source files remain untouched.

### Integration test

Start one daemon with two workspaces and multiple sessions. Run concurrent commands, config reloads, RunStore reads, LSP queries, and Git operations. Assert service sharing within a workspace and isolation across workspaces.

## 15. Acceptance criteria

Phase 3 is complete when:

- one daemon-owned `WorkspaceServices` instance serves all sessions for a workspace;
- production tools and TUIs no longer construct their own filesystem RunStore;
- RunStore writes/readers share one concurrency domain per workspace;
- configuration is resolved from explicit workspace roots and versioned by snapshot revision;
- LSP processes are workspace-owned, shared within a workspace, and isolated across workspaces;
- Git mutation locks are shared across native and Bash-translated execution;
- daemon catalog state is user-scoped and workspace artifacts remain workspace-local;
- existing project-local sessions can be imported without data loss or duplicate import;
- idle service eviction is lease-safe and observable;
- multi-workspace service integration tests pass.

## 16. Handoff checklist

- [ ] Add `WorkspaceServices`, lease, factory, registry, and policy types.
- [ ] Establish user-scoped daemon catalog initialization.
- [ ] Add explicit legacy project-store initialization and migration tooling.
- [ ] Move RunStore construction into workspace services.
- [ ] Route TUI run inspection through daemon protocol.
- [ ] Add workspace config snapshots and reload semantics.
- [ ] Move LSP ownership into workspace services.
- [ ] Add shared Git service and mutation lock table.
- [ ] Centralize workspace path policy.
- [ ] Add service lifecycle, idle eviction, and shutdown handling.
- [ ] Add health/snapshot protocol fields and diagnostics.
- [ ] Add concurrency, migration, service-sharing, and isolation tests.
- [ ] Update storage, core, tool, LSP, Git, and RunStore architecture docs.
