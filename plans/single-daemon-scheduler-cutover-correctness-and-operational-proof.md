# Single-Daemon Scheduler Cutover, Correctness, and Operational Proof

## Status

Proposed targeted implementation pass following completion of the single-daemon roadmap Phases 1-5.

This plan does not redesign the daemon, workspace registry, workspace service registry, durable job store, or admission scheduler. Those foundations now exist. The purpose of this pass is to make the scheduler the actual production execution authority, remove remaining bypasses, consolidate subprocess execution policy, close lifecycle and recovery gaps, and prove that one daemon can safely coordinate several active projects on one constrained build system.

---

## 1. Current state

The repository now has the major architectural components required for machine-wide orchestration:

- a user-scoped singleton daemon with advisory locking and connect-or-start frontend behavior;
- typed workspace identity and immutable `ExecutionContext` propagation;
- daemon-owned workspace service bundles and a user-scoped authoritative database;
- durable jobs, attempts, schedules, dependencies, cancellation, retries, and daemon-generation recovery;
- a global admission scheduler with workspace lanes, priority classes, resource accounting, exclusivity keys, typed executors, snapshots, and scheduler events.

The remaining issue is authority rather than architecture.

The scheduler currently exists as an observe-stage subsystem while several production paths can still execute work through legacy or direct routes. The current architecture documentation explicitly stages the remaining cutover:

1. test-related tool and TUI paths submit through the scheduler;
2. subagent execution submits through the scheduler;
3. the scheduler becomes mandatory and legacy dispatchers are removed or feature-gated;
4. all daemon-owned process creation is guarded by durable job identity and an admission permit.

The current static bypass guard still carries migration exemptions for `src/tool/bash.rs`, `src/job_dispatcher.rs`, and background-task migration code. `ManagedArgvExecutor` also launches commands directly with `tokio::process::Command`, which risks creating a second subprocess policy separate from the Bash translation layer, TestRunner, Git execution services, and existing process hardening.

This pass must close those gaps without weakening the canonical execution subsystems that already exist.

---

## 2. Target invariant

At completion, the following invariant must be true:

> No daemon-owned operation capable of starting a process, consuming a build slot, dispatching an agent, mutating a workspace, or using a constrained shared resource may execute without first obtaining a durable job attempt and a scheduler-issued resource permit.

The invariant applies to:

- tests;
- builds;
- linting;
- formatting;
- structured Bash translation routes;
- raw shell commands launched by daemon-owned tools;
- Python execution;
- Git operations that spawn subprocesses;
- subagents;
- scheduled/background work;
- maintenance jobs;
- future plugins or executors that consume shared process or machine capacity.

The scheduler owns admission and lifecycle. It does not need to own every execution implementation. Canonical executors remain responsible for command semantics, environment policy, process-group control, cancellation, output handling, RunStore persistence, and domain-specific safety.

---

## 3. Goals

### 3.1 Authority goals

- Make `JobScheduler` the sole production entry point for daemon-owned heavy execution.
- Require every admitted execution to have a durable `JobRecord`, `JobAttempt`, daemon generation, workspace binding, and resource permit.
- Remove or hard-disable legacy execution bypasses after migration.
- Ensure scheduler-disabled production mode cannot silently execute through legacy paths.
- Preserve explicit standalone modes for tests and embedding, while clearly excluding them from machine-wide orchestration guarantees.

### 3.2 Execution-policy goals

- Replace direct process creation in `ManagedArgvExecutor` with a canonical managed-process service.
- Reuse existing subprocess hardening, environment sanitization, cancellation, process-group cleanup, timeout, output bounding, and provenance behavior.
- Keep domain-specific services authoritative:
  - TestRunner for tests;
  - Git execution services for Git;
  - Bash translation/raw-shell service for shell commands;
  - Python subsystem for Python execution;
  - SubAgentPool for agent execution.
- Prevent duplicate execution when scheduler dispatch or executor startup fails.

### 3.3 Scheduling goals

- Enforce global, per-workspace, and per-job-class concurrency limits in active production mode.
- Add conservative resource profiles for common job kinds.
- Use exclusivity keys to prevent unsafe concurrent mutation of shared workspace and build artifacts.
- Preserve interactive responsiveness without starving normal or background jobs.
- Define overload, impossible-request, cancellation, and shutdown semantics.

### 3.4 Recovery and proof goals

- Prove restart recovery does not lose, duplicate, or incorrectly resume attempts.
- Prove two or more workspaces can submit overlapping work without exceeding configured process capacity.
- Prove one workspace cannot monopolize the machine indefinitely.
- Prove cancellation releases permits and terminates process trees.
- Prove all production paths emit durable lifecycle and RunStore evidence.

---

## 4. Non-goals

This pass does not:

- introduce distributed scheduling across multiple machines;
- add Kubernetes, cgroups, systemd scopes, containers, or platform-specific sandboxing;
- guarantee hard CPU, memory, or IO enforcement;
- redesign workspace registration or session persistence;
- redesign the TUI;
- replace TestRunner, Git execution, Bash routing, or the Python subsystem;
- add arbitrary workflow-DAG authoring beyond the existing durable dependency model;
- make standalone in-process mode participate in the user-scoped daemon scheduler;
- change agent prompting or model-selection policy except where job metadata must be made daemon-authoritative;
- add remote multi-user tenancy.

---

## 5. Required architecture

The production execution path should converge on:

```text
Frontend / schedule / agent tool request
    -> CoreDaemon validates session and workspace
    -> JobSubmissionService creates durable JobRecord
    -> JobScheduler queues and admits the job
    -> JobStore creates JobAttempt
    -> AdmissionController reserves ResourcePermitGuard
    -> ExecutorRegistry selects typed executor
    -> typed executor delegates to canonical domain service
    -> progress / cancellation / heartbeat flow through scheduler
    -> executor returns structured completion
    -> scheduler persists terminal attempt and job state
    -> permit is released exactly once
    -> events and snapshots expose bounded state changes
```

There must be no alternate production path from a tool or TUI command directly to process creation.

---

# Track A — Inventory and submission boundary

## A1. Create an authoritative execution-surface inventory

Enumerate every daemon-owned path that can:

- call `tokio::process::Command`, `std::process::Command`, or a process wrapper;
- invoke TestRunner;
- dispatch a subagent;
- invoke Python execution;
- invoke Git subprocess execution;
- invoke raw shell execution;
- start scheduled/background work;
- construct an executor directly.

At minimum inspect:

- `src/tool/bash.rs`;
- `src/tool/test.rs`;
- `src/tool/python.rs` and Python subsystem modules;
- `src/tool/git.rs` and Git mutation/read services;
- `src/tui/commands/` test/build-related handlers;
- `src/agent/worker.rs`;
- `src/job_dispatcher.rs`;
- `src/background_task_migration.rs`;
- `src/test_runner/`;
- `src/scheduler/executors.rs`;
- plugin/tool execution adapters;
- HTTP/server request handlers that can trigger work.

Produce a table in `architecture/scheduler.md` or a dedicated migration document:

| Production caller | Current backend | Target `JobKind` | Canonical executor/service | Resource profile | Exclusivity keys | Migration status |
|---|---|---|---|---|---|---|

### Acceptance criteria

- Every production process/agent launch site is classified.
- Every site has one target scheduler submission path.
- Unclassified process creation fails the static guard.
- Test fixtures and explicitly standalone-only code are separated from daemon production paths.

## A2. Introduce `JobSubmissionService`

Do not let every tool manually construct persisted jobs and scheduler queue entries.

Add a daemon-owned submission facade, for example:

```rust
pub struct JobSubmissionService {
    job_store: Arc<dyn JobStore>,
    scheduler: Arc<JobScheduler>,
    workspace_services: Arc<WorkspaceServiceRegistry>,
    daemon_generation: DaemonGeneration,
}
```

The service should:

1. validate workspace and session binding;
2. validate `JobSpec` and job-kind-specific payload;
3. resolve resource profile and exclusivity keys;
4. create the durable job exactly once;
5. enqueue it exactly once;
6. wake the scheduler;
7. return typed submission metadata.

Recommended result:

```rust
pub struct SubmittedJob {
    pub job_id: JobId,
    pub state: JobState,
    pub workspace_id: WorkspaceId,
    pub priority: JobPriority,
}
```

Idempotency support should be included for callers that may retry after transport uncertainty:

```rust
pub struct SubmissionKey(String);
```

A repeated valid submission key for the same canonical request must return the existing job rather than create duplicate work.

### Acceptance criteria

- Tools and protocol handlers no longer write directly to `JobStore` and then separately call `scheduler.submit()`.
- Durable creation and scheduler enqueue are one logical operation with explicit failure semantics.
- A client retry cannot create duplicate build/test execution.

---

# Track B — Canonical managed-process execution

## B1. Extract a shared managed-process service

`ManagedArgvExecutor` must not remain a separate raw `tokio::process::Command` implementation.

Create or promote a canonical service used by scheduled non-shell argv execution. The exact module can follow existing repository boundaries, but it should provide a typed API similar to:

```rust
pub struct ManagedProcessRequest {
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment_policy: EnvironmentPolicy,
    pub timeout: Option<Duration>,
    pub cancellation: CancellationToken,
    pub output_policy: OutputPolicy,
    pub run_metadata: RunMetadata,
}

pub struct ManagedProcessResult {
    pub exit_status: ExitStatus,
    pub stdout: BoundedOutput,
    pub stderr: BoundedOutput,
    pub duration: Duration,
    pub termination: TerminationReason,
    pub run_id: Option<RunId>,
}
```

The service must centralize:

- process-group/session creation;
- cancellation and timeout termination;
- descendant cleanup;
- stdin policy;
- noninteractive environment defaults;
- environment allow/deny policy;
- secret redaction;
- bounded output collection;
- exit classification;
- RunStore linkage;
- audit-safe argv persistence;
- working-directory validation through `ExecutionContext`.

Reuse an existing canonical process abstraction where possible. Do not duplicate TestRunner or Git-specific behavior.

## B2. Make `ManagedArgvExecutor` a scheduler adapter

After extraction, `ManagedArgvExecutor` should only:

- validate supported job kinds;
- resolve the workspace service handle;
- construct a `ManagedProcessRequest` from the durable job payload;
- delegate to the canonical process service;
- convert the result to `ExecutorCompletion`.

It must not directly create a child process.

### Acceptance criteria

- No direct `tokio::process::Command::new` remains in `src/scheduler/executors.rs`.
- Managed build/lint/format jobs use the same environment, cancellation, process-tree cleanup, output bounding, and provenance rules as other managed execution.
- Cancellation before spawn and cancellation after spawn have deterministic terminal states.
- Executor failure never triggers a raw-shell fallback.

---

# Track C — Test and command cutover

## C1. Route TestTool through durable scheduler submission

Change `src/tool/test.rs` so daemon-owned test requests:

1. build a typed `JobSpec::Test`;
2. submit through `JobSubmissionService`;
3. await or stream the durable job result according to current tool semantics;
4. return the canonical TestRunner report projection.

Preserve:

- framework detection;
- test selection semantics;
- TestRunner timeouts and stall handling;
- RunStore records and artifacts;
- process-group cleanup;
- current user-facing output shape where practical.

Do not create a second test result schema inside the scheduler.

## C2. Route Bash test translation through the same path

`dispatch_to_test_runner` should become a scheduler-submission adapter, not a direct TestRunner call.

The Bash translation layer should preserve:

- typed command classification;
- exact argv/workdir;
- routing provenance;
- planned and actual backend metadata;
- no-double-execution behavior.

If scheduler submission is rejected, blocked, cancelled, or impossible, BashTool must return that state. It must not execute the original command through raw shell.

## C3. Route TUI and server test commands through the daemon

TUI and HTTP/server entry points must send a daemon request or invoke the daemon submission service. They must not instantiate TestRunner or scheduler executors locally.

Where existing UI behavior assumes synchronous completion, add a thin wait/subscribe layer over durable job events rather than bypassing the scheduler.

## C4. Migrate build, lint, and format command paths

Identify structured build/lint/format routes and submit `JobKind::Build`, `JobKind::Lint`, or `JobKind::Format` using canonical argv payloads.

Recommended default exclusivity rules:

- build: shared workspace read plus exclusive build-artifact key where tools share a target/output directory;
- lint: shared unless the command writes caches or generated files;
- format check: shared;
- format write: exclusive workspace mutation key.

For Rust, derive stable keys from canonical target directory when available:

```text
exclusive:build-target:<canonical-target-dir-hash>
```

This prevents two workspaces or sessions pointing at the same target directory from overloading or corrupting shared build state.

### Acceptance criteria

- TestTool, Bash-translated tests, TUI tests, and server-triggered tests all produce durable jobs and attempts.
- No direct TestRunner invocation remains outside the scheduler executor and test-only fixtures.
- Build/lint/format structured routes no longer launch directly.
- Scheduler snapshots accurately report queued/running work by workspace and job kind.

---

# Track D — Subagent and background cutover

## D1. Route subagent dispatch through the scheduler

Replace direct production use of `SubAgentJobDispatcher` with `JobKind::Subagent` submission.

The durable subagent payload should include authoritative references rather than client-supplied duplicated state:

- parent session ID;
- parent turn ID where applicable;
- workspace ID;
- agent definition identifier;
- selected model identifier or daemon-resolved policy reference;
- task text;
- allowed path policy derived from `ExecutionContext`;
- priority and cancellation relationship.

The scheduler executor should delegate to `SubAgentPool`, preserving its internal worker mechanism where useful. The SubAgentPool semaphore may remain as a defensive local cap initially, but scheduler admission must be the outer authority.

Avoid double-throttling ambiguity by documenting which cap is authoritative and how the pool cap relates to scheduler `process_slots` or subagent class limits.

## D2. Bind subagent cancellation to durable attempts

Required behavior:

- cancelling a queued subagent job prevents dispatch;
- cancelling a running attempt signals the subagent cancellation token;
- cancellation propagates to nested tools/processes;
- the attempt reaches one terminal state;
- permits release exactly once;
- parent-turn cancellation can cancel child jobs according to explicit policy.

## D3. Replace legacy background scheduler dispatch

Schedules should create normal durable jobs through `JobSubmissionService`.

Remove direct immediate subagent dispatch from schedule creation. A schedule firing should:

1. evaluate due time;
2. create or reuse a unique occurrence key;
3. submit a normal durable job;
4. record the occurrence/job relation;
5. advance the schedule only according to defined misfire policy.

Define misfire behavior explicitly:

- skip;
- run once immediately;
- catch up to bounded count;
- coalesce missed occurrences.

The default should be conservative coalescing rather than unbounded catch-up after a long daemon outage.

### Acceptance criteria

- Production subagent dispatch always has a durable job and scheduler permit.
- Schedule creation never dispatches work directly.
- Daemon restart cannot duplicate a schedule occurrence.
- Legacy `SubAgentJobDispatcher` is removed from production wiring or retained only behind an explicit standalone/test feature.

---

# Track E — Mandatory scheduler mode and bypass removal

## E1. Define production behavior when scheduling is disabled

Once Tracks C and D land, production daemon mode must not fall back to unscheduled execution.

Choose one of these explicit models:

1. scheduler always enabled in production daemon mode; configuration only tunes policy; or
2. scheduler may be disabled, but heavy job submission returns a clear `SchedulerDisabled` error.

Do not retain a mode where disabling the scheduler restores direct execution.

Standalone in-process and stdio test modes may use dedicated harness executors, but their limitations must be explicit and they must not be reachable through ordinary TUI startup.

## E2. Tighten the static guard

Expand `scripts/check_scheduler_bypass.py` to detect:

- direct TestRunner calls outside `src/scheduler/**`;
- direct `SubAgentPool`/dispatcher production sends outside the scheduler adapter;
- direct process construction in daemon-owned tool, TUI, server, agent, and background paths;
- direct construction/invocation of scheduler executors outside `JobScheduler`;
- direct durable-job writes followed by execution outside `JobSubmissionService`.

Remove migration exemptions as their callers are migrated:

- `src/tool/bash.rs`;
- `src/job_dispatcher.rs`;
- `src/background_task_migration.rs`.

Any remaining exemptions must include a source comment and plan reference explaining why they are safe and when they will be removed.

## E3. Add runtime assertions and provenance

Static guards are insufficient. Add runtime checks:

- executor context must contain a valid `AttemptId`;
- executor context must own a live permit guard;
- workspace-bound jobs must resolve a workspace service handle;
- daemon generation must match the active scheduler generation;
- a terminal attempt cannot be executed again;
- process launch provenance records job ID and attempt ID.

Where practical, add debug assertions plus release-mode typed errors.

### Acceptance criteria

- The scheduler is the only production dispatch authority.
- All temporary bypass allowlists are removed or reduced to explicit test/standalone code.
- Disabling scheduling cannot cause unscheduled production execution.
- Every managed process and subagent can be traced to one durable attempt.

---

# Track F — Resource policy and exclusivity correctness

## F1. Add conservative default resource profiles

Define defaults centrally by `JobKind`, with configuration overrides.

Example starting profiles:

| Job kind | CPU weight | Memory hint | Process slots | IO weight | Network slots |
|---|---:|---:|---:|---:|---:|
| Test | 2 | 1024 MB | 1 | 2 | 0 |
| Build | 3 | 2048 MB | 1 | 3 | 0 |
| Lint | 1 | 768 MB | 1 | 1 | 0 |
| Format check | 1 | 256 MB | 1 | 1 | 0 |
| Format write | 1 | 256 MB | 1 | 2 | 0 |
| Subagent | 1 | 512 MB | 0 or policy-defined | 1 | policy-defined |
| Git network | 1 | 256 MB | 1 | 1 | 1 |
| Maintenance | 1 | 256 MB | 1 | 1 | 0 |

These are admission hints, not hard OS limits. Document that distinction.

Allow project configuration to refine profiles, but validate values against global maxima and prevent a workspace from declaring zero-cost heavy jobs.

## F2. Centralize exclusivity key derivation

Add typed helpers rather than hand-built strings:

```rust
pub enum ExclusivityScope {
    WorkspaceMutation(WorkspaceId),
    BuildArtifact(PathFingerprint),
    GitWorktree(PathFingerprint),
    Database(PathFingerprint),
    UserDefined(String),
}
```

Canonicalize and hash paths before key creation so aliases resolve to the same key without leaking full paths into metrics.

Required initial rules:

- Git worktree mutation conflicts within the same worktree;
- format-write conflicts with other workspace mutations;
- shared Cargo target directories conflict for build-like writers;
- workspace-local read-only jobs may run concurrently when resource budgets permit;
- background maintenance yields to interactive work but eventually runs.

## F3. Add pressure feedback without pretending it is hard enforcement

Optional but recommended in this pass:

- collect observed duration and peak process-tree RSS where portable;
- store bounded observations by job kind/toolchain/workspace;
- expose machine-pressure snapshots;
- suspend new background admission when host memory pressure crosses a configurable threshold;
- never kill arbitrary external processes.

Keep this as adaptive admission input, not a platform-specific resource-control framework.

### Acceptance criteria

- Resource profiles are deterministic and inspectable.
- Impossible resource requests fail before queueing or enter an explicit unschedulable state.
- Shared target/worktree conflicts are blocked by typed exclusivity rules.
- One workspace cannot bypass global caps through configuration.

---

# Track G — Lifecycle, cancellation, and recovery

## G1. Formalize job/attempt state transitions

Document and enforce a transition table. At minimum:

```text
Created -> Queued -> Admitted -> Running -> Succeeded
                                      \-> Failed
                                      \-> Cancelled
                                      \-> TimedOut
                                      \-> Interrupted
Queued -> Cancelled
Queued -> Blocked -> Queued
Failed/Interrupted -> Queued (bounded retry only)
```

Invalid transitions must return typed errors and leave persisted state unchanged.

## G2. Close spawn-boundary race conditions

Test and handle cancellation at each boundary:

- before durable job creation;
- after creation but before queue insertion;
- queued but not admitted;
- admitted but before executor spawn;
- child spawned but before running heartbeat;
- running;
- executor completion racing cancellation;
- daemon shutdown.

The implementation must guarantee:

- zero or one child/process execution;
- one terminal attempt state;
- one permit release;
- no fallback execution;
- no orphan process tree.

## G3. Validate daemon-generation recovery

On startup:

- recover attempts owned by the previous daemon generation;
- classify them as interrupted unless an executor has an explicit safe resume contract;
- release reconstructed resource accounting;
- apply retry policy once;
- avoid duplicate schedule occurrences;
- rebuild the ready queue from durable state;
- emit a bounded recovery summary event.

Do not attempt to attach to arbitrary surviving process trees in this pass unless the repository already has a verified process identity mechanism.

### Acceptance criteria

- Kill/restart tests prove jobs are not duplicated.
- Interrupted jobs follow bounded retry policy.
- Scheduler resource usage returns to zero after recovery.
- Schedule occurrences remain unique across restart.

---

# Track H — Protocol, snapshots, and operator visibility

## H1. Add daemon-authoritative job protocol operations

Expose only the minimum required client surface:

- submit a typed job request or invoke a higher-level operation that the daemon converts to a job;
- list jobs with bounded filters;
- inspect one job and attempts;
- cancel a job/attempt;
- subscribe to job/scheduler events;
- request scheduler snapshot.

Clients must not supply authoritative daemon generation, attempt IDs, resolved workspace paths, resource permits, or executor implementation details.

## H2. Extend `SnapshotDaemon`

Include bounded scheduler state:

- queued/running counts;
- per-workspace counts;
- resource usage and configured caps;
- blocked/unschedulable summaries;
- active attempts;
- oldest queued age by priority;
- scheduler enabled/mandatory mode;
- recent recovery summary.

Avoid embedding unbounded job history in the daemon snapshot.

## H3. Preserve event-log boundedness

Scheduler events should report deltas and identifiers. Full job/attempt details should be fetched through request/response APIs.

Add rate limiting or coalescing for repeated admission-blocked and queue-reconciled events.

### Acceptance criteria

- TUI/server can display scheduler state without direct access to scheduler internals.
- Snapshot size remains bounded under thousands of historical jobs.
- Client reconnect can replay meaningful lifecycle events without flooding.

---

# Track I — Verification and operational proof

## I1. Unit and component tests

Required coverage:

- `JobSubmissionService` idempotency;
- durable create/enqueue failure boundaries;
- executor registration and dispatch;
- permit ownership and release;
- resource-profile validation;
- exclusivity-key canonicalization;
- invalid lifecycle transitions;
- cancellation races;
- scheduler-disabled production behavior;
- static guard fixtures.

## I2. Two-workspace contention integration test

Create two temporary Rust workspaces with independent sessions and one singleton daemon.

Submit overlapping work:

- workspace A: long test and build;
- workspace B: short interactive test and lint;
- one background maintenance job;
- one conflicting mutation job.

Assert:

- global process cap is never exceeded;
- workspace B receives service before workspace A drains its queue;
- interactive work receives the configured priority advantage;
- background work eventually runs;
- conflicting exclusivity keys never overlap;
- each operation has one durable attempt and one terminal state.

## I3. Shared build-target test

Configure two workspaces to use the same Cargo target directory.

Assert:

- build-like writers derive the same exclusivity key;
- they do not run concurrently;
- read-only work that does not conflict may still proceed;
- alias/symlink paths do not produce different keys.

## I4. Restart and recovery test

Start several jobs, force daemon termination after selected lifecycle points, restart, and verify:

- singleton ownership transfers only after lock release;
- old-generation attempts become interrupted;
- retry policy is applied once;
- no duplicate process markers appear;
- schedule occurrence keys remain unique;
- scheduler resource snapshot returns to a valid state.

## I5. Cancellation and process-tree test

Launch a test/build fixture that creates descendants.

Cancel through the daemon and assert:

- root and descendants terminate;
- terminal state is cancelled or timed out according to initiating cause;
- permit counts return to baseline;
- RunStore captures termination reason;
- no second execution occurs.

## I6. Full regression and static verification

Run at minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
```

Also run repository-specific nextest profiles and constrained-build tests where available.

Record exact commands, platform, test counts, skipped tests, and observed peak concurrency in the implementation commit or handoff report.

### Acceptance criteria

- All new targeted tests pass.
- Existing suite remains green.
- Static guards have no production migration exemptions.
- Operational tests prove the configured process cap and fairness behavior.

---

## 6. Recommended implementation order

Implement in this order to minimize split authority:

1. **A — inventory and `JobSubmissionService`**
2. **B — canonical managed-process service**
3. **C — tests/build/lint/format cutover**
4. **D — subagent and schedule cutover**
5. **E — mandatory mode and bypass removal**
6. **F — resource profiles and typed exclusivity keys**
7. **G — lifecycle/recovery tightening**
8. **H — protocol and visibility**
9. **I — operational proof and full regression**

Do not make the scheduler mandatory before the migrated executors preserve cancellation, environment policy, output handling, and RunStore behavior. Conversely, do not leave dual production execution paths after each category is migrated.

---

## 7. Compatibility and rollout

### Stage 1: observe comparison

- construct durable job specs alongside current execution;
- compare resource profile, workspace, priority, and executor selection;
- do not duplicate execution;
- emit diagnostic mismatches.

### Stage 2: category-by-category active routing

Enable active scheduler routing independently for:

1. TestTool;
2. Bash-translated tests;
3. TUI/server test commands;
4. build/lint/format;
5. subagents;
6. schedules/background tasks;
7. remaining managed processes.

Each category must remove its old production route when activated.

### Stage 3: mandatory production authority

- scheduler always active in daemon-client mode;
- legacy dispatcher unavailable in production builds or behind explicit development feature;
- static guard exemptions removed;
- scheduler-disabled submission returns an error rather than bypassing.

### Stage 4: remove temporary compatibility code

After one validation cycle:

- remove observe-only comparison fields;
- delete unused legacy dispatcher types;
- collapse transitional configuration flags;
- update architecture documentation to state the scheduler invariant as current behavior rather than future rollout.

---

## 8. Security and correctness requirements

- Workspace paths must come from `ExecutionContext`, never process current directory.
- Clients cannot choose arbitrary canonical roots for an existing session.
- Scheduler admission denial cannot fall back to direct execution.
- Executor startup failure cannot trigger duplicate execution.
- Environment sanitization remains canonical across scheduled and direct standalone test paths.
- Secrets must not be persisted in job payloads, argv, events, snapshots, or RunStore records.
- Cancellation authorization must respect existing session/client control policy.
- Job payload deserialization must be bounded and validated before persistence.
- User-defined exclusivity keys must be namespaced and length-bounded.
- Scheduler events and snapshots must not leak full filesystem paths unless existing diagnostics policy explicitly permits them.

---

## 9. Completion checklist

The pass is complete only when all are true:

- [ ] Every daemon-owned process/agent launch surface is inventoried.
- [ ] `JobSubmissionService` is the sole durable submission path.
- [ ] TestTool submits scheduler jobs.
- [ ] Bash test translation submits scheduler jobs.
- [ ] TUI/server test commands submit through the daemon.
- [ ] Build/lint/format structured commands submit scheduler jobs.
- [ ] Subagents submit scheduler jobs.
- [ ] Scheduled/background work creates normal durable jobs.
- [ ] `ManagedArgvExecutor` delegates to canonical managed-process execution.
- [ ] Scheduler-disabled production mode cannot bypass admission.
- [ ] Legacy dispatcher production wiring is removed.
- [ ] Static guard migration exemptions are removed.
- [ ] Every execution has job ID, attempt ID, workspace ID, daemon generation, and permit provenance.
- [ ] Resource profiles and exclusivity keys are centralized and tested.
- [ ] Cancellation releases permits and kills process trees.
- [ ] Restart recovery produces no duplicate execution.
- [ ] Two-workspace contention test proves fairness and global caps.
- [ ] Shared-target test proves exclusivity correctness.
- [ ] Architecture and operator documentation reflect actual mandatory behavior.
- [ ] Full workspace tests, clippy, formatting, and static guards pass.

---

## 10. Final handoff outcome

After this pass, Codegg should no longer be described as a daemon that contains a scheduler. It should be a scheduler-owned execution daemon:

- frontends are clients;
- sessions are bound to typed workspaces;
- work becomes durable jobs;
- the scheduler is the only admission authority;
- executors delegate to canonical domain services;
- resource use is bounded and fair across projects;
- cancellation and restart behavior are durable and observable;
- one build machine can service several active repositories without independent TUI instances overcommitting it.
