# Agent Run, Async Delegation, and Worktree Concurrency M009 — Root Completion Delivery, Invocation Scope, and Exact-Head Closure

Status: ready

Repository baseline: `d08f089f7a72319eb343a070c93369cbb4fc50a4`

Source corrective addendum:

- `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md`

Historical corrective records retained:

- `plans/closure/agent-run-worktree-concurrency/007-status.md`
- `plans/closure/agent-run-worktree-concurrency/008-status.md`

Superseded strict subsystem disposition:

- `plans/closure/agent-run-worktree-concurrency/008-status.md`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/003-planning-process.md#7-corrective-passes`

Primary class: correctness / compatibility / closure

## 1. Objective

Close the remaining post-M008 production-path defects without reopening the durable run, scheduler, worktree, group, or projection architecture that M007/M008 corrected successfully.

M009 has four bounded responsibilities:

1. make top-level child and turn-owned group completion push back into the active owning root turn, while preserving existing nested run-owned completion push;
2. make model tool-call identity unique to the accepted execution occurrence rather than only `session_id + provider tool_call_id`;
3. separate durable delegation identity from request payload fingerprinting so incompatible reuse of one accepted call identity fails explicitly;
4. return the exact accepted repository head to a green existing CI lane, fixing only the lint failures exposed by that lane.

M009 MUST NOT introduce a second scheduler, a new notification database, polling workers, synthetic root `AgentRun`s, new CI lanes, release automation, or broad agent-loop refactors.

## 2. Why M009 exists

M007/M008 remain substantial and accepted implementation history. A subsequent production-path audit found four residual issues that invalidate the later claim of strict subsystem closure.

### F8 — top-level child completion is not pushed to the active root turn

`RunControlService::record_terminal()` currently resolves the durable parent through `run.parent_run_id`. That correctly routes nested completion to a live parent run, but a top-level delegated child has `parent_run_id = None` because its owner is the originating turn.

The root turn already has a stable `(session_id, turn_id)` identity and an `AgentLoop::follow_up_sender()`, but `RunControlService` has no live-turn endpoint keyed by that owner. A primary agent can therefore spawn durable work asynchronously but generally must call `wait`/`status` to observe completion.

### F9 — turn-owned group terminal delivery and terminal projection use the wrong routing seam

M007 correctly added `AgentRunGroupOwner::Turn`, but terminal live delivery still consults `summary.group.owner_run_id`. For turn-owned groups that field is compatibility/storage metadata, not an actual live owner run.

The member-terminal recomputation path also does not consistently publish the newly authoritative `AgentRunGroupUpdated` projection. `TaskTool` publishes group projections for direct group actions, but completion caused by member state transitions can become visible in storage without an equivalent incremental projection event.

### F10 — model tool-call identity is scoped too narrowly

`AgentLoop::build_tool_execution_context()` currently derives `invocation_key` as:

```text
<session_id>:<provider tool_call_id>
```

and leaves `ToolExecutionContext.turn_id` unset.

Provider call IDs MUST NOT be treated as session-global identifiers. Codegg's own text-repair compatibility path emits IDs such as `text-repair-0`, `text-repair-1`, and restarts that numbering for each repaired response. Separate model responses—and separate root turns—can therefore produce the same current invocation key.

A durable call identity must distinguish the accepted execution occurrence while remaining stable when that same accepted call is retried internally.

### F11 — delegation identity still includes the request body

The M008 implementation added `AgentTaskRecord.request_fingerprint`, but `delegation_key()` still hashes the resolved invocation identity together with agent/prompt/path request content.

That means reusing one invocation identity with a changed payload can produce a different delegation key and bypass the intended `request_fingerprint` conflict check. Identity and request validation remain partially conflated.

### F12 — the exact strict-closure head has a failed existing CI run

The push CI for `d08f089f7a72319eb343a070c93369cbb4fc50a4` is:

- workflow run `33566227214`
- job `100049920583` (`verify`)
- conclusion: failure
- failing step: `Workspace Clippy`
- observed error: `clippy::too_many_arguments` on `collect_agent_run_result` in `src/scheduler/executors.rs`
- workspace tests were skipped after the lint failure.

This does not invalidate M007/M008's focused local test evidence, but a strict final disposition MUST NOT describe the exact repository head as clean while the repository's one existing normal verification lane is red.

## 3. Invariants that must remain unchanged

M009 is corrective composition work. Preserve all of the following:

- scheduler remains the sole daemon machine-resource admission authority;
- root orchestration remains turn-owned; do not fabricate a primary `AgentTask` or `AgentRun`;
- nested orchestration remains owned by the currently executing durable run;
- direct-parent control authorization from M007 remains unchanged;
- top-level control remains exact originating session + turn;
- durable run depth and root lineage remain store-authoritative;
- mutation-capable delegated runs continue to receive managed isolated worktrees;
- child local commit authority remains limited to the owned worktree;
- child completion never merges, pushes, rebases, or rewrites parent history implicitly;
- `AgentRunResult` remains the structured completion contract;
- mailbox/group/run stores remain the durable authorities;
- projection remains derived and non-authoritative;
- no prompts, mailbox bodies, credentials, authority bodies, hidden reasoning, or full paths are added to projection or identity records;
- existing legacy/direct TaskTool callers remain compatible unless they depended on accidental cross-call identity collisions.

## 4. Target design

### 4.1 One typed execution-owner scope for model invocation identity

Model tool-call identity must be scoped to the actual agent execution owner plus the provider-turn sequence in which the tool call was accepted.

Use the smallest representation that fits the existing runtime. Conceptually:

```rust
enum AgentInvocationOwner {
    Turn {
        session_id: String,
        turn_id: String,
    },
    Run {
        run_id: AgentRunId,
    },
}

struct AgentInvocationScope {
    owner: AgentInvocationOwner,
    provider_turn: u64,
}
```

This type does not need durable storage if equivalent bounded fields already exist in `AgentLoop`. It is a runtime identity namespace, not a new authority database.

The accepted tool invocation key should be equivalent to:

```text
<owner-scope>/provider-turn/<sequence>/tool-call/<provider-id>
```

The concrete serialized form may be a bounded opaque digest. Do not persist full prompts or arguments in the key.

Requirements:

- root execution scope uses exact `session_id + turn_id`;
- durable subagent execution scope uses exact current `AgentRunId`;
- provider-turn sequence is the existing monotonic AgentLoop provider-turn counter, or an equally stable monotonic accepted-response sequence;
- a retry of one already accepted tool call reuses the same resolved identity;
- the same provider call ID in a later provider response receives a different identity;
- the same provider call ID in a later user turn receives a different identity;
- the same provider call ID in a different durable run receives a different identity;
- duplicate provider call IDs inside one accepted response must not silently alias. Preserve an existing rejection if one exists, otherwise include accepted call ordinal or reject the malformed duplicate explicitly.

Do not use display text, prompt content, tool arguments, filesystem paths, provider/model names, or wall-clock timestamps as primary identity.

### 4.2 Propagate real turn identity into `ToolExecutionContext`

Root `TurnRunInput` already has the authoritative `turn_id`, and `build_session_tool_registry` already receives it. Carry that same value into the owning `AgentLoop` rather than leaving `ToolExecutionContext.turn_id = None`.

For a durable child loop, the invocation owner is the current run. Its task's originating turn may still be copied into `ToolExecutionContext.turn_id` for audit/provenance, but the `AgentRunId` is the uniqueness authority for that child execution scope.

Avoid introducing another generated root-turn identifier when the daemon already supplies one.

### 4.3 Separate delegation identity from request fingerprint

Refactor the durable spawn identity contract so:

- **delegation identity** answers: "is this the same accepted spawn call?"
- **request fingerprint** answers: "is this accepted call being replayed with the same immutable request?"

The default durable `delegation_key` MUST be derived from resolved call identity plus bounded action/owner scope only. It MUST NOT include prompt, description, allowed paths, agent name, or other request body fields that are already validated by `request_fingerprint`.

For `spawn_many`, member identities remain deterministic children of the accepted parent invocation identity (`/member/<ordinal>` or equivalent). Each member then has its own delegation identity and request fingerprint.

Required behavior:

- same accepted call + same request -> same task/run/job;
- same accepted call + materially different request -> explicit idempotency/protocol conflict;
- different accepted calls + identical request -> distinct task/run/job;
- restart replay preserves the same result when the accepted key is re-presented;
- explicit user/caller idempotency key retains precedence where currently supported.

Keep request fingerprints bounded digests. Do not duplicate prompt bodies into identity columns.

### 4.4 Add a live turn-owned follow-up endpoint beside live run handles

`RunControlService` already owns live run registration and parent completion delivery. Extend that same service with a bounded live root-turn registration instead of creating another subsystem.

Conceptually:

```rust
struct LiveTurnOwner {
    session_id: String,
    turn_id: String,
}

// value needs only follow-up delivery; do not grant run steer/cancel authority.
live_turns: HashMap<LiveTurnOwner, mpsc::Sender<String>>
```

A unified typed map is acceptable if it is smaller and preserves the distinction between turn follow-up capability and run control capability. Do not accidentally give a root-turn registration child-run steering channels.

Lifecycle:

1. `DefaultTurnRuntime` builds the root `AgentLoop` with the exact turn ID.
2. Before the spawned loop begins accepting asynchronous completion, register `(session_id, turn_id) -> agent_loop.follow_up_sender()` with `RunControlService`.
3. Unregister on every terminal path: success, error, cancellation, and caught panic.
4. Registration replacement for the same exact turn should be deterministic and bounded; stale registrations must not accumulate.
5. If a child completes after the owning turn is no longer live, completion remains durable in run/group state and projection. Do not create an unbounded in-memory backlog merely to replay into an ended turn.

### 4.5 Route individual completion by explicit owner kind

When `record_terminal()` receives a run completion:

- if `run.parent_run_id = Some(parent)`, preserve current run-owned delivery to the live direct parent run;
- if `run.parent_run_id = None`, load the owning task and route to the exact live originating `(session_id, turn_id)` when present;
- if the top-level task has no originating turn ID (legacy row), do not guess from session, current UI state, or another run;
- absence of a live owner endpoint is not a run failure.

Completion text remains bounded. Do not include hidden reasoning or full prompt bodies.

### 4.6 Route group completion by `AgentRunGroupOwner`, never compatibility `owner_run_id`

Terminal group delivery must branch on the persisted group owner:

- `AgentRunGroupOwner::Run { run_id }` -> live run-owned follow-up;
- `AgentRunGroupOwner::Turn { session_id, turn_id }` -> exact live turn-owned follow-up.

`owner_run_id` may remain for additive storage/backward compatibility, but it MUST NOT be the live delivery authority for turn-owned groups.

Group terminal push must be deduplicated using the existing durable notification-claim semantics or an equally bounded transition result. Repeated member terminal reconciliation/restart replay must not spam the owner with the same completion.

### 4.7 Publish authoritative terminal group projection from the member-transition path

When member completion causes a group recomputation, publish the resulting bounded `AgentRunGroupUpdated` projection through the existing bus/reducer path.

Requirements:

- use `projection_replay::run_group_summary()`; do not hand-build a second DTO;
- session routing derives from explicit owner/task authority, not display IDs;
- a transition to terminal group state becomes visible incrementally without waiting for the parent to issue `status_group`;
- restart/snapshot reconstruction and incremental publication agree;
- duplicate upserts are acceptable only if reducer semantics are idempotent; duplicate user-facing follow-up messages are not;
- preserve bounded owner metadata from M008.

Do not add a polling task per group.

### 4.8 Resolve the existing exact-head lint failure minimally

The current CI failure is `collect_agent_run_result` having nine arguments under workspace `-D warnings`.

Use the smallest maintainable correction:

- preferred: one private input/evidence struct that groups the already-related result-collection fields without changing behavior;
- acceptable if strictly smaller and documented: one narrow function-local lint allowance;
- prohibited: broad `#![allow]`, workspace-wide lint relaxation, CI removal, or result-collection refactor unrelated to the warning.

After that first warning is corrected, run the same exact workspace Clippy command. If another warning that was already hidden behind the first failure appears, fix only that directly exposed warning with an equally narrow change. Do not turn M009 into general lint cleanup.

## 5. Ordered work packages

### A — Add regression fixtures before changing behavior

Add focused tests demonstrating the current failures:

1. root turn + top-level child completion -> no push today, expected push after correction;
2. nested run + child completion -> existing direct-parent push remains valid;
3. root turn + turn-owned group reaches terminal -> one root follow-up;
4. member-terminal recomputation -> `AgentRunGroupUpdated` reflects terminal state;
5. same provider tool-call ID in two root turns -> distinct invocation identities;
6. `text-repair-0` in two provider turns of one AgentLoop -> distinct invocation identities;
7. same accepted invocation identity replay -> same identity;
8. same accepted spawn identity + changed request -> explicit conflict;
9. distinct accepted identities + identical spawn request -> distinct runs.

Prefer unit/integration seams around `AgentLoop`, `TaskTool`, `RunControlService`, and core run/group stores. Do not require live model/provider calls.

### B — Make execution owner and provider-turn sequence available to tool execution

Likely touchpoints:

- `src/agent/turn_runtime.rs`
- `src/agent/agent_loop_factory.rs`
- `src/agent/loop.rs`
- `src/agent/tool_batch.rs`
- `src/agent/worker.rs` only if delegated owner propagation is not already sufficient.

Tasks:

- carry root `turn_id` into AgentLoop;
- expose current durable `run_id` as invocation owner for child loops using the existing run-control/run-owner seam rather than another copy of lineage;
- use the existing monotonic provider-turn sequence;
- build a bounded canonical invocation key;
- populate `ToolExecutionContext.turn_id` from authoritative runtime state.

### C — Correct delegation identity/fingerprint composition

Likely touchpoints:

- `src/tool/task.rs`
- `crates/codegg-core/src/agent_run.rs` only if existing conflict return types/tests need a small adjustment.

Tasks:

- remove request body components from delegation identity derivation;
- preserve request fingerprint validation;
- keep `spawn_many` member identity deterministic;
- ensure same-key/different-request replay reaches and fails the fingerprint conflict path;
- keep explicit idempotency key precedence.

No storage migration should be necessary unless implementation review proves the current bounded fingerprint column cannot express the conflict. A new migration requires explicit justification in the closure record.

### D — Register and clean up live root-turn follow-up ownership

Likely touchpoints:

- `src/agent/run_control.rs`
- `src/agent/turn_runtime.rs`
- possibly a small typed owner key in an existing agent-run/control module.

Tasks:

- add exact `(session_id, turn_id)` live follow-up registration;
- register before root loop execution can receive child completion;
- unregister on all spawned-loop terminal paths;
- prevent stale/replaced sender retention;
- preserve existing live run registration for nested agents.

### E — Correct individual/group terminal routing and projection publication

Likely touchpoints:

- `src/agent/run_control.rs`
- `crates/codegg-core/src/agent_run_group.rs`
- existing bus/projection adapter call sites only as needed.

Tasks:

- top-level run completion -> exact owning turn endpoint;
- nested run completion -> exact direct parent run endpoint;
- group completion -> branch on persisted owner enum;
- terminal group recomputation -> publish authoritative group projection;
- ensure completion push is claimed/deduplicated;
- do not use compatibility `owner_run_id` as turn delivery authority.

### F — Clear the exact CI lint blocker

Touch `src/scheduler/executors.rs` only as much as necessary to make the existing workspace Clippy step pass. Address any immediately revealed second baseline warning narrowly, then stop.

### G — Documentation and strict closure

Update only architecture text affected by the final contract, likely:

- `architecture/agent.md`
- `architecture/projection.md`
- `architecture/scheduler.md` only if invocation/execution ownership wording changes.

Create:

- `plans/closure/agent-run-worktree-concurrency/009-status.md`

Then reconcile:

- this plan -> `implemented`;
- final corrective addendum -> `closed`;
- registry subsystem row -> `closed` only if all criteria below pass.

M007/M008 closure records remain unchanged historical evidence.

## 6. Required tests and verification

Verification must remain proportional. Do not add workflows, matrices, scanners, benchmarks, coverage gates, release jobs, or provider-dependent tests.

### Focused behavior tests

At minimum cover:

- root active-turn top-level child completion push;
- no cross-turn or cross-session completion delivery;
- nested direct-parent completion push regression;
- turn-owned group terminal push exactly once;
- run-owned group terminal push regression;
- terminal member transition publishes terminal group projection;
- snapshot/replay agrees with incremental group terminal state;
- invocation identity differs across root turns with the same provider call ID;
- invocation identity differs across provider-turn iterations using repeated repaired IDs;
- invocation retry remains stable within one accepted call;
- identical payload under different accepted identities creates distinct durable runs;
- changed payload under the same accepted identity returns an explicit conflict;
- restart-readable durable acceptance still resolves the same call identity/fingerprint;
- no max-depth, lineage, worktree, authorization, cancellation, or group-join regression.

### Focused commands

Use the smallest applicable existing suites, expected to include variants of:

```text
cargo test -p codegg-core agent_run --locked -- --test-threads=1
cargo test -p codegg-core agent_run_group --locked -- --test-threads=1
cargo test --lib agent::run_control --locked -- --test-threads=1
cargo test --lib tool::task --locked -- --test-threads=1
cargo test --lib agent::worker --locked -- --test-threads=1
cargo test --test session_projection_consumer --locked -- --test-threads=1
cargo test --test scheduler_restart_recovery --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

Do not mechanically run every historical M001–M008 command if the focused suites and existing quick posture cover the changed seams.

### Hosted/exact-head evidence

Because the currently accepted `main` has a failed normal push CI run, strict M009 closure requires the repository's existing normal `CI / verify` lane to be green on the exact final candidate or exact closure head.

Do not create a manual verification workflow or additional lane. The ordinary push-triggered run produced by the implementation/closure commit is sufficient.

The closure record must cite:

- exact commit SHA;
- workflow run ID;
- verify job ID;
- final conclusion;
- confirmation that Workspace tests executed rather than being skipped behind Clippy.

## 7. Acceptance criteria

M009 may close only when all are true:

1. root-turn TaskTool ownership remains turn-owned and no synthetic root run exists;
2. an active root turn receives a bounded follow-up when one of its top-level durable children terminates;
3. a nested active run still receives direct-child completion and no sibling/child-to-parent authority widening is introduced;
4. completion cannot route to another turn merely because session IDs match;
5. turn-owned group completion reaches the exact active owning turn without consulting compatibility `owner_run_id` as authority;
6. run-owned group completion still reaches the exact live owner run;
7. group completion push is not duplicated by replay/reconciliation;
8. member-terminal group recomputation publishes the authoritative bounded group projection;
9. incremental and restart/snapshot projection agree on terminal group state;
10. root model tool-call identity includes exact turn ownership and provider-turn occurrence;
11. durable child tool-call identity includes exact current run ownership and provider-turn occurrence;
12. repeated repaired tool-call IDs across provider turns cannot alias;
13. same accepted call replay remains idempotent;
14. different accepted calls with identical payloads remain distinct;
15. same accepted call identity with changed immutable spawn request fails explicitly through fingerprint conflict;
16. no full prompt/body/path is added to identity or projection records;
17. scheduler, depth, worktree, direct-parent control, cancellation, and join semantics from M007/M008 remain intact;
18. `cargo fmt --all -- --check` passes;
19. `cargo clippy --workspace --all-targets --locked -- -D warnings` passes without broad lint suppression;
20. the repository's normal existing `CI / verify` run is green on the exact accepted final candidate and reaches Workspace tests;
21. no critical/high/medium correctness or security finding remains in this corrective scope;
22. no new CI lane, scanner, dependency bot, release automation, fixed release cadence, polling worker, notification database, or scheduler authority was added.

## 8. Explicit non-goals

M009 does not implement:

- arbitrary sibling messaging;
- persistent cross-session human/team chat;
- durable user notification inboxes;
- cross-daemon or remote worktree execution;
- provider-specific background job APIs;
- arbitrary tool execution in detached jobs;
- a new event-sourcing subsystem;
- progress streaming beyond existing bounded completion/control seams;
- TUI redesign beyond consuming the corrected projection events;
- new release/CI architecture;
- broad lint or style cleanup;
- refactoring `RunControlService`, `AgentLoop`, or scheduler merely for aesthetics.

## 9. Handoff notes

Implementation should start by reproducing F8–F12 against `d08f089f` before editing. Treat M007/M008's store and ownership corrections as constraints, not code to redesign.

Prefer small additive seams:

- one runtime invocation-owner/scope value;
- one live-turn follow-up registry in the existing run-control service;
- one explicit group-owner routing branch;
- one existing projection adapter call on member terminalization;
- one corrected delegation-key derivation;
- one narrow Clippy repair.

If implementation discovers a defect outside these seams, record it in the M009 closure review. Do not silently broaden the patch or create another large architecture pass unless the defect materially blocks the acceptance criteria.
