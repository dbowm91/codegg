# Architecture Convergence M004 — AgentLoop Coordinator Reduction

Status: active

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Hard dependencies:

- M001 context/compaction ownership convergence must close;
- M002 process/tool execution ownership convergence must close;
- M003 Git ownership convergence must close.

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Primary class: infrastructure / polish

## 1. Objective

Reduce `AgentLoop` to a coordinator over canonical context, provider, tool-execution, Git/worktree, recovery/convergence, persistence, and projection services. This is not a mechanical file split. It is complete only when policy and mutable state have moved to their proper owners and the loop primarily sequences typed operations/results.

Target conceptual flow:

```text
turn admission/context
      |
      v
context preparation
      |
      v
provider invocation
      |
      v
tool-call dispatch/execution
      |
      v
progress/recovery/convergence
      |
      v
persistence/projection/completion
```

The loop owns the sequence and turn lifecycle. It does not independently own the policies behind each box.

## 2. Explicit non-goals

M004 must not:

- redesign agent-run persistence or scheduler admission;
- introduce a new actor/event framework solely to shrink a file;
- create a generic workflow engine;
- reimplement context/process/Git policy in temporary helper modules;
- change tool/provider behavior unless required to preserve typed service contracts;
- alter host-owned goal verification authority;
- use LOC reduction as the sole acceptance metric.

## 3. Current implementation evidence to inspect

Re-inspect:

- `src/agent/loop.rs`;
- `src/agent/mod.rs`;
- `src/agent/processor.rs` and other turn/message processors;
- progress/recovery/convergence services;
- run-control and durable run/result code;
- context APIs after M001;
- process/tool APIs after M002;
- Git/worktree APIs after M003;
- provider call wrappers and session context;
- session/history persistence and projection emission.

Map mutable fields and major methods by semantic owner before editing.

## 4. Required coordinator contract

By milestone end, `AgentLoop` should own only state that is intrinsically one-loop/one-turn orchestration state, such as:

- current turn/run identity;
- current lifecycle phase;
- references/handles to canonical services;
- bounded transient sequencing state;
- cancellation/steering checkpoints needed to coordinate phases.

It should not own duplicate:

- token/context policy;
- subprocess lifecycle policy;
- Git provenance/worktree policy;
- provider transport policy;
- durable completion authority;
- scheduler admission;
- tool authorization/registry state;
- convergence state machine internals.

## 5. Ordered work packages

### WP1 — Field/method ownership map

Classify `AgentLoop` fields and major method groups as coordinator-owned, service-owned, compatibility-only, or dead. Use this map to drive extraction/deletion.

### WP2 — Introduce typed phase boundaries

Where current code passes broad mutable loop state into helpers, define narrow typed requests/results or service methods. Avoid “god context” structs containing the entire loop.

### WP3 — Remove service-policy leakage

Migrate remaining context/process/Git policy calls to the owners closed by M001-M003. Also identify provider/recovery/persistence logic that already has a canonical owner but is still duplicated in the loop.

### WP4 — Collapse duplicate result/recovery branches

Audit success/error/tool-result/continuation branches for equivalent state transitions represented in several places. Route them through existing typed outcome/recovery structures rather than retaining parallel string/boolean state.

### WP5 — Compatibility cleanup

Delete dead/deprecated branches whose supported callers were already removed by prior consolidation work. Do not remove externally supported compatibility merely because it is inconvenient.

### WP6 — Module boundary cleanup

After semantic ownership is correct, split remaining large coordinator code into lifecycle-focused modules if that improves comprehension. Prefer modules such as turn lifecycle, provider phase, tool phase, and completion phase over arbitrary size-based chunks.

### WP7 — Documentation

Update `architecture/agent.md` with the final coordinator/service diagram and explicit ownership table.

## 6. Storage, protocol, migration, compatibility

No storage/protocol migration is expected. Durable runs, results, session events, tool outcomes, and goal verification must remain wire/storage compatible.

Behavioral equivalence for ordinary turns is required. Intentional deletion of unreachable compatibility code must be backed by caller evidence.

## 7. Concurrency, cancellation, failure semantics

Preserve existing cancellation, steering, safe-boundary run control, tool cancellation, and provider interruption semantics. Extraction must not introduce detached tasks whose lifecycle outlives the owning turn without durable ownership.

Failures must remain typed enough that recovery/convergence can distinguish retryable provider/tool/process failures from terminal policy or user-denied outcomes.

## 8. Verification

Add/retain focused tests around:

- ordinary text-only turn;
- provider tool call -> tool result -> continuation;
- tool failure/recovery;
- cancellation during provider and tool phases;
- compaction retry;
- delegated/convergence interaction;
- goal completion handoff.

Then run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

No new harness or benchmark gate.

## 9. Acceptance criteria

M004 is complete only when:

- M001-M003 are closed and their canonical owners are consumed;
- AgentLoop policy ownership is materially reduced rather than relocated into adjacent root helpers;
- major lifecycle phases communicate through narrow typed contracts;
- duplicate outcome/recovery branches are removed where current canonical structures already exist;
- behavior/cancellation/goal-verification regressions are covered;
- architecture docs identify one owner per phase;
- focused and quick verification pass.

A reduction in `loop.rs`/`agent/mod.rs` size should be recorded, but there is no arbitrary target percentage.

## 10. Stop conditions

Stop if a required extraction would change scheduler authority, agent-run identity, provider ownership, tool authorization, or goal-verification authority. Record a follow-up architecture decision instead.

## 11. Closure evidence required

Record:

- field/method ownership map before and after;
- implementation commits;
- resulting phase/service diagram;
- deleted compatibility/duplicate branches;
- representative turn/cancellation/recovery evidence;
- file/module size observations;
- verification outcomes and remaining findings.
