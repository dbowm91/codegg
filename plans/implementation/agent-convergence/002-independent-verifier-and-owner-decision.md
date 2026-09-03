# Agent Convergence M002 — Independent Verifier and Explicit Owner Decision

Status: implemented

Repository baseline: `ffc3847c711a3ce7b410a1a59c205da8356dc645`

Source subsystem roadmap:

- `plans/subsystems/agent-convergence-roadmap.md`

Hard dependency:

- M001 `plans/implementation/agent-convergence/001-durable-convergence-cycle-foundation.md` must be strictly closed.

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable decisions and closed dependencies:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- agent-run/worktree M009 closure remains authoritative;
- goal-verification M013 closure remains authoritative;
- M001 convergence storage/state contract becomes authoritative after closure.

Primary class: capability / invariant

## 1. Objective

Implement the first complete user-visible convergence vertical slice:

```text
owner turn/run
   -> create convergence
   -> submit one producer through the existing durable task/scheduler path
   -> observe producer terminal result
   -> assemble bounded verifier evidence
   -> submit one independent read-only verifier AgentRun
   -> parse/persist typed semantic verdict
   -> expose `awaiting_decision`
   -> exact owner chooses accept | stop | escalate
```

This milestone proves independent verification and explicit owner decision points without introducing automatic repair. A semantic verifier that says `Pass` remains advisory; parent Git integration and host goal completion remain separate explicit operations.

## 2. Explicit non-goals

M002 must not:

- implement `repair` or `replan` execution;
- chain a new worktree from a producer result commit;
- run multiple competing producers;
- add automatic task-complexity detection or model-profile convergence defaults;
- allow the verifier to mutate files, run arbitrary shell commands, commit, merge, push, answer permissions, or request goal completion;
- auto-integrate a producer branch after verifier `Pass`;
- add a separate `team` scheduler/tool/runtime;
- copy producer transcripts or hidden reasoning into verifier context;
- modify deterministic goal-verification acceptance semantics;
- add unrestricted inter-agent chat.

If a caller wants a cheap second opinion without formal convergence, ordinary read-only `task spawn` remains the intended path.

## 3. Re-inspection required before implementation

After M001 lands, re-read:

- M001 closure and exact convergence store/service APIs;
- `src/tool/task.rs` for current action parsing, invocation identity, owner derivation, run/group service injection, and bounded output conventions;
- `src/agent/turn_runtime.rs` and `src/agent/worker.rs` for canonical child construction and resolved agent execution profiles;
- `src/agent/run_control.rs` for active turn/run control and completion notification;
- `crates/codegg-core/src/agent_run_group.rs` for group wait/notification rather than polling;
- `crates/codegg-core/src/run_result.rs` and `AgentRunStore::get_result` for producer evidence;
- `src/agent/registry.rs`, built-in agent generation, permission safety envelope, and prompt compilation;
- session projection/reducer/TUI run-tree surfaces;
- `crates/codegg-core/src/goal/verification.rs` and model-facing goal tools for the completion-authority boundary.

Use current production constructors. Do not instantiate `AgentLoop` directly from the convergence coordinator.

## 4. Model-facing orchestration surface

Prefer extending the existing `task` tool because it already owns durable delegation/control/group operations. Do not add a parallel `team` tool family merely to imitate MiniMax terminology.

Add bounded actions equivalent to:

```text
converge
convergence_status
convergence_decide
convergence_cancel
```

Exact spelling may be shortened if the current task schema has stricter compatibility constraints, but one canonical action family must exist.

### 4.1 `converge`

Initial M002 request shape should support exactly one producer request:

```json
{
  "action": "converge",
  "producer": {
    "prompt": "...",
    "agent": "build",
    "model": "optional override"
  },
  "criteria": ["bounded criterion", "..."],
  "verifier_agent": "verifier",
  "verifier_model": "optional override",
  "max_cycles": 1
}
```

Requirements:

- `max_cycles` must be exactly 1 in M002 or omitted to default to 1;
- objective/criteria are bounded before persistence/model construction and become the durable M001 convergence spec;
- the accepted task-tool invocation identity is the idempotency owner; retries of one accepted call return the same convergence/producer identity;
- owner is derived from the exact current turn or current durable run, never from a caller-supplied string;
- producer request uses normal agent resolution, permission intersection, scheduler submission, worktree classification, and budget inheritance;
- no convergence-specific resource bypass exists.

The action may return after durable acceptance rather than waiting synchronously for the producer/verifier. Status changes and completion should use existing push/projection mechanisms. If the current task tool can safely perform one bounded wait without blocking scheduler resources, it may optionally return a terminal single-cycle result when already available; detached durable acceptance remains the correctness baseline.

### 4.2 Status and cancel

`convergence_status` returns a bounded summary from the M001 convergence store plus referenced run/group statuses. It must not copy full child output.

`convergence_cancel` authorizes the exact owner/ancestor using existing run-control rules, transitions the convergence through a revision-checked cancel request, and requests cancellation only for currently active producer/verifier runs owned by this convergence.

Cancellation must not target unrelated sibling runs merely because they share a session or root.

### 4.3 Owner decision

`convergence_decide` accepts only the current legal M002 decisions:

```text
accept
stop
escalate
```

`repair` and `replan` must return a structured “not available until M003” error rather than being silently treated as another producer spawn.

A decision is authorized from the same owner relation that created the convergence. A stale duplicate decision returns the persisted winner or a typed conflict; it does not apply twice.

`accept` marks the convergence operation completed. It does not integrate Git or complete a goal.

## 5. Producer execution

The convergence application coordinator should call/reuse the same service boundary as `TaskTool::spawn`; it must not duplicate job/run creation logic.

Producer requirements:

- one durable `AgentTask`/`AgentRun` under the convergence owner;
- mutation-capable producer receives the ordinary managed worktree lease before model execution;
- read-only producer may inherit the parent workspace according to existing policy;
- normal depth/fan-out/tool/token/wall-clock limits apply;
- producer completion is observed from the authoritative run/group store and existing completion notifications;
- the convergence record stores only run/group references and state transitions plus its own bounded objective/criteria spec;
- if producer fails/cancels/times out/conflicts without a reviewable result, the convergence fails or becomes attention-required according to the M001 state/reconciliation contract; do not manufacture a verifier pass opportunity from missing evidence.

For a single producer, implementation may either create a one-member `AgentRunGroup` for uniform group semantics or reference the producer run directly. Prefer the shape that reuses existing completion notification without introducing synthetic ownership. Document the choice in architecture/closure evidence.

## 6. Independent verifier agent

### 6.1 Built-in definition

Add a dedicated hidden or normal subagent definition for semantic verification, for example `verifier` or `delivery-reviewer`. Use the existing built-in TOML + prompt generator path; never edit generated Rust directly.

The verifier's hard safety ceiling must be read-only. Even if a project user overrides/extends the agent definition, `apply_safety_envelope` or equivalent host construction must cap it to the verifier ceiling.

At minimum deny:

```text
write
edit
replace
multiedit
apply_patch
bash
terminal
python_script modes that can mutate
commit
Git mutation/integration actions
task/delegation
permission response
goal completion request
```

Allow only the minimal inspection surface required for semantic review, such as bounded file reads, grep/glob/list, read-only Git/diff, LSP reads where available, deterministic validators, and artifact/context reads. Do not give the verifier test execution in M002; producer/test jobs already contribute host validation evidence. If implementation proves a read-only verifier cannot inspect a required artifact without a narrowly scoped additional tool, document that exception and keep it non-mutating.

### 6.2 Prompt contract

The verifier system prompt must state:

- it did not produce the artifact and must independently challenge it;
- supplied `AgentRunResult`/validation/Git facts are host evidence;
- producer claims not present in the evidence packet are not facts;
- it must return exactly one typed verdict shape: pass, revise, or inconclusive;
- `Pass` means no blocking semantic finding within scope, not goal completion or merge approval;
- it must not request mutations or pretend to have run checks that are absent from host evidence;
- findings must cite changed path/line/evidence references where available;
- uncertainty or missing evidence must produce `Inconclusive` rather than invented confidence.

Use the existing model-adapter/tool-contract repair machinery for malformed structured output; allow at most the normal bounded repair behavior, not a convergence-specific infinite parse loop.

### 6.3 Context isolation

Construct the verifier with a fresh child context and pinned runtime asset snapshot appropriate to its run. Do not append the producer's full message history.

The verifier input should include the M001 `VerifierEvidencePacket`, persisted bounded objective/criteria, and references needed for targeted reads. If a diff is small enough for an existing bounded summary, include it. Otherwise provide a handle/read instruction rather than dumping the full diff into the system prompt.

Do not expose producer hidden reasoning even if the provider returned reasoning events.

## 7. Verifier result capture

A verifier `AgentRun` still terminates through the ordinary structured `AgentRunResult` path. M002 additionally needs a typed semantic verdict.

Preferred implementation:

- add a narrow host-owned verdict submission/finalization seam for the verifier runtime, analogous in spirit to specialized research/security finalization but specific to the convergence child;
- parse/validate the final structured verdict before marking convergence `AwaitingDecision`;
- persist the bounded verdict in the convergence cycle store;
- retain the verifier `AgentRunResult` separately as ordinary run evidence.

Avoid parsing arbitrary final prose with regex after the fact if a typed structured response/finalization seam already exists.

Malformed/unparseable verifier output after the normal single repair opportunity yields `Inconclusive`/attention-required or verifier failure. It must never default to `Pass`.

The convergence coordinator, not the verifier model, validates that returned file/evidence references fit allowed bounds and belong to the producer result/workspace scope.

## 8. Interaction with host goal verification

This is a mandatory regression boundary.

When the owner subsequently requests goal completion:

```text
semantic verifier Pass
       |
       v
(optional bounded explanatory evidence)
       |
       v
existing GoalVerificationService
       |
       +-- failed/in-flight host evidence -> NotMet
       +-- unsupported human criterion -> AwaitingUser
       `-- only deterministic accepted evidence -> Met
```

M002 may optionally surface the semantic verdict as explanatory context to the working model/user. It must not add a new `source = semantic_verifier` that the current deterministic goal verifier automatically counts as a passing test/delegated-run criterion unless a later independently planned goal-verification extension defines that authority.

Add a direct regression test proving semantic `Pass` cannot complete a goal with a failed host-recorded test.

## 9. Runtime coordinator and state advancement

Add an application-layer coordinator/service that composes the M001 store with existing task/run/group services. It owns advancing convergence phases after authoritative child completion.

Requirements:

- producer terminal notification can advance `Producing -> Verifying` exactly once;
- verifier terminal + persisted verdict can advance `Verifying -> AwaitingDecision` exactly once;
- active turn/run notification uses existing run-control/session projection endpoints rather than a new callback bus;
- daemon startup reconciliation reads M001 reconciliation decisions and resumes only missing coordination steps, never completed model work;
- waiting for a child consumes no scheduler process slot;
- failures to persist a transition are logged with convergence/run identity and leave recoverable durable child state; do not acknowledge a later phase before its prior durable transition is stored.

If the existing scheduler completion hook cannot safely invoke this coordinator without circular ownership, use the same bounded reconciliation/notification pattern used by run groups rather than modifying scheduler internals to know convergence semantics.

## 10. Projection and TUI behavior

Publish additive bounded convergence state through the existing session projection. The TUI should show enough to answer:

```text
Convergence <short id>: verifying (cycle 1/1)
Producer: <agent/run> completed
Verifier: <agent/run> running
Decision: pending
```

After verdict:

```text
Verdict: revise — 3 findings
Awaiting owner decision
```

Detailed findings should open existing run/result/detail surfaces rather than expanding the sidebar/event payload.

Reconnect/resync must reconstruct the same state from durable records. Do not create TUI-local convergence ownership.

ACP/native clients that do not understand the new optional projection fields must continue functioning.

## 11. Expected production-code touch set

Expected areas, subject to M001 final APIs:

- `src/tool/task.rs` — convergence actions routed through a coordinator;
- new `src/agent/convergence.rs` or equivalent application coordinator;
- `assets/agents/verifier.toml` and `assets/prompts/agents/verifier.md` (exact name may differ);
- generated built-in agent output via the existing script;
- tool/agent factory wiring for convergence store/service access;
- session projection DTO/reducer/event adapter;
- TUI run/agent sidebar or detail rendering for bounded status;
- `architecture/agent.md`, `architecture/tool.md`, `architecture/goal.md`, and built-in-agent docs.

Do not modify legacy team inbox/outbox code or scheduler resource algorithms.

## 12. Required tests

### Tool/action authorization and idempotency

- turn-owned `converge` produces one convergence + one producer on retry;
- nested run-owned convergence assigns direct owner correctly;
- unrelated sibling/forged run cannot status/decide/cancel if current run-control policy would deny it;
- same invocation identity with changed request fingerprint fails closed;
- `repair`/`replan` actions are rejected in M002.

### Verifier safety

- effective verifier permissions exclude all mutating/shell/Git-write/delegation/goal-completion tools even when a custom override tries to allow them;
- verifier cannot answer a pending permission request;
- verifier receives producer result evidence but not producer reasoning/transcript;
- malformed verdict never becomes pass.

### Lifecycle

- successful producer -> verifier -> awaiting decision -> accept;
- verifier revise -> awaiting decision -> stop/escalate;
- producer failure/cancel/timeout;
- verifier failure/cancel/timeout;
- user cancel while producer active;
- user cancel while verifier active;
- decision/cancel race;
- daemon restart between producer completion and verifier submission;
- daemon restart after verifier completion before decision notification;
- detached restart uses persisted objective/criteria without transcript reconstruction;
- no duplicate producer/verifier runs on reconciliation.

### Goal authority

- semantic `Pass` plus failed host test cannot yield goal `Met`;
- semantic `Pass` cannot directly mutate `GoalStatus`;
- normal host goal completion path remains unchanged for non-convergence work.

### Projection

- replay/resync yields same convergence summary;
- projection bounds findings to counts/summaries;
- hidden reasoning and secrets absent.

## 13. Verification commands

Required after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core agent_convergence --locked
cargo test -p codegg --lib convergence --locked
cargo test --test agent_convergence --locked
python3 scripts/check_builtin_agents.py
```

During implementation, regenerate built-in agent output with the repository's normal `python3 scripts/generate_builtin_agents.py` workflow after changing source TOML/prompt assets. Run focused goal-verification regressions touched by the integration and the existing narrow task/run tests needed to prove no ownership regression.

Then:

```bash
scripts/verify.sh quick
```

No live MiniMax/OpenAI/Anthropic provider call is required for closure. Use deterministic mock/capture providers for verifier result tests.

## 14. Acceptance criteria

M002 may close only when:

1. `task converge` (or the accepted equivalent) uses the normal durable agent-run/scheduler submission path.
2. One producer run is idempotently associated with one convergence cycle.
3. Producer completion is consumed from authoritative run/group state; no polling loop is required for correctness.
4. One fresh independent verifier run is created only after reviewable producer evidence exists.
5. The verifier has a host-enforced read-only ceiling that custom agent configuration cannot widen.
6. The verifier receives bounded structured host/run evidence and not the producer's full transcript/hidden reasoning.
7. Verifier output is a typed bounded `Pass | Revise | Inconclusive` verdict; malformed output fails closed.
8. The exact owner can make one durable `accept | stop | escalate` decision.
9. `accept` does not merge Git or complete a goal.
10. Semantic `Pass` cannot override a failed/missing deterministic host check.
11. Cancellation/restart/duplicate-notification behavior is deterministic and does not duplicate child model work.
12. Detached/restarted verification uses the durable convergence spec rather than transcript scraping.
13. Projection/TUI state is derived from durable convergence/run records and bounded.
14. Legacy `task spawn`/group behavior remains compatible.
15. Focused tests and `scripts/verify.sh quick` pass.

## 15. Stop conditions

Stop and register a corrective/additional plan if:

- safe verifier execution would require giving it mutation authority;
- the coordinator must construct raw `AgentLoop` instances or bypass scheduler-owned delegation;
- a verifier needs full unredacted producer transcripts to function;
- passing semantic review must change deterministic goal-verification semantics to make the feature usable;
- restart requires rerunning a terminal producer/verifier because durable provenance/specification is insufficient;
- extending the `task` schema creates an unavoidable compatibility break that justifies a separate orchestration tool surface.

## 16. Closure evidence required

Create `plans/closure/agent-convergence/002-status.md` with:

- implementation commit(s) and reviewed M001 dependency revision;
- producer/verifier/owner state-machine evidence;
- verifier effective-permission matrix;
- durable-spec/restart and evidence-packet/redaction proof;
- duplicate/restart/cancellation race results;
- goal-authority regression evidence;
- projection/replay evidence;
- exact focused and quick verification results;
- unresolved findings and recommendation.

Only accepted M002 closure moves M003 to `ready`.
