# Project Work Orders and Task View M006 — Agent WorkOrder Tool and Atomic Sequential Batches

Status: implemented

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Applicable ADRs:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: capability / security

Hard dependency: M002 closure and the M001 canonical WorkOrder mutation service. Scheduled after M005 for the default handoff sequence.

## 1. Objective

Expose project-level WorkOrder creation to agents through a dedicated model-visible tool/service that is semantically separate from the existing delegated `TaskTool`.

The tool must support bounded single creation and atomic ordered batch creation so an authorized agent can turn a set of large implementation plans into sequential future sessions—e.g. “add all plan files in this directory as sequential tasks and start the first now”—without partially committing the queue, widening authority, or enabling unbounded persistent self-replication.

## 2. Current implementation evidence

- `src/tool/task.rs` is already a large delegated-agent surface tied to `SubAgentTask`, AgentTask/AgentRun lineage, run groups, child control, and scheduler `JobKind::Subagent` submission.
- execution-ownership documentation explicitly assigns `TaskTool` to scheduler-backed delegation through `JobSubmissionService`.
- WorkPlan tools already demonstrate a bounded model-visible read/update surface with host-owned evidence restrictions.
- Tool execution context carries project/session/turn/run/origin authority and approval/sandbox snapshot information.
- child authority narrowing and parent ceilings are already implemented for delegated AgentRuns.
- M001 provides atomic WorkOrder batch creation and lane ordering; M002 provides execution/materialization.

## 3. Invariants that must not regress

- Existing `TaskTool` continues to mean delegated child/subagent work; WorkOrder creation is a distinct tool/domain action.
- The model never receives trigger secrets or project credentials through this tool.
- Agent-created WorkOrders remain project-scoped and carry exact parent session/turn/run/work-order lineage.
- Created WorkOrders cannot request a broader project, approval mode, sandbox profile, filesystem scope, model/provider authority, or workspace policy than the creating execution permits.
- Batch creation is atomic: all WorkOrders/lane positions commit or none do.
- Fan-out, batch count, pending descendant count, repeat count, WorkOrder nesting depth, and total created WorkOrders per root/turn are bounded.
- The tool cannot recursively invoke itself through an approval reviewer or special internal agent.
- A model assertion that work “should be scheduled” is not permission to bypass deterministic tool authorization/approval routing.
- Default agent-created model identity derives from the creating session/effective model policy, not the human Task-mode convenience preference.

## 4. Scope

### In scope

- dedicated WorkOrder tool/service registration and schema;
- actions for inspect/list capability as needed and create single/batch;
- atomic sequential batch creation using M001 service;
- lane selection/order and “start first now” semantics;
- parent lineage capture;
- authority/policy narrowing;
- bounded model/provider selection override only where explicitly allowed;
- hard fan-out/depth/repeat/rate limits;
- idempotent model retry/tool-call behavior;
- concise bounded tool result containing canonical WorkOrder IDs/status/order;
- audit/attribution events;
- tests against prompt/tool retry duplication, privilege escalation, and partial batch failure;
- tool docs/schema descriptions that clearly distinguish project WorkOrders from delegated TaskTool.

### Explicitly out of scope

- automatic autonomous conversion of every WorkPlan into WorkOrders;
- unbounded directory traversal or plan discovery inside the tool itself;
- reading plan files without ordinary read/list/glob tools;
- arbitrary workflow graphs;
- external trigger-secret creation/return through the model tool;
- automatic merging of completed WorkOrders;
- changing existing TaskTool delegated-run semantics.

## 5. Tool naming and schema

Prefer a distinct tool name such as `work_order` or a similarly explicit project-task name. Do not expose a second model-visible tool also named simply `task` unless the existing TaskTool is intentionally/versionedly renamed in a separate compatibility decision.

Suggested actions:

```text
work_order(action="create", ...)
work_order(action="create_batch", items=[...], sequence={...})
work_order(action="list", project_scope="current", ...)
work_order(action="get", id=...)
```

Mutation actions must use the canonical WorkOrder service. List/get may be useful for agents to avoid duplicate scheduling and inspect their own created work, but outputs remain bounded and project-authorized.

The tool should not expose low-level Schedule JSON, JobTemplate, raw database IDs beyond opaque canonical IDs, trigger verifiers, or hidden audit/provider fields.

## 6. Single creation semantics

Input fields should be intentionally narrower than the full human protocol:

- prompt/objective;
- optional bounded title;
- optional delay/not-before/sequential lane placement;
- finite repeat within agent-specific cap;
- optional requested model only if caller/provider policy allows explicit model choice;
- no external trigger secret return;
- workspace policy default inherited/narrowed from caller.

Default behavior for an agent-created WorkOrder should be explicit in the tool description: either immediate independent future session or sequence placement. It must not silently inherit the human Task composer’s last-used preferences.

## 7. Atomic batch semantics

`create_batch` accepts a bounded ordered list of WorkOrder specs. Minimum requirements:

- hard maximum item count, likely well below WorkPlan's 64-item cap unless justified by measurement;
- total serialized prompt/title bytes bounded;
- all entries target the caller's current authorized project unless future policy explicitly permits another project;
- one optional new/existing sequence lane;
- deterministic stable ordering;
- optional `start_first_now=true` implemented by making the first occurrence immediately/sequence-ready, not by bypassing WorkOrderCoordinator;
- remaining tasks use `SequenceReady` and any specified additional gates;
- transaction commits every WorkOrder + lane membership + idempotency record together;
- one failed item validation aborts the entire batch.

Return canonical IDs/order and concise status. Do not dump full prompts back into context.

## 8. “Plans directory” workflow

The user story:

> add all the plan files in the plans directory as sequential tasks and start the first one now

should be achievable by the normal agent using existing deterministic tools:

1. list/glob/read the target plan files according to user scope;
2. derive bounded prompts/titles for each requested plan;
3. call one `work_order.create_batch` with the ordered specs;
4. M001 commits the batch/ordering atomically;
5. M002 releases the first immediate item and later sequence-ready items.

The WorkOrder tool itself should not recursively scan arbitrary directories because that duplicates file-tool authority and makes scope/bounds harder to audit.

## 9. Lineage and causal attribution

Every agent-created WorkOrder must record:

- originating principal;
- current project/session;
- current turn ID;
- current AgentRun ID when applicable;
- parent WorkOrder ID/occurrence if the creating session itself came from a WorkOrder;
- tool-call/invocation identity used for idempotency;
- authorization/approval decision attribution.

This lineage is structural metadata, not an execution grant.

## 10. Authority and policy narrowing

Resolve effective create authority from the current `ToolExecutionContext` and daemon authorization service.

Rules:

- project target cannot differ from caller's allowed project unless an explicit multi-project capability exists and is checked;
- requested approval mode <= caller/parent/project ceiling;
- requested sandbox/workspace policy <= caller/parent/project ceiling;
- requested model/provider must be available to the current principal/project and allowed by provider scope;
- if omitted, model defaults to the creating session's stable selected/effective identity/policy;
- child/future WorkOrder does not inherit ephemeral permission “allow once” beyond its documented execution snapshot semantics;
- yolo/full-host cannot be smuggled through tool arguments when caller lacks it;
- external trigger gate creation via agent must not return trigger secret; either disallow in M006 or create disabled metadata requiring human action.

## 11. Fan-out, recursion, and resource bounds

Define host-owned limits such as:

- `MAX_AGENT_WORK_ORDER_BATCH`;
- max newly pending WorkOrders per tool call;
- max active/pending descendants attributed to one root WorkOrder/session/turn;
- max WorkOrder lineage depth;
- max total agent-created WorkOrders per turn/root session over time;
- max repeat count for agent-created WorkOrders (may be lower than human cap);
- max prompt/title bytes per item and per batch.

When limits are exceeded, return a typed actionable failure. Do not partially create up to the cap unless the caller explicitly requested a smaller valid batch.

Consider a project/admin policy switch disabling agent-created persistent WorkOrders while retaining human Task mode.

## 12. Idempotency and model retry

Use tool invocation identity plus canonical caller/project/turn/run scope as the submission namespace. Model/provider retry of the same tool call must return the existing WorkOrder/batch result rather than create duplicates.

Separate:

- invocation identity (one model tool call);
- batch submission key (one atomic WorkOrder set);
- semantic contents/fingerprint used to reject conflicting retransmission.

Do not deduplicate two distinct model tool calls merely because their prompts are textually equal.

## 13. Tool result and context budget

Return a bounded structured result such as:

```text
created: 7
lane: lane_x
items:
  - {id, position, state, short_title}
first_release: immediate
```

Cap returned item count/bytes to the same accepted batch maximum. Never return full prompt bodies unless a tiny single-item result requires it, and prefer not to.

Errors should distinguish validation, authorization/policy ceiling, conflict/stale lane, batch bound, unsupported capability, and service unavailable without exposing secrets/other-project existence.

## 14. Ordered work packages

### A — Tool contract and factory registration

Define dedicated tool name/actions/schema/descriptions, read/write category/risk classification, factory wiring, model-profile availability, and docs.

### B — Authority/lineage/idempotency adapter

Translate `ToolExecutionContext` into canonical WorkOrder create context, enforce project/policy/model narrowing, capture lineage, and derive invocation-scoped submission keys.

### C — Single/batch creation

Wire create/create_batch to M001 service, lane/order/start-first semantics, total-byte/item bounds, all-or-none transaction behavior, and compact results.

### D — Fan-out/recursion policy

Add durable/queryable root/parent counts and hard limits; ensure future nested WorkOrder sessions cannot recursively grow without bound.

### E — Negative/retry/security qualification

Add duplicate tool-call retry tests, partial-validation failure, broader-authority arguments, cross-project targets, model/provider scope, external-trigger secret negative, and TaskTool non-regression tests.

## 15. Required tests

Tool/schema:

- factory exposes dedicated WorkOrder tool and existing TaskTool unchanged;
- create/list/get schemas bounded and descriptive;
- unknown action/field rejected according to tool conventions.

Batch:

- ordered 1/N item creation;
- `start_first_now` creates first immediate + remaining sequential gates without direct execution;
- invalid item N aborts entire batch;
- lane conflict aborts entire batch;
- total bytes/item count/repeat caps exact.

Idempotency:

- same invocation retry returns same IDs;
- changed payload under same key conflicts;
- identical text in distinct tool calls creates distinct WorkOrders.

Authority/security:

- cross-project target rejected;
- child cannot select broader yolo/full-host/workspace policy;
- unauthorized model/provider rejected;
- default model derives from creating session, not Task-mode preference;
- agent cannot receive/create usable trigger secret through tool;
- agent-created descendant limits enforced across multiple calls/restart.

Integration:

- synthetic plan-file list -> one atomic sequential batch -> M002 starts only first immediate item;
- after first succeeds, next becomes eligible through sequence lane;
- parent WorkOrder/session/turn/run lineage round trips and is auditable.

## 16. Required verification

```bash
cargo test -p codegg --lib -- tool::task
cargo test -p codegg --lib -- work_order
cargo test -p codegg-core -- work_order
cargo test --test authorization -- work_order
cargo test --test agent_run -- work_order
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 17. Acceptance criteria

- Dedicated WorkOrder tool exists without changing delegated TaskTool semantics.
- Agent can create one project WorkOrder through canonical service.
- Agent can atomically create a bounded ordered sequential batch and request first immediate release.
- Model retries do not duplicate WorkOrders.
- Invalid batch never partially commits.
- Agent-created model/policy/workspace authority cannot exceed caller/parent/project ceiling.
- Fan-out/repeat/depth limits prevent unbounded persistent self-replication and survive restart.
- Parent causal lineage is durable/auditable.
- The plan-file queue user story works using ordinary file tools + one WorkOrder batch call.

## 18. Stop conditions

Stop if implementation requires widening the existing TaskTool into two unrelated meanings, bypassing WorkOrderCoordinator to start the first task, granting trigger secrets to the model, trusting model-provided principal/project authority, or relying only on in-memory fan-out counters.

## 19. Closure evidence required

- implementation commits;
- tool schema/name/factory evidence showing TaskTool separation;
- batch transaction and bounds table;
- invocation-idempotency matrix;
- authority narrowing/negative-test matrix;
- fan-out/depth/repeat restart evidence;
- representative plan-file sequential batch trajectory;
- exact verification commands and residual findings.
