# ADR-0005: Project Work Orders, Task Orchestration, and Workspace Dashboard

Status: accepted

Date: 2026-09-16

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

External design input:

- OpenAI Codex app: multi-agent command-center model, worktree-isolated parallel work, and scheduled Automations.
- Cursor Agents/Worktrees: one isolated checkout per concurrent task and explicit foreground/background handoff.
- Cursor Automations: schedule- and webhook-triggered agent work, reinforcing separation between trigger intent and the executing agent session.

## Context

CodeGG already has the major lower-level components required for project-level task orchestration:

- a daemon-owned project catalog with stable project/workspace/session identity;
- durable sessions and frontend-neutral session projections;
- one global durable scheduler with jobs, attempts, schedules, cancellation, retry, dependencies, restart recovery, and a mandatory `JobSubmissionService` boundary;
- managed worktree lifecycle and leases for mutation-capable concurrent agent work;
- team principals, project authorization, immutable origin attribution, audit, presence, observation, and project collaboration;
- durable runtime preferences for selected model, approval mode, and sandbox profile;
- `WorkPlan`/`WorkItem` state for long-horizon work *inside* one session;
- an existing TUI project picker, project tabs, per-project session summaries, stale-completion guards, and Vim-oriented navigation;
- a legacy-named TUI `/tasks` surface that is currently only a thin projection over durable `Schedule*` operations.

The missing layer is one level above a session. A developer or authorized team member needs to be able to describe work now, order or schedule it for later, run several independent sessions concurrently in isolated workspaces, inspect all current/past/future work by project, and let an agent create future project work when explicitly authorized.

Treating this directly as a `Schedule`, `Job`, `AgentTask`, `WorkPlan`, or not-yet-started `Session` would conflate existing ownership boundaries:

- a `Schedule` is a durable rule that materializes jobs and has no full project-task interaction contract;
- a `Job` owns scheduler lifecycle/admission, not human-facing task intent;
- an `AgentTask` is delegated child intent beneath an agent run;
- a `WorkPlan` records detailed work remaining inside one session and explicitly is not a workflow/scheduling engine;
- a `Session` is a real durable conversation and should not be fabricated before execution begins.

The requested TUI label “Task” is useful, but CodeGG's internal domain needs a distinct unambiguous name.

## Decision drivers

- Preserve exactly one global scheduler/admission authority.
- Preserve `Workspace` as a concrete checkout, not a synonym for project/dashboard/task.
- Make future work durable without creating fake empty sessions.
- Let running task sessions behave exactly like ordinary sessions: steering, stop/cancel, permission responses, observation, model interaction, and history remain canonical session behavior.
- Permit several tasks in one project to execute concurrently without write collisions by reusing managed worktrees.
- Support editable sequential ordering without rewiring scheduler job dependencies.
- Support time, delay, repeat, sequence, and external-trigger conditions without becoming a general workflow/BPM engine.
- Keep task creation and mutation project-authorized and auditable for team use.
- Make external scripting triggers narrowly scoped and safe against accidental GET/prefetch activation.
- Let agents create future work without giving them unbounded persistent self-replication authority.
- Preserve existing `TaskTool` delegated-agent semantics rather than overloading it with project-level future sessions.

## Considered options

### Option A — Extend `ScheduleRecord` until it is the user-facing Task domain

Benefits:

- reuses current tables and protocol;
- small initial schema change.

Costs and failure modes:

- schedule records are workspace/session/job-template oriented rather than project-task oriented;
- interactive ordering, creator attribution, materialized session identity, attention state, team authorization, worktree policy, and external release gates become awkward labels/JSON extensions;
- `ScheduleList/Get/Delete` are currently opaque authorization surfaces for team principals;
- risks turning the low-level schedule primitive into a second workflow/domain model.

Rejected as the canonical user-facing model. Existing schedules remain useful infrastructure and compatibility behavior.

### Option B — Create future `Session` rows immediately and put schedule metadata on sessions

Benefits:

- the TUI can display “future sessions” using existing session lists.

Costs and failure modes:

- sessions would exist without a real conversation, turn, workspace execution snapshot, or provider lifecycle;
- session lifecycle queries and asset/model state become misleading;
- cancellation/repeat semantics do not naturally fit one session record;
- repeated work needs several executions but would be forced into one identity or speculative session proliferation.

Rejected.

### Option C — Add a durable project-scoped `WorkOrder` above sessions; occurrences materialize ordinary sessions/jobs

A `WorkOrder` records human/agent-authored project intent and release conditions. A `WorkOrderOccurrence` records one execution instance. When an occurrence becomes ready, a daemon coordinator claims it, allocates/resolves an execution workspace, creates a normal session, and submits the initial `AgentTurn` through the existing scheduler submission boundary.

Selected.

## Decision

### 1. Domain separation

CodeGG adds a durable project-scoped `WorkOrder` domain. The TUI may label WorkOrders as “Tasks,” but protocol/storage/architecture use `WorkOrder` to avoid collision with `AgentTask`, `TaskTool`, Todo items, WorkPlan items, TUI background tasks, and scheduler jobs.

Canonical ownership is:

```text
WorkOrder decides when/where a normal session may be born.
Session owns human/agent interaction once materialized.
Job/Scheduler owns admission and execution lifecycle.
Worktree/Workspace owns filesystem isolation and execution root.
WorkPlan owns detailed within-session completion state.
AgentTask owns delegated child intent below an AgentRun.
```

No layer may become a second owner of another layer's state machine.

### 2. WorkOrder and occurrence identity

At minimum the durable model contains:

- `WorkOrderId` — stable project-task identity;
- `WorkOrderOccurrenceId` — one materialization/execution of a WorkOrder;
- `SequenceLaneId` — ordered project lane used for sequential release;
- optional `TaskTriggerId` — public locator for a narrowly scoped external release trigger.

A WorkOrder contains bounded prompt/title metadata, project, creator/origin attribution, optional parent session/turn/work-order lineage, selected model/connection request, requested approval/sandbox snapshot, workspace policy, release-gate specification, sequence lane/order, repeat policy, lifecycle state, revision, and timestamps.

An occurrence contains occurrence number, gate/latch state, scheduled/released timestamps, materialized session/job/workspace/worktree references, terminal/attention status, and bounded diagnostics.

The original prompt/provenance for a materialized occurrence is immutable. Waiting WorkOrders may be edited through revision/CAS before claim.

### 3. No fake future sessions

Waiting/future WorkOrders are projected in the UI as future work but are not inserted into `SessionStore` until a WorkOrder occurrence is actually claimed for execution.

Materialization creates a normal canonical session. From that point, steering, cancellation, permission/question response, observation, chat references, provider/model behavior, continuation, and WorkPlan behavior use existing session/runtime paths.

### 4. Release gates, not a general workflow language

The initial closed gate set is:

- `Immediate` / zero-delay eligibility;
- `Delay(duration)`;
- `NotBefore(timestamp)`;
- `SequenceReady(lane)`;
- `ExternalTrigger(trigger_id)`.

Enabled gates are combined by one explicit `All` or `Any` join policy. Satisfied gates latch for the current occurrence so repeated polling or duplicate trigger delivery cannot create duplicate executions.

The default WorkOrder has only zero delay/immediate eligibility and therefore behaves like a normal newly started session.

Arbitrary expressions, shell predicates, dataflow variables, loops, branch graphs, cron-language embedding, and generic workflow scripting are out of scope.

### 5. Repeat semantics

Repeat is occurrence creation, not replaying one session. `repeat_count` is finite and bounded. Each occurrence receives a distinct occurrence/session/job identity. Delay for later occurrences may anchor to the prior occurrence's terminal timestamp according to the explicit repeat policy.

Unbounded repeat is not part of the initial contract.

### 6. Sequential ordering

Sequential task order is owned by a revisioned `SequenceLane`, not by mutable scheduler `job_dependency` edges.

Waiting WorkOrders have stable lane order. Reordering through `j/k` or Up/Down uses expected lane revision/CAS. Running/claimed work is pinned; reorder affects future work only.

`SequenceReady` is satisfied when no earlier non-terminal lane member remains under the lane's continuation policy.

Default failure behavior is conservative: failed/cancelled/needs-attention predecessors hold the lane until an authorized actor retries, skips/cancels, or explicitly advances according to policy. A failed task must not silently unleash the rest of a long implementation queue.

### 7. Coordinator and scheduler ownership

A daemon-owned `WorkOrderCoordinator` evaluates release conditions and claims ready occurrences. It never executes agent work directly.

When ready it:

1. atomically claims one occurrence/idempotency key;
2. resolves project/workspace/repository execution context;
3. acquires managed isolation when required;
4. creates one canonical session with project/workspace binding and origin attribution;
5. submits the initial `AgentTurn` through `JobSubmissionService`/the existing global scheduler;
6. stores canonical session/job/workspace/worktree references on the occurrence.

Timer expiry, trigger fire, predecessor transition, reorder/update, and restart reconciliation wake the same daemon scheduling/reconciliation apparatus. There is no second scheduler loop.

### 8. Parallel workspace isolation

Task-mode mutation-capable sessions default to an `AutoIsolated` workspace policy.

For a local Git repository, materialization acquires a managed worktree using the existing `WorktreeService`; allocation occurs at claim/start time, not when future work is authored. Dirty/conflicted/attention semantics remain owned by the managed-worktree service.

Read-only work may share when policy permits. Non-Git workspaces must not be falsely described as isolated; the initial implementation either serializes mutation-capable task sessions on the shared workspace or requires explicit unsafe shared execution until a real copy/overlay backend exists.

### 9. Model and execution-policy snapshots

The Task composer remembers a principal-scoped `task.last_model`-style preference through the existing daemon-owned runtime preference service. It is a convenience default only.

Each WorkOrder snapshots the requested stable model/connection identity at creation. At execution it is re-resolved against the current catalog; unavailable/removed models produce `NeedsAttention` rather than silent model substitution.

Requested `ApprovalMode` and `SandboxProfile` are also snapshotted for the WorkOrder and re-resolved/narrowed against current project/administrator/parent authority at execution. Later preference changes do not silently widen existing scheduled work; later policy tightening may narrow or block it.

### 10. Project Task view and global Workspace dashboard

The TUI adds a composer-level `Task` mode distinct from Vim `InputMode`. Enter from Task mode opens a scheduling dialog/sheet; confirming creates the WorkOrder. With default zero delay, it starts immediately.

The project Task view shows bounded running, waiting/future, needs-attention, and recent WorkOrders/sessions with Vim-like navigation and reorder controls.

A global user-facing “Workspace” dashboard is reachable through a hotkey and `/workspace`. The product label is allowed, but internally this view is a project/dashboard projection and MUST NOT redefine canonical `Workspace` identity. It shows authorized projects, active/future work counts, coarse task status, and pending permission/question/attention signals. Details are lazy-loaded.

The existing project picker/tab/view-switch infrastructure should be evolved/reused rather than creating a second project-navigation authority.

### 11. Team authorization and audit

All WorkOrder APIs are project-scoped from their first version. Create/list/get/update/reorder/cancel/trigger operations resolve authorization at the daemon boundary and record immutable origin/actor attribution.

Unauthorized project enumeration retains existing privacy behavior. A WorkOrder ID does not grant project visibility or mutation authority.

Materialized sessions inherit normal project/session authorization. Creating a WorkOrder does not grant the creator or future agent authority unavailable at execution time.

### 12. External trigger endpoint

External scripting uses a narrowly scoped trigger capability, not a side-effecting unauthenticated GET.

The preferred contract is an authenticated/idempotent `POST` to a trigger locator with a high-entropy bearer secret. Trigger secrets are shown once and stored only as a strong verifier/hash. They may have expiry and maximum-fire bounds and can only satisfy the gate they were created for.

Query-string secrets and GET-triggered side effects are prohibited because browser/link previewers, crawlers, proxies, and logs can activate or leak them.

Duplicate/replayed trigger delivery latches the same occurrence gate and cannot create duplicate sessions/jobs.

### 13. Agent-created project tasks

Project WorkOrders use a distinct model-visible tool/service from the existing delegated `TaskTool`. The existing `TaskTool` remains the owner of subagent/delegated-run semantics.

The new tool must support atomic bounded batch creation so an agent can convert, for example, all ordered plan files into sequential WorkOrders in one transaction. Either the batch and ordering commit together or none do.

Agent-created WorkOrders carry parent session/turn/work-order lineage, cannot exceed the creating execution's project/approval/sandbox authority, and are subject to bounded batch size, pending descendant count, depth, repeat count, and rate limits. Agent creation cannot become unbounded persistent self-replication.

By default an agent-created WorkOrder inherits the creating session's model request rather than a human Task-composer convenience preference.

### 14. WorkPlan remains within-session state

`WorkPlan` does not become the project WorkOrder queue. A WorkOrder may materialize a session whose agent then creates/uses a WorkPlan for long-horizon completion.

This permits a large project program to be split into many clean sessions while each session independently preserves detailed work state across compactions/restarts.

### 15. Compatibility

Existing low-level `Schedule*` protocol and scheduler behavior remain valid for current consumers. The existing TUI `/tasks` schedule UI may be migrated/deprecated behind the new WorkOrder view, but the migration must be explicit and must not reintroduce the removed legacy independent task scheduler.

`TaskTool`, `AgentTask`, TodoState, WorkPlan, and TUI task registry names remain unchanged unless an implementation plan explicitly performs a bounded naming cleanup.

### 16. Canonical planning terminology

The next implementation milestone must amend `plans/001-terminology-and-domain-model.md` and the minimal relevant long-term specification relationships to add `WorkOrder`, `WorkOrderOccurrence`, and `SequenceLane` as distinct concepts. This ADR is the accepted decision authorizing that canonical amendment; implementation must not reinterpret `Workspace`, `AgentTask`, `Job`, `Schedule`, or `WorkPlan` instead.

## Consequences

### Positive

- Future work becomes durable and project-visible without corrupting session semantics.
- Several tasks can run simultaneously through existing worktree isolation and scheduler admission.
- Long implementation programs can be represented as multiple clean sessions rather than one indefinitely compacted conversation.
- Human, team, script, and agent creation converge on one project-scoped WorkOrder service.
- The global dashboard can report permission/attention state without becoming execution authority.
- Existing scheduler, session, worktree, approval, authorization, and WorkPlan work is reused rather than duplicated.

### Negative

- A new durable domain/store/protocol surface is required.
- The current simple `/tasks` schedule view becomes a compatibility/migration concern.
- Team-safe authorization requires WorkOrder operations to be directly project-resolvable rather than opaque schedule IDs.
- External trigger tokens add a secret lifecycle and HTTP attack surface that needs dedicated qualification.
- Non-Git local projects cannot receive truthful worktree isolation without an additional backend; v1 must serialize or explicitly opt into sharing.

### Neutral or deferred

- Cron syntax and arbitrary calendar recurrence are not required by this decision; bounded `NotBefore`, delay, repeat, and current low-level schedule infrastructure are sufficient initially.
- Cross-node placement may later resolve WorkOrders onto remote workspaces/execution nodes; the local milestone must use typed execution-target seams and not hard-code cwd.
- A web dashboard is not required; the TUI is the reference frontend.
- Automatic merge/integration of parallel task work is not implied by successful task completion.

## Security and reliability implications

- WorkOrder creation/update/reorder/cancel/trigger is project-authorized and audited.
- Model, approval, sandbox, and workspace policy are snapshotted/narrowed; execution cannot silently gain authority because preferences changed.
- Trigger secrets never appear in WorkOrder projections, logs, audit metadata, or query strings.
- External trigger replay is idempotent per occurrence.
- Occurrence claim/session/job materialization must be transactional or explicitly recoverable so restart cannot create duplicate sessions/jobs.
- Sequence reorder uses revision/CAS so concurrent team members cannot silently overwrite one another.
- A failed/attention predecessor holds a sequence by default.
- Worktree allocation/release uses existing managed lifecycle; cleanup never assumes task completion means a clean mergeable tree.
- Agent-created WorkOrders are bounded and inherit/narrow authority.

## Verification

Conformance requires evidence that:

- one immediate WorkOrder creates exactly one normal session and one scheduler-owned initial turn;
- restart at every claim/materialization boundary cannot duplicate an occurrence/session/job;
- two mutation-capable tasks for one Git project run in distinct managed worktrees;
- waiting task reorder is CAS-safe under concurrent clients;
- sequence failure holds later work until explicit resolution;
- AND/OR gate joining and duplicate trigger delivery are deterministic;
- finite repeat creates distinct occurrence/session identities and stops at the configured count;
- removed model/current policy tightening produces attention/narrowing rather than silent fallback/widening;
- team principals can only see/mutate authorized project WorkOrders;
- trigger tokens cannot enumerate or mutate anything beyond their one gate;
- agent batch creation is atomic and bounded;
- running WorkOrder sessions accept ordinary steer/cancel/permission behavior through existing session mechanisms;
- TUI/global dashboard remains a projection and does not own durable truth;
- no second scheduler/admission loop is introduced.

## Supersession

None.
