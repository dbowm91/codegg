# Runtime Consolidation, Deletion, and Footprint M003 — AgentLoop Ownership Decomposition

Status: blocked

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- `architecture/agent.md`
- `architecture/tool.md`
- `architecture/scheduler.md`
- M002 structured outcome/recovery contract

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Primary class: infrastructure / maintainability with correctness preservation

Dependencies:

- hard: M002 closed;
- soft: M001 and M004 should preferably land first because their deletions reduce the amount of legacy state that would otherwise be moved;
- interface: PromptCompiler/ContextPlan, ToolBroker, permission checker, provider adapter, compaction/context types, scheduler/subagent services remain authoritative.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/003-status.md`

## 1. Objective

Reduce `src/agent/loop.rs` from a multi-policy god file into a small turn orchestration driver that coordinates existing canonical subsystem owners.

This is not a rewrite and not an abstraction exercise. Success is measured by fewer policy owners, smaller review surfaces, less duplicated state, and unchanged behavior.

## 2. Current implementation evidence

At the reviewed baseline `src/agent/loop.rs` is roughly 287 KiB and directly contains or coordinates implementation details for:

- provider streaming/retry and event handling;
- context packing observation, cache-cost analysis, tool-palette reduction/backoff;
- prompt/profile/model flags;
- compaction;
- permission handling;
- native/MCP/question tool dispatch;
- per-tool timeout policy;
- tool execution context construction;
- progress recovery and follow-up continuation;
- snapshots/file mutation handling;
- plugin/hooks;
- history repair/provider compatibility;
- usage/pricing/security findings;
- goals/todos/task state;
- notifications;
- steering/cancellation;
- event/public terminal projection.

Large neighboring files (`agent/mod.rs`, `compaction.rs`, `prompt.rs`) increase review coupling. The goal is not merely to move LOC from one giant file into another giant file.

## 3. Explicit non-goals

Do not:

- invent a generic actor, middleware, workflow, command bus, event sourcing, service locator, or dependency injection framework;
- rewrite provider clients or Tool Broker;
- change tool schemas, user-facing prompts, session protocol, ACP, scheduler semantics, or database schema;
- split CodeGG into more crates merely to satisfy LOC targets;
- create traits where one concrete owner is sufficient;
- move code without deleting duplicate policy/state;
- combine this milestone with broad naming/style cleanup.

## 4. Target ownership model

`AgentLoop` should retain only turn-level mutable state and orchestration sequencing.

Expected concrete ownership domains:

1. **Tool batch executor** — permission resolution, execution context, broker/MCP dispatch, timeout/cancellation normalization, result ordering, snapshot/effect collection.
2. **Context/prompt runtime** — context-plan application, packer observation/policy, cache stats, compaction interaction, base tool surface restoration.
3. **Provider turn adapter** — provider request/stream normalization, adapter-owned repair/wire compatibility, stop-reason normalization.
4. **Autonomy/recovery owner** — M002's final state machine and observation handling.

These may be modules/private structs, not necessarily traits. Existing subsystem types should be reused before creating new ones.

The final loop should read conceptually as:

```text
prepare turn
while within limits:
  prepare provider request/context
  receive normalized provider turn
  if final -> finish
  execute structured tool batch
  observe effects/recovery
  continue/replan/stall
finalize
```

## 5. Ordered work packages

### A. Responsibility inventory and move order

1. Produce a temporary implementation inventory of every `AgentLoop` field/helper and assign one owner: loop, tool execution, context runtime, provider adapter, recovery, or existing external subsystem.
2. Mark fields/helpers that are duplicated, transitional, or removable rather than movable.
3. Identify call cycles before extracting modules.
4. Choose extraction order that keeps each commit compiling and behaviorally equivalent.

Do not add this inventory as permanent architecture documentation unless it captures durable ownership rather than transient file mapping.

### B. Extract tool batch execution

Move tool-specific mechanics out of `loop.rs` behind one concrete internal owner:

- permission outcome normalization;
- per-call execution context construction;
- timeout lookup;
- native ToolBroker execution;
- MCP/question compatibility dispatch;
- result ordering;
- structured status/effect normalization from M002;
- snapshot/effect facts needed by the loop.

Where possible, move hard-coded per-tool timeout/default/effect metadata into existing tool contract/catalog metadata instead of another match table. Do not move user-configured global timeout semantics out of configuration ownership.

Return one typed batch result to the loop.

### C. Extract context/prompt policy runtime

Move:

- context packer observation;
- effective cache-cost analysis;
- tool-palette reduction/backoff;
- base tool-surface state;
- context plan cache identity;
- context cache stats interactions;
- compaction coordination directly tied to context policy.

The extracted owner must consume PromptCompiler/ContextPlan outputs rather than become another prompt compiler.

Hard-coded curated/minimal tool name arrays should be replaced with existing catalog/contract capability metadata if that can be done narrowly and without widening scope. If no suitable metadata exists, keep the list but place it in one documented owner rather than duplicating it.

### D. Move provider compatibility to adapter-owned boundary

Move model/provider-specific request/response repair and stop-reason normalization out of the generic orchestration path when an adapter boundary already exists.

The loop may ask an adapter to normalize a provider turn. It should not contain model-family name checks or parse arbitrary prose itself.

Do not change provider network clients unless necessary to expose already-existing normalized facts.

### E. Reduce `AgentLoop` state

After extraction:

- remove fields now owned by helper runtimes;
- replace related clusters with one owned component where appropriate;
- remove duplicated counters/cache fields whose authoritative state exists elsewhere;
- retain explicit workspace/session/agent identity and turn-level limits/state.

Avoid `Arc<Mutex<...>>` additions unless actual concurrent shared ownership requires them. Most per-turn components should remain ordinary owned mutable state.

### F. Test behavior equivalence

Before and after each major extraction, preserve focused behavior tests for:

- primary tool-call loop;
- follow-up continuation;
- structured recovery/stall;
- permission denial;
- timeout/cancellation;
- MCP result handling;
- context/tool-palette reduction/backoff;
- compaction trigger;
- snapshot around file mutation;
- steering/cancellation;
- goal/todo accounting touched by moved code.

Prefer existing harness tests. Add new tests only for a previously untested boundary needed to make extraction safe.

### G. Architecture documentation

Rewrite `architecture/agent.md` to describe durable owners and the main turn sequence. Remove stale field-by-field `AgentLoop` inventories and historical names such as superseded doom-loop machinery.

Keep detailed type/function behavior in Rustdoc/source rather than mirroring it in Markdown.

## 6. Concurrency, cancellation, restart, failure semantics

Concurrency:

- preserve parallel tool batch semantics and result association/order;
- do not introduce shared locks merely because code moved modules;
- existing semaphore/resource limits remain authoritative.

Cancellation:

- steering, turn cancellation, tool cancellation, and provider cancellation retain existing propagation ordering;
- helper components must not swallow cancellation into generic errors.

Restart:

- no new durable state;
- runtime components reconstruct from existing turn/runtime inputs.

Failure:

- helper errors remain typed enough for the loop to decide fail/continue/recover;
- extraction must not replace structured results with strings.

## 7. Verification

Expected focused verification after relevant extractions:

```bash
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Run context/MCP/permission-focused tests when those modules move.

No new hosted lane or all-features campaign is required for M003.

## 8. Explicit acceptance criteria

M003 is complete only when:

1. `AgentLoop` is primarily a turn orchestration driver, not the implementation owner for tool execution, context policy, provider compatibility, and recovery simultaneously.
2. Tool batch execution has one concrete typed boundary returning structured outcomes/effects from M002.
3. Context packer/palette/cache policy has one concrete owner outside the main loop body.
4. Provider/model-specific repair logic is adapter-owned where an adapter exists; generic loop code contains no new model-family heuristics.
5. `AgentLoop` field count/state is materially reduced and duplicated policy/cache/counter state is removed rather than merely renamed.
6. No new generic framework or gratuitous trait hierarchy was introduced.
7. No new `Arc<Mutex<...>>`/global state was introduced without demonstrated concurrency need.
8. Existing primary/follow-up/subagent behavior, permission semantics, snapshots, context handling, cancellation, and recovery remain covered by passing focused tests.
9. Structured execution results are not downgraded to strings at the new module boundaries.
10. `architecture/agent.md` documents durable ownership and sequence rather than a stale struct inventory.
11. Workspace Clippy and `scripts/verify.sh quick` pass.
12. The implementation report includes before/after file-size/LOC or equivalent coarse evidence showing that `loop.rs` was materially reduced; there is no hard numeric CI threshold.
13. No single newly extracted module simply recreates the same 287 KiB multi-domain ownership problem.

## 9. Stop conditions

Stop and split a follow-up if extraction reveals a genuine architecture decision involving public protocol, storage schema, scheduler authority, or provider API. Do not use M003 as authorization for a broad runtime rewrite.
