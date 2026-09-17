# Project Work Orders and Task View Roadmap

Status: active

Repository baseline reviewed: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Long-term references:

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
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Governing ADR:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`

Related accepted ADRs/foundations:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `architecture/jobs.md`
- `architecture/scheduler.md`
- `architecture/project_catalog.md`
- `architecture/session.md`
- `architecture/worktree.md`
- `architecture/authorization.md`
- `architecture/tui.md`
- `architecture/work_plan.md`

External research basis:

- OpenAI Codex app uses a command-center model for multiple concurrent agents, isolated worktrees, and scheduled Automations.
- Cursor worktrees isolate concurrent task runs into independent checkouts.
- Cursor Automations separate schedule/webhook trigger intent from background agent execution.

## 1. Purpose and ownership boundary

This workstream adds a durable project-level task/work-order layer one step above ordinary CodeGG sessions. It enables developers, teams, scripts, and authorized agents to queue future work, order work sequentially, release work by time/delay/external trigger, run several sessions concurrently in isolated workspaces, and inspect all current/past/future project work from the TUI.

It owns:

- durable `WorkOrder`, occurrence, sequence-lane, and trigger identity/state;
- bounded release-gate evaluation and finite repeat semantics;
- project-authorized WorkOrder protocol/store/service operations;
- daemon coordination that materializes ready WorkOrders into normal sessions and scheduler jobs;
- project Task composer/view and the global Workspace dashboard projection;
- external trigger capability lifecycle;
- model-visible WorkOrder creation including atomic sequential batches;
- restart/contention/security qualification of the complete feature.

It consumes but does not redefine:

- Session/Turn interaction and steering;
- scheduler Job/Attempt/Schedule execution authority;
- `JobSubmissionService` admission boundary;
- managed Workspace/Worktree lifecycle;
- project catalog and typed identity;
- ApprovalRouter, sandbox policy, runtime preferences, and model resolution;
- team authorization/origin attribution/audit;
- WorkPlan/Goal/Todo long-horizon state inside a session;
- presence/session observation/project chat.

The governing rule is:

> WorkOrder decides when a normal session may be born; Session owns interaction; Scheduler owns execution; Worktree/Workspace owns isolation; WorkPlan owns within-session completion.

## 2. Work classification

### Invariants

- No second scheduler, admission queue, background scheduler loop, or independent retry owner is introduced.
- Waiting WorkOrders are not speculative Session rows.
- A materialized WorkOrder session is an ordinary canonical session, not a parallel task-chat runtime.
- WorkOrder, AgentTask, Job, Schedule, WorkPlan/WorkItem, Todo item, and TUI background task identities remain distinct.
- `Workspace` continues to mean a concrete checkout; the user-facing Workspace dashboard is only a UI label/projection.
- Project authorization is resolved at the daemon boundary for every WorkOrder mutation/read.
- WorkOrder reordering is revisioned/CAS-safe under concurrent principals.
- A WorkOrder cannot widen the model/provider/approval/sandbox/project authority available at execution time.
- External trigger tokens only satisfy one declared WorkOrder gate and never authenticate as a normal principal.
- Agent-created WorkOrders are bounded and cannot recursively create unlimited persistent work.
- Non-Git concurrent mutation is never mislabeled as isolated.

### Capabilities

- Start a Task from the normal prompt composer and optionally schedule/order/repeat it.
- Default Task submission (`wait 0`) starts like a normal session.
- View/reorder future tasks within one project using Vim-style navigation.
- Run several mutation-capable tasks in one Git project concurrently in distinct managed worktrees.
- Stop, steer, answer permissions/questions, and observe running task sessions using normal session mechanisms.
- Open a global Workspace dashboard showing authorized projects and task/session/attention state.
- Trigger a waiting task from an external script through a narrow authenticated endpoint.
- Ask an agent to create one or a bounded atomic batch of project WorkOrders, including sequential plan-file queues.
- Preserve the last Task-mode model as a convenience preference for human task composition.

### Infrastructure

- `WorkOrderStore` and revisioned domain types in `codegg-core`.
- `WorkOrderCoordinator` using the existing scheduler wake/reconciliation path.
- bounded project/global WorkOrder projections.
- task-trigger secret verifier storage and idempotency ledger.
- model-visible WorkOrder tool/service adapter.

### Polish

- concise task status/attention labels;
- `/workspace` command and keybinding/help integration;
- migration of the existing thin `/tasks` Schedule UI;
- diagnostics explaining why a task is waiting or needs attention.

## 3. Non-goals

This roadmap does not authorize:

- a general workflow/BPM engine;
- arbitrary boolean expression trees or shell/data predicates;
- unbounded cron/recurrence language;
- automatic merging of completed task worktrees;
- replacing low-level `Schedule*` APIs;
- replacing WorkPlan with a project queue;
- replacing AgentTask/TaskTool delegation;
- fabricating future sessions before materialization;
- a web frontend;
- a new project identity or a second project picker;
- general webhook integration beyond the narrow task-trigger capability;
- transparent copy/overlay isolation for non-Git projects in the first pass;
- unbounded agent-created persistent queues.

## 4. Current state

The baseline already contains most required substrate:

1. `ProjectCatalog` owns stable project identity and project→workspace/session associations without eager heavy activation.
2. The TUI already has project tabs, picker/catalog flows, per-project session summaries, view-switch epochs, stale completion guards, project-correct event routing, and Vim-style project navigation.
3. `JobStore`/`ScheduleStore` and the global scheduler provide durable queueing, attempts, retry/cancel/restart semantics, and one-shot/interval schedule occurrence claiming.
4. `JobSubmissionService` is the canonical durable submission boundary; `src/tool/task.rs` already uses it for delegated `Subagent` work.
5. The old independent background-task scheduler was removed. Existing TUI `/tasks` now adapts `ScheduleCreate/List/Delete`; legacy `Task*` wire operations fail closed.
6. Current TUI task scheduling is workspace/session-oriented and creates recurring `Subagent` schedule templates. It is not a project-level future-session domain.
7. Team authorization exists, but raw schedule list/get/delete/pause/resume operations remain opaque or workspace-oriented and therefore are not an adequate team-facing project Task API.
8. Managed worktrees already provide durable records, leases, restart reconciliation, safe cleanup, dirty/conflict attention states, and mutation-capable AgentRun isolation.
9. Runtime preferences already persist selected model/approval/sandbox state in daemon-owned principal scope.
10. WorkPlan/WorkItem provides durable long-horizon state *within* one session and explicitly excludes generic workflow/user automation.
11. Presence, project chat, read-only observation, and audit already provide the collaboration/visibility primitives required once WorkOrders project their status.

## 5. Target architecture

```text
Project
  |
  +-- WorkOrder (human/team/script/agent authored intent)
  |     |
  |     +-- release gates + sequence lane + repeat policy
  |     |
  |     `-- WorkOrderOccurrence
  |             |
  |             +-- optional managed Worktree / Workspace
  |             +-- Session
  |             |     `-- normal Turn / AgentRun / WorkPlan behavior
  |             `-- initial AgentTurn Job
  |                    `-- existing global Scheduler
  |
  +-- ordinary existing Sessions
  +-- project communication / presence / audit
  `-- repository/workspace/worktree state
```

### 5.1 WorkOrder lifecycle

Suggested WorkOrder/occurrence projection states:

- waiting/future;
- ready;
- claiming/starting;
- running;
- needs-attention;
- completed;
- failed;
- cancelled.

Exact core enum decomposition may distinguish WorkOrder template state from occurrence state. Terminal occurrence state must never be inferred from model prose.

### 5.2 Release gates

The initial closed gate set is:

- immediate/zero delay;
- delay duration;
- not-before timestamp;
- sequence-ready lane condition;
- external trigger.

Multiple enabled gates use explicit `All` or `Any`. Gate satisfaction is persisted/latched per occurrence.

### 5.3 Sequential lanes

Sequence ordering is a project-level revisioned lane. Waiting rows have stable positions. Reorder is atomic/CAS. Claimed/running occurrences cannot be reordered behind themselves. Default failure/attention behavior holds downstream work.

### 5.4 Materialization

The daemon coordinator claims ready occurrences exactly once, resolves model/policy/workspace state, optionally acquires a managed worktree, creates the canonical session, and submits its initial AgentTurn through `JobSubmissionService`.

Occurrence/session/job correlation must survive restart and allow deterministic recovery of partial materialization.

### 5.5 Workspace isolation

Mutation-capable Git task sessions default to managed worktree isolation. Allocation is lazy at start. Read-only work may share under existing leases/policy. Non-Git mutation defaults to serialized/shared-safe behavior unless an explicit unsafe option is chosen.

### 5.6 Task-mode preference

Human Task mode remembers the last selected stable model identity through runtime preferences. Each WorkOrder snapshots the requested model identity and requested approval/sandbox policy, then re-resolves/narrows at execution.

### 5.7 TUI views

Project Task view:

- running;
- waiting/future ordered lane(s);
- needs-attention;
- recent history;
- task composer/scheduling sheet;
- enter/focus opens materialized normal session;
- `j/k`/arrows reorder only eligible future tasks.

Global Workspace dashboard:

- all authorized projects;
- counts of running/waiting/future/attention tasks;
- pending permission/question indicators;
- coarse task/session/project health;
- lazy project/detail loading;
- hotkey + `/workspace` entry;
- same Vim navigation conventions as project/session views.

## 6. Dependency graph

```text
M001 WorkOrder domain/store/protocol foundation
      |
      v
M002 release coordinator + session/worktree materialization
      |
      v
M003 project Task composer/view
      |
      v
M004 global Workspace dashboard + team projection
      |
      +------------------+
      |                  |
      v                  v
M005 external trigger  M006 agent WorkOrder tool + atomic batches
      |                  |
      +---------+--------+
                v
M007 restart/contention/security qualification
```

Dependency classification:

- M001 has only closed foundation dependencies plus accepted ADR-0005 and is ready.
- M002 hard-depends on M001.
- M003 hard-depends on M002 so the UI is built against real materialization semantics rather than mocks.
- M004 hard-depends on M003's project projection contract and consumes closed project catalog/presence/routing infrastructure.
- M005 hard-depends on M002's occurrence/gate semantics and M001 authorization/storage contracts; it may proceed after M002 independently of M004 in implementation reality, but registry sequencing keeps one clear handoff chain.
- M006 hard-depends on M002 and the canonical WorkOrder mutation service; it may proceed independently of M005 after M004 if desired.
- M007 hard-depends on M001-M006.

## 7. Milestones

### M001 — WorkOrder domain, storage, authorization, and protocol foundation

Class: infrastructure / invariant

Plan: `plans/implementation/project-work-orders-task-view/001-work-order-domain-storage-protocol.md`

Status: closed (`plans/closure/project-work-orders-task-view/001-status.md`; implementation `a856f2e0`).

Add typed WorkOrder/Occurrence/SequenceLane IDs and bounded domain validation, additive durable storage, CAS ordering/update semantics, project-scoped protocol/authorization/audit operations, and bounded projections. Amend canonical terminology/spec relationships to include WorkOrder without redefining existing concepts.

Exit condition: authorized clients can create/list/get/update/reorder/cancel durable waiting WorkOrders and occurrence records with stable identities/revisions, but no WorkOrder starts execution yet.

### M002 — Release coordinator, occurrence materialization, and workspace isolation

Class: capability / invariant

Plan: `plans/implementation/project-work-orders-task-view/002-release-coordinator-session-materialization.md`

Status: closed (`plans/closure/project-work-orders-task-view/002-status.md`; implementation `c54656e5`).

Implement release gates, finite repeat, sequence progression, atomic occurrence claim, model/policy re-resolution, managed-worktree allocation, canonical session creation, initial AgentTurn submission, cancellation/attention propagation, and restart reconciliation through the existing scheduler boundary.

Exit condition: immediate/time/delay/sequence WorkOrders materialize exactly once into normal sessions/jobs; concurrent Git mutation tasks receive distinct managed worktrees; failures hold sequence by default.

### M003 — Project Task composer, scheduling sheet, and task/session view

Class: capability

Plan: `plans/implementation/project-work-orders-task-view/003-project-task-composer-and-view.md`

Status: closed (`plans/closure/project-work-orders-task-view/003-status.md`; implementation `79a00460`).

Add composer-level Task mode, scheduling dialog, last-task-model preference, ordered project Task view, Vim-like navigation/reorder, migration from thin Schedule `/tasks` presentation, and normal-session focus/steer/cancel behavior after materialization.

Exit condition: a developer can author, order, schedule, inspect, enter, stop, and redirect project tasks entirely from the TUI without bypassing daemon authority.

### M004 — Global Workspace dashboard and team-aware task projection

Class: capability

Plan: `plans/implementation/project-work-orders-task-view/004-global-workspace-dashboard.md`

Status: closed (`plans/closure/project-work-orders-task-view/004-status.md`; implementation `8c6e8190`).

Evolve/reuse the project picker into a global Workspace dashboard with bounded authorized project/task/session/attention summaries, lazy detail loading, hotkey and `/workspace`, team privacy behavior, and project-correct navigation.

Exit condition: authorized users can see all known projects and coarse ongoing/future/attention state from one bounded screen and enter the relevant project/session without N+1 heavy activation.

### M005 — External task-trigger capability and endpoint

Class: capability / security

Plan: `plans/implementation/project-work-orders-task-view/005-external-task-trigger-endpoint.md`

Status: closed (`plans/closure/project-work-orders-task-view/005-status.md`; implementation `f22c9d8d`).

Add high-entropy one-purpose task-trigger secrets, verifier-only storage, authenticated/idempotent POST endpoint, expiry/max-fire/revocation, audit, occurrence gate latching, and replay/race protection.

Exit condition: an external script can safely release exactly the intended waiting gate without receiving general CodeGG credentials or causing duplicate execution.

### M006 — Agent WorkOrder tool and atomic sequential batch creation

Class: capability / security

Plan: `plans/implementation/project-work-orders-task-view/006-agent-work-order-tool-and-batches.md`

Status: ready (M002 closed; M005 closed — ordering gate satisfied).

Add a distinct model-facing WorkOrder tool/service (not delegated `TaskTool`) supporting bounded single/batch creation, atomic lane ordering, parent lineage, authority inheritance/narrowing, and fan-out/repeat/depth limits.

Exit condition: an authorized agent can transform a bounded ordered set of plan files into sequential WorkOrders atomically, start the first immediately, and cannot create unbounded or broader-authority persistent work.

### M007 — End-to-end recovery, contention, security, and usability qualification

Class: invariant / capability closure

Plan: `plans/implementation/project-work-orders-task-view/007-work-order-trajectory-and-recovery-qualification.md`

Status: blocked on M001-M006 closure.

Run fault-injection and representative long trajectories across restart boundaries, concurrent clients, duplicate triggers, removed models, policy narrowing, permission waits, worktree conflicts, cancellation, reorder races, sequence failure, repeat exhaustion, team privacy, and agent batch creation. Add only corrective production changes needed for closure.

Exit condition: the complete WorkOrder/task-view capability has closure evidence proving exactly-once materialization, one scheduler owner, correct authority/isolation, recoverability, and bounded UI/tool behavior.

## 8. Cross-cutting storage and migration

- M001 uses the next sequential additive storage migration and bumps the storage layout version.
- Waiting WorkOrders are not backfilled from historical `Schedule` rows unless an unambiguous one-time compatibility mapping is explicitly justified; default is no speculative migration.
- Existing low-level schedules remain readable/usable.
- Occurrence→session/job/workspace/worktree links are nullable until materialization and immutable or monotonic after claim.
- Trigger secret verifiers are separate from ordinary provider/API credentials and never stored in cleartext.
- CAS revision applies to WorkOrder updates and sequence-lane ordering.

## 9. Protocol and compatibility

- Add bounded `CoreRequest`/`CoreResponse`/event DTOs for WorkOrder capabilities.
- Operations carrying `project_id` should be direct-project scoped; ID-only operations must resolve owning project server-side before authorization.
- Project/global dashboard projection must remain bounded and capability-negotiated.
- Legacy `Task*` compatibility requests remain rejected as today.
- Existing `Schedule*` protocol remains a low-level primitive; TUI migration must not delete it solely because WorkOrder exists.
- Running task sessions remain ordinary Session protocol targets.

## 10. Security and authorization

- WorkOrder create requires the project capabilities needed to create/invoke the future session; read/list requires project/session read semantics; cancel/reorder/update use explicit mapped capabilities.
- Origin principal and authorizing decision are captured on creation and mutations are audit-attributed.
- Execution-time authority is revalidated; old WorkOrders do not retain revoked/widened permissions as a hidden capability.
- Trigger tokens are narrow capabilities, hashed/verifier-only, revocable, non-enumerable, and never query-string credentials.
- Agent WorkOrder tool inherits/narrows caller authority and has hard fan-out/repeat/depth limits.
- Dashboard privacy follows existing not-found/filtered project conventions.

## 11. Reliability, cancellation, restart, and contention

The workstream must define and prove:

- idempotent WorkOrder/batch submission;
- CAS-safe edits/reorder;
- exactly-once occurrence claim under duplicate wakes;
- recoverable claim→worktree→session→job materialization;
- deterministic restart handling for partially materialized occurrences;
- cancellation before claim, during workspace preparation, during session execution, and after terminal result;
- default sequence hold on failed/cancelled/needs-attention predecessor;
- no duplicate occurrence from repeated trigger delivery;
- bounded repeat exhaustion;
- worktree dirty/conflict attention retention;
- stale UI completions do not mutate the wrong project/task;
- daemon/TUI disconnect does not cancel daemon-owned running work.

## 12. Observability and audit

At minimum expose bounded structural events for:

- WorkOrder created/updated/reordered/cancelled;
- occurrence gate satisfied/released/claimed;
- occurrence session/job/workspace materialized;
- task entered needs-attention and reason code;
- external trigger created/revoked/fired/replayed;
- agent batch created/rejected;
- occurrence terminal result.

Events must not include trigger secrets, provider credentials, hidden reasoning, or unbounded prompt/output bodies.

## 13. User-visible exit conditions

The workstream is complete when a developer can:

1. press/cycle into Task composer mode in a project, type a normal prompt, press Enter, and confirm a schedule whose default is immediate/zero delay;
2. create and reorder sequential future tasks with `j/k` or arrows;
3. combine sequence, delay/not-before, finite repeat, and optional external trigger with explicit All/Any semantics;
4. run multiple same-project Git mutation sessions concurrently without sharing a write checkout;
5. enter any running task and steer/stop/respond exactly as a normal session;
6. see all current/future/recent tasks for a project;
7. open `/workspace`/hotkey global view showing authorized projects and attention state;
8. fire a waiting task from an external script using a narrow trigger token;
9. ask an agent to atomically enqueue ordered plan files as sequential tasks;
10. restart the daemon/TUI at adversarial points without duplicate or lost task execution.

## 14. Known risks and deferred work

- Non-Git isolated mutation requires a future copy/overlay backend; initial behavior must serialize or require explicit unsafe sharing.
- Automatic merge/integration of parallel worktrees remains separate.
- Calendar/cron richness beyond bounded initial semantics is deferred.
- Cross-node placement may later extend WorkOrder materialization onto linked nodes; local typed seams must not block it.
- Large project dashboards can become N+1/heavy unless aggregate projections remain daemon-owned and bounded.
- Naming collision with existing TaskTool/TUI task registry is an ongoing maintainability risk; architecture/docs must consistently use `WorkOrder` internally.

## 15. Planning/closure discipline

Each milestone must land its own closure record under `plans/closure/project-work-orders-task-view/` before the next hard-dependent milestone becomes `ready` in `plans/registry.md`.

M007 is qualification/closure, not a broad polish phase. Any material architecture defect discovered there requires a bounded corrective plan rather than silently expanding M007.
