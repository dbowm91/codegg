# Single-Daemon Multi-Project Orchestration Roadmap

## Status

Proposed roadmap for evolving Codegg from a multi-client session daemon with independently invoked execution backends into a single user-scoped daemon that owns, schedules, supervises, and records execution across multiple projects.

This roadmap is intentionally incremental. It preserves the existing `CoreDaemon`, socket protocol, session runtime registry, event log, command routing, TestRunner, RunStore, subagent pool, and tool implementations. The work introduces the missing ownership and admission boundaries around those components rather than replacing them.

## 1. Target outcome

The production topology should converge on one long-lived daemon per operating-system user:

```text
TUI in project A ─┐
TUI in project B ─┼── local transport ── one codegg daemon
CLI / automation ─┤                         │
HTTP frontend ────┘                         ├── workspace registry
                                            ├── session runtimes
                                            ├── durable job queue
                                            ├── global admission control
                                            ├── executor registry
                                            ├── schedules
                                            ├── event/replay system
                                            └── shared machine resources
```

The daemon must be the sole production owner of:

- workspace registration and canonical path identity;
- session-to-workspace binding;
- agent turns and subagents;
- builds, tests, lints, formatting, shell commands, Python runs, and Git mutations;
- resource admission and concurrency limits;
- scheduled and deferred work;
- cancellation, recovery, and execution leases;
- workspace-scoped services such as RunStore and LSP;
- durable execution history and frontend event delivery.

A TUI is a frontend. It must not create an independent execution environment merely because it was launched from another project directory.

## 2. Current architectural baseline

The repository already contains substantial prerequisite infrastructure:

- `src/core/daemon.rs` provides a daemon composition root with a daemon ID, event log, session runtime registry, client registry, notification routing, recovery hooks, and request handling.
- `src/core/transport/daemon_socket.rs` accepts multiple local socket clients, negotiates client identity, supports event subscriptions, and replays persisted events.
- `src/core/session_runtime.rs` tracks per-session active turns, control channels, selected model/agent, attached clients, pending interactions, token counts, and subagent counts.
- `src/agent/turn_runtime.rs` separates daemon request handling from turn construction and execution.
- `src/agent/worker.rs` already provides one bounded daemon-local subagent pool.
- `src/test_runner/` supervises test processes, handles timeouts, captures logs, produces structured reports, and emits lifecycle events.
- `crates/codegg-core/src/run_store.rs` provides structured execution manifests and artifact persistence.
- `src/tool/bash.rs`, command-intent planning, Python scripting, Git execution, and TestRunner delegation already establish canonical execution backends and no-double-execution rules.

The remaining problem is architectural ownership:

1. production clients can still select in-process or stdio cores and thereby create independent runtimes;
2. daemon startup is not protected by a process-lifetime singleton lock;
3. workspace identity is stored in session metadata but is not propagated as an immutable execution context;
4. daemon execution paths still use process-global `current_dir()` in critical locations;
5. RunStore and other workspace services can be constructed per turn or per frontend rather than owned once per workspace;
6. the current background scheduler is a timer over subagent requests, not a durable machine-wide job scheduler;
7. heavy executors do not acquire permits from one global admission controller;
8. client attachment is observable but does not yet enforce control leases;
9. `TurnSubmit` still lets clients provide authoritative agents, model selection, and message history.

## 3. Architectural principles

### 3.1 One production daemon

Socket-backed daemon operation becomes the normal local mode. In-process execution remains available for unit tests, embedding, and explicit standalone diagnostics, but must not be the default production path.

### 3.2 Workspace identity is immutable execution input

Every daemon-owned execution receives a canonical `WorkspaceId` and root resolved by the daemon. Executors must not infer their workspace from process-global cwd.

### 3.3 Heavy work enters through one scheduler

Any operation that spawns a process, performs a model turn, or consumes a bounded external resource must enter through the daemon scheduler. Lightweight reads may remain direct when they do not consume scarce resources.

### 3.4 Scheduling and execution are separate

The scheduler decides when work may begin. Executor implementations decide how admitted work runs. RunStore records execution but is not an admission mechanism.

### 3.5 Durable identity and lifecycle

Jobs, schedules, attempts, and leases use stable typed identifiers. A daemon restart must not leave ambiguous `running` state. Recovery is policy-driven and idempotent.

### 3.6 Shared workspace services

A workspace has one daemon-owned service bundle, including its RunStore, configuration snapshot, LSP manager, Git service, filesystem policy, and workspace locks.

### 3.7 Frontends are untrusted state projections

Clients request actions and render events. The daemon resolves authoritative session history, workspace configuration, agent definitions, permissions, and execution policy.

### 3.8 No double execution

A routing, persistence, scheduler, or transport failure must never cause Codegg to execute a logical command twice. This existing invariant remains mandatory across queued and delegated execution.

## 4. Proposed top-level architecture

```text
CoreDaemon
├── DaemonInstanceGuard
├── WorkspaceRegistry
│   └── WorkspaceServices
│       ├── ResolvedProjectConfig
│       ├── RunStore
│       ├── LspService / language servers
│       ├── GitService
│       ├── FilesystemPolicy
│       └── WorkspaceLockTable
├── SessionRuntimeRegistry
├── ClientRegistry / control leases
├── JobScheduler
│   ├── DurableJobStore
│   ├── AdmissionController
│   ├── ResourceBudget
│   ├── FairQueue
│   └── ExecutorRegistry
├── ScheduleService
├── EventLog / replay / projections
├── NotificationRouter
└── Shared global services
    ├── provider registry
    ├── memory
    ├── plugin manager
    └── search backend
```

The initial implementation should remain one Rust process. This roadmap does not require subprocess workers, a distributed queue, an external broker, or microservices.

## 5. Core domain model

The exact module placement may evolve, but the following concepts must become explicit.

### Workspace identity

```rust
pub struct WorkspaceId(String);

pub struct WorkspaceRecord {
    pub workspace_id: WorkspaceId,
    pub canonical_root: PathBuf,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
}
```

Workspace identity must be derived from a canonical root and persisted in the daemon database. Path aliases and symlinks must not produce duplicate active workspace identities.

### Execution context

```rust
pub struct ExecutionContext {
    pub workspace_id: WorkspaceId,
    pub workspace_root: PathBuf,
    pub session_id: Option<String>,
    pub project_config: Arc<ResolvedProjectConfig>,
    pub run_store: Arc<dyn RunStore>,
    pub cancellation: CancellationToken,
    pub allowed_read_roots: Arc<[PathBuf]>,
    pub allowed_write_roots: Arc<[PathBuf]>,
}
```

This context is daemon-resolved and passed into turns, tools, subagents, tests, Python, Git, managed processes, and raw shell execution.

### Job lifecycle

```text
Scheduled -> Queued -> Admitted -> Running
                              -> Completed
                              -> Failed
                              -> Cancelled
                              -> TimedOut
                              -> Interrupted
```

A job and an execution attempt are separate. Retrying a job creates another attempt rather than mutating away the history of the previous one.

### Resource request

```rust
pub struct ResourceRequest {
    pub cpu_weight: u32,
    pub memory_mb_hint: u64,
    pub process_slots: u16,
    pub io_weight: u32,
    pub network_slots: u16,
    pub exclusivity_keys: Vec<ResourceKey>,
}
```

Initial resource accounting can be conservative and configuration-driven. It does not need live kernel telemetry to provide value.

## 6. Roadmap phases

### Phase 1 — Singleton lifecycle and transport convergence

Establish one user-scoped daemon as the production invariant. Add a process-lifetime lock, safe stale-socket recovery, daemon generation metadata, connect-or-start behavior, and socket-default frontend startup. Prevent `core-stdio`, HTTP server mode, and ordinary TUI startup from silently creating parallel production runtimes.

Detailed plan: `plans/single-daemon-phase-01-singleton-lifecycle-and-default-transport.md`.

### Phase 2 — Workspace registry and execution context

Introduce typed workspace identity, durable workspace registration, session-to-workspace validation, and immutable execution context propagation. Remove process-global cwd dependence from daemon-owned turn, tool, LSP, Git, TestRunner, Python, and subagent paths.

Detailed plan: `plans/single-daemon-phase-02-workspace-registry-and-execution-context.md`.

### Phase 3 — Daemon-owned workspace services and storage separation

Create `WorkspaceServices` and a daemon-owned registry keyed by `WorkspaceId`. Move RunStore, workspace config, LSP, Git, filesystem policy, and workspace-local locks behind this registry. Separate the user-scoped daemon catalog from workspace-local artifacts.

Detailed plan: `plans/single-daemon-phase-03-workspace-services-and-storage.md`.

### Phase 4 — Durable job and schedule model

Add typed job, attempt, schedule, dependency, lease, cancellation, retry, and recovery records. Replace ambiguous background-task identity and lifecycle semantics with durable job creation. Keep execution initially behind a compatibility executor while persistence and protocol are proven.

Detailed plan: `plans/single-daemon-phase-04-durable-jobs-and-schedules.md`.

### Phase 5 — Global admission controller and initial executor integration

Implement the fair queue, resource budget, exclusivity keys, queue bounds, and scheduler event model. Route TestRunner, build/lint/format managed processes, and subagents through central admission first. Preserve executor-specific behavior and RunStore ownership.

Detailed plan: `plans/single-daemon-phase-05-admission-control-and-initial-executors.md`.

### Phase 6 — Complete executor migration

Route agent turns, raw shell, Python, Git mutations, research, plugin-spawned work, and remaining managed processes through scheduler admission. Define which lightweight reads may bypass scheduling. Remove direct process-spawn paths from daemon-owned tools.

### Phase 7 — Scheduled and deferred orchestration

Replace `BackgroundScheduler` with `ScheduleService`. Support interval and calendar schedules, one-shot deferred jobs, missed-run policy, overlap policy, idempotency classification, pause/resume, and durable next-run calculation.

### Phase 8 — Daemon-authoritative turn submission

Reduce `TurnSubmit` to session identity, user text, and bounded overrides. Resolve message history, selected model, agents, project configuration, permissions, and execution context inside the daemon. Add optimistic session revision checks to reject stale client submissions.

### Phase 9 — Multi-client control and subscription model

Implement observe/control/exclusive-control leases, explicit takeover and expiry, authorization for submit/cancel/steer/permission responses, workspace subscriptions, all-session dashboard subscriptions, and frontend capability negotiation.

### Phase 10 — Recovery, observability, and operational controls

Add daemon generation leases, deterministic interrupted-job recovery, scheduler snapshots, queue diagnostics, resource utilization projections, executor health, structured logs, graceful drain, shutdown deadlines, and safe daemon upgrades.

### Phase 11 — Compatibility migration and deprecation

Migrate existing project-local sessions and tasks where feasible, retain explicit standalone mode, provide protocol feature negotiation, document changed startup behavior, and phase out deprecated stdio/in-process production flows without breaking tests or embeddings.

### Phase 12 — Closure validation

Run multi-process and multi-project contention tests, restart/recovery fault injection, path-isolation tests, duplicate-daemon races, no-double-execution sentinels, queue fairness tests, resource-cap tests, RunStore attribution checks, and long-duration soak tests.

## 7. Cross-cutting invariants

Every phase must preserve these invariants:

1. A session belongs to exactly one canonical workspace.
2. A daemon-owned executor never infers workspace identity from global cwd.
3. A logical execution produces at most one process execution and one canonical RunStore record.
4. Queue admission is distinct from executor invocation.
5. Cancellation is scoped by job/attempt/session and cannot affect unrelated workspaces.
6. Workspace mutation locks are acquired before mutation begins and released through RAII on every terminal path.
7. A daemon restart cannot leave an attempt indefinitely `running` without a valid generation lease.
8. Existing permission, sandbox, Git environment, preflight, redaction, and command-routing policies remain in force.
9. Frontend disconnect does not cancel daemon-owned work unless policy explicitly requests that behavior.
10. All scheduler and protocol collections are bounded.

## 8. Configuration direction

Add a daemon execution section with conservative defaults. Exact names should follow existing schema conventions.

```toml
[daemon]
mode = "single_user"
autostart = true

[daemon.resources]
max_processes = 4
max_cpu_weight = 8
max_memory_mb_hint = 8192
max_network_jobs = 4
max_queued_jobs = 256

[daemon.fairness]
policy = "weighted_round_robin"
interactive_weight = 8
scheduled_weight = 2
maintenance_weight = 1

[daemon.recovery]
running_job_policy = "interrupt"
requeue_idempotent = true
```

Configuration validation must reject impossible or unsafe combinations, such as zero queue capacity with autostarted scheduled work or negative-equivalent weights.

## 9. Testing strategy

Use the repository’s resource-capped test policy and run relevant Rust test suites with `--test-threads=1`.

Testing must include:

- unit tests for typed identities, state transitions, queue ordering, and resource permits;
- integration tests using temporary workspaces and a real Unix socket;
- multi-process singleton races;
- two or more concurrent TUI/client simulations;
- path and symlink canonicalization tests;
- daemon restart and stale lease recovery;
- marker-file sentinels proving no double execution;
- queue fairness under mixed interactive and scheduled load;
- resource cap enforcement under several Rust and Python projects;
- migration tests for existing sessions, tasks, and run artifacts;
- protocol compatibility and capability negotiation tests.

CI should avoid launching uncontrolled parallel compiler/test processes. The scheduler integration tests should use synthetic executors for most fairness/resource cases and reserve real Cargo/Pytest process tests for a bounded subset.

## 10. Rollout strategy

Use staged feature flags and protocol capabilities rather than one large switch:

1. land singleton locking while preserving explicit legacy modes;
2. introduce workspace identity and context in observe/assert mode;
3. create workspace services while retaining compatibility constructors;
4. persist jobs while executing synchronously through a compatibility path;
5. enable scheduler admission for tests/builds/subagents;
6. expand executor coverage family by family;
7. switch ordinary TUI startup to connect-or-start daemon mode;
8. deprecate production in-process and stdio cores after operational proof.

Each stage should expose diagnostics through `codegg daemon status` or an equivalent snapshot so regressions are visible before defaults change.

## 11. Explicit non-goals

This roadmap does not require:

- multiple daemons cooperating across machines;
- remote distributed builds;
- Kubernetes, systemd-run, launchd job delegation, or external queue services;
- hard real-time scheduling;
- perfect memory estimation before a job starts;
- containerizing every command;
- replacing SQLite solely to support scheduling;
- rewriting TestRunner, Python, Git, RunStore, or command routing;
- allowing several active turns in one session;
- exposing arbitrary daemon administration over an unauthenticated network interface.

## 12. Completion criteria

The roadmap is complete when one daemon can serve several projects and frontends while proving all of the following:

- a second production daemon cannot become active for the same user;
- every execution is attributed to the correct canonical workspace and session;
- no daemon-owned executor depends on process-global cwd;
- all heavy work passes through one admission controller;
- configured machine resource limits and workspace fairness are enforced;
- scheduled work survives frontend disconnect and daemon restart according to policy;
- cancellation and control are scoped and authorized;
- RunStore artifacts are workspace-correct and free of duplicate records;
- existing security, permission, routing, and sandbox invariants remain intact;
- operational status exposes active workspaces, queued/running jobs, resource usage, connected clients, and recovery state.
