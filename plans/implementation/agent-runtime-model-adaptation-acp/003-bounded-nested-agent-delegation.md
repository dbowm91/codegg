# Agent Runtime, Model Adaptation, and ACP Milestone 003 — Bounded Nested Agent Delegation

Status: active

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-003--bounded-nested-agent-delegation`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- Stop and propose an ADR if implementation would introduce a second admission scheduler, redefine durable `AgentRun` ownership, or select a new persistence authority.

Primary class: capability/invariant

## 1. Objective

Make nested subagents functionally real and safely bounded using the existing daemon, scheduler/task, tool, event, and subagent-pool foundations. A child agent may call approved descendants only when its resolved capability surface permits delegation, its definition allows the target, depth/fan-out/budget limits permit it, and the resulting child authority is no broader than the parent.

This milestone is a bounded compatibility step toward the long-term durable agent-run service. It must correct the current wiring defect in which descendant requests carry depth and parent metadata but child `AgentLoop` instances are not installed with a functional shared spawner. It must not claim completion of full durable restart recovery, worktree allocation, team authorization, or the final `AgentRun` store.

## 2. Dependencies

Hard dependencies:

- Milestone 001: canonical prompt and agent resolution;
- Milestone 002: typed capabilities and one resolved tool surface.

Existing interfaces:

- `SubAgentPool`, `SubAgentSpawner`, `SubAgentRequest`, `TaskStore`, task tool runtime, session store, scheduler submission seam, cancellation tokens, app events, parent session/turn identifiers, max depth, max concurrency, and tool-call limits.

Soft dependencies:

- final durable `AgentRunId`/store and worktree-native mutation isolation remain future subsystem work;
- this milestone must leave typed seams so that migration does not require changing agent-facing delegation semantics again.

## 3. Current implementation evidence

The implementation agent must confirm:

- `SubAgentRequest` includes task ID, prompt, agent, parent ID, denied tools, allowed paths, description, depth, max tool calls, and parent model;
- `SubAgentPool` has max concurrency and max depth and owns a shared request queue;
- enqueue rejects a request when depth reaches the configured limit;
- root turn construction can install a subagent pool on `AgentLoop`;
- descendant execution constructs a fresh loop without installing the shared pool/spawner;
- read-only filtering currently removes `task` categorically;
- child permission calculation does not yet consume a typed parent authority ceiling from Milestone 002;
- parent/child lifecycle is represented mainly through task/session IDs rather than a durable lineage object;
- cancellation exists for pool shutdown and root turns but must be audited for subtree propagation and join completion.

## 4. Invariants

- The singleton daemon/scheduler remains the only admission authority.
- One shared descendant service/pool is reused; children do not create recursive independent pools.
- Child authority is an intersection and can never exceed parent authority.
- Child filesystem roots cannot exceed the parent workspace/path scope.
- Delegation is explicit per agent definition and target allowlist/denylist.
- Depth, direct-child count, active-descendant count, tool-call budget, token/model-call budget seam, and wall-clock budget are bounded per root lineage.
- Duplicate model retries or frontend retransmission do not create duplicate children when a stable delegation identity is available.
- Parent cancellation propagates downward by default and joins descendant tasks/processes before ownership is released.
- Read-only parents may delegate to equal-or-narrower read-only children when explicitly permitted.
- Mutation-capable parallel descendants must not be enabled without a worktree/serialization policy; default this milestone to deny or serialized behavior.
- Every descendant remains attributable to session, turn, parent lineage, agent definition/digest, workspace, model, and task call.

## 5. Scope

### In scope

- Define a bounded `DelegationPolicy` in agent configuration/runtime resolution:
  - enabled;
  - allowed/denied child agents;
  - max depth;
  - max direct children;
  - max active descendants;
  - max concurrent children;
  - max total child tool calls;
  - wall-clock/token/model-call budget seams;
  - workspace/path inheritance;
  - model inheritance/fallback behavior;
  - join policy.
- Carry a typed lineage context through root and child requests.
- Install the shared descendant spawner/runtime into eligible child loops.
- Increment depth and enforce root-level limits atomically.
- Intersect child capabilities/tool/path permissions with the parent ceiling.
- Add idempotent delegation identity derived from stable available fields such as session, turn, parent lineage, tool-call ID, and delegation ordinal.
- Define join policies needed now: `all`, `any_successful`, `first_completed`, and `detached` only if existing task ownership can support detached work safely. It is acceptable to defer detached mode explicitly.
- Add subtree cancellation and bounded join behavior.
- Publish lineage-aware lifecycle/progress events through existing event/projection seams.
- Ensure prompts advertise `task` only when delegation is functional and a target is permitted.
- Add user/project custom-agent configuration examples.

### Out of scope

- Final durable agent-run database schema and full restart recovery.
- Team principal authorization completion.
- Automatic worktree creation for mutation-capable descendants.
- Cross-daemon descendants.
- Agent-to-agent free-form messaging/team mailbox redesign.
- Arbitrary recursive delegation or unbounded fan-out.
- Specialized security/research preflight logic, which lands in M004/M005.

## 6. Required production changes

### Lineage and policy types

Add a typed lineage/runtime object independent of display session IDs. A compatibility lineage may use existing task/session/turn identifiers, but it must not encode hierarchy solely by concatenated strings.

Suggested fields:

```rust
pub struct AgentLineageContext {
    pub root_id: AgentLineageId,
    pub parent_id: Option<AgentLineageId>,
    pub delegation_id: DelegationId,
    pub depth: u16,
    pub root_budget: Arc<DelegationBudgetState>,
    pub cancellation: CancellationToken,
    pub parent_capabilities: AgentCapabilitySet,
    pub parent_workspace_scope: WorkspaceScope,
}
```

Use names/types consistent with existing domain terminology. Do not introduce a fake final `AgentRunId` if its durable semantics are not implemented.

### Agent configuration

Extend native agent TOML/config with a bounded delegation section. Built-in defaults:

- build/general may delegate to explicit known subagents within global limits;
- security-review may later delegate to approved security specialists at depth one;
- research may later delegate to approved evidence scouts;
- title, summary, compaction, and most leaf specialists cannot delegate;
- unspecified custom agents default to delegation disabled unless existing behavior requires a narrow compatibility default.

Prompt fragments cannot enable delegation independently of runtime config.

### Shared runtime integration

- Refactor descendant construction to use a shared factory capable of installing the same pool/spawner/submission authority.
- Pass explicit execution context and pinned asset snapshot/pin to children.
- Build child tool surfaces using parent ceilings through M002.
- Avoid recursive ownership cycles between the pool and child loop; use weak/service handles where appropriate.
- Do not register `task` when the resolved child policy has no valid target or the runtime lacks a spawner.

### Budget and admission

Use one root-scoped budget state with atomic/locked accounting. Enforce before enqueue and reconcile on completion/cancellation. Avoid holding locks while awaiting provider/tool work.

At minimum enforce depth, fan-out, active descendant count, concurrent children, child tool-call budget, and wall-clock timeout. Token/model-call accounting may initially be an explicit seam if provider usage cannot be atomically reserved; do not claim enforcement without evidence.

### Cancellation and joins

- Parent cancellation triggers child cancellation tokens.
- Child-owned tool/program/job cancellation uses existing mechanisms.
- Join waits are bounded; fallback aborts are explicit and recorded.
- Detached mode is disabled unless ownership, notification, restart, and cleanup are demonstrably correct.
- Pool shutdown cancels all roots and joins active handles without leaking counters/permits.

### Events and projection

Additive events/fields should report stable lineage identity, parent identity, depth, agent name, status, bounded description/result summary, and cancellation/failure reason. Do not embed full prompts or hidden reasoning.

Reuse canonical session projections instead of creating a second tree reducer.

## 7. Ordered work packages

### A — Contract and defect fixtures

- map current task/subagent creation and cancellation paths;
- define policy, lineage, budget, and join contracts;
- add a failing child-spawns-grandchild fixture demonstrating the missing spawner;
- add failing authority-escalation and read-only-delegation fixtures.

### B — Policy/config resolution

- add delegation schema and built-in defaults;
- integrate with agent inheritance from M001;
- validate target names, cycles where statically knowable, limits, and unsupported detached mode;
- expose resolved delegation capability to M002 surface.

### C — Shared descendant runtime

- pass shared service/spawner into child loop factories;
- propagate execution context, asset pin, lineage, model policy, and parent ceilings;
- increment depth correctly;
- ensure child `task` registration reflects actual eligibility.

### D — Root budgets and idempotency

- implement root-scoped limit accounting;
- derive delegation IDs from stable call identity;
- return/reuse prior task outcome on duplicate spawn where safe;
- reconcile counters on success, failure, cancellation, enqueue failure, and panic.

### E — Cancellation, joins, and events

- cascade cancellation;
- implement supported join policies;
- publish lineage-aware events/projections;
- verify shutdown and parent-completion cleanup.

### F — Documentation and compatibility

- document custom nested-agent examples;
- state mutation/worktree restrictions;
- identify compatibility types to be replaced by the final durable agent-run service.

## 8. Failure, cancellation, restart, and contention semantics

- Enqueue failure consumes no lasting budget/active count.
- A child that fails before provider start produces one terminal task/event state.
- Duplicate delegation requests return the same accepted child identity/outcome or an explicit idempotency conflict.
- Parent failure/cancel defaults to subtree cancellation; `detached` is not an implicit escape.
- Cancellation while waiting for a semaphore releases the request and reservation.
- Panic in one child cannot prevent sibling cancellation/join or root-budget reconciliation.
- Daemon restart may mark transient descendants interrupted under existing policy; full recovery is explicitly deferred and documented.
- Concurrent parents cannot exceed global pool/scheduler limits by each creating an independent pool.
- Root budget counters remain bounded and race-safe without serializing all child execution.

## 9. Compatibility and migration

- Existing first-level `task` calls continue to work within the new default policy.
- Existing `SubAgentRequest` may be extended additively or adapted into a new typed request.
- Existing task/session display IDs remain available but are not the sole hierarchy authority.
- Existing subagent events remain compatible; lineage fields/events are additive.
- Agents without a delegation section receive documented safe defaults.
- Do not remove `SubAgentPool` until a later durable-agent migration plan explicitly owns replacement.

## 10. Required tests

Focused:

- delegation policy parsing/inheritance;
- target allow/deny resolution;
- depth/fan-out/active/concurrency boundaries;
- parent capability and path intersection;
- read-only parent delegating to read-only child;
- attempted child escalation;
- missing spawner means no `task` surface;
- stable duplicate delegation identity;
- root budget reconciliation across all terminal paths;
- join policy behavior;
- subtree cancellation and pool shutdown.

Production-shaped:

- root build -> research coordinator -> two read-only scouts;
- root build -> security-review -> approved security specialist;
- three-level general fixture using mock provider/tools;
- one sibling fails while another completes and parent joins according to policy;
- cancellation during child tool execution returns resources to baseline.

Negative/security:

- child cannot widen tools, paths, shell, Git, commit, terminal, or network authority;
- custom prompt cannot enable an unconfigured target;
- mutation-capable parallel children are denied/serialized without worktree policy;
- recursive self-spawn terminates at policy/depth boundary;
- task descriptions/results remain bounded in events.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test agent::worker
cargo test tool::task
cargo test --test subagent
cargo test --test agent_loop_harness
cargo check --workspace
```

Run one broad local library suite at handoff. No new matrix CI, external provider requirement, or release automation.

## 12. Acceptance criteria

- Child loops receive a functional shared descendant spawner.
- A focused three-level fixture succeeds.
- Delegation is explicit, target-bounded, depth/fan-out/budget limited, and idempotent where stable identity exists.
- Child tool/path/capability authority cannot exceed the parent ceiling.
- Read-only agents may delegate safely when configured.
- Cancellation cascades and joins descendants without permit/counter leaks.
- Events/projections represent lineage without exposing private prompt/reasoning content.
- The implementation remains a bounded bridge and does not falsely claim final durable agent-run closure.

## 13. Stop conditions

Stop if:

- correct nesting requires a second scheduler or per-child pool;
- stable idempotency cannot be represented without selecting the final durable `AgentRun` schema;
- mutation-capable children require worktree allocation not yet available;
- team authorization must be invented rather than using the existing explicit authority seam;
- detached work cannot be owned, notified, cancelled, and cleaned up correctly;
- cancellation cannot reach existing child-owned jobs/tool programs without reopening their closed ownership contract.

## 14. Closure evidence

Include:

- lineage/policy diagrams and config examples;
- three-level execution transcript from deterministic fixtures;
- authority intersection matrix and escalation negatives;
- duplicate delegation evidence;
- cancellation/join/resource-baseline evidence;
- focused and broad local command results;
- explicit deferred durable/restart/worktree limitations;
- closure recommendation.
