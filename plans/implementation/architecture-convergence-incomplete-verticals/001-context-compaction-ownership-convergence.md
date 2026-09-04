# Architecture Convergence M001 — Context and Compaction Ownership Convergence

Status: closing

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable dependencies:

- runtime consolidation M010 closed;
- agent-runtime correctness M013 closed;
- provider session-context M009 closed;
- no hard dependency on other milestones in this roadmap.

Primary class: infrastructure / polish

## 1. Objective

Make context selection, token-budget policy, compaction, bounded summary production, and context-policy state have one canonical production owner. The implementation should converge existing `eggcontext`, root `src/context`, and agent-local context/compaction machinery rather than introduce a new abstraction.

The desired shape is:

```text
AgentLoop / agent runtime
        |
        v
root context adapter
        |
        v
eggcontext canonical policy/runtime
        |
        +--> bounded context result
        +--> compaction request/result
        `--> diagnostics/metrics
```

Agent-specific turn orchestration may decide when context preparation is required, but it must not independently implement token policy or compaction semantics.

## 2. Explicit non-goals

M001 must not:

- redesign provider APIs or model adapters;
- add another memory system, vector store, skill store, or transcript store;
- change user-visible context limits merely to simplify code;
- remove provider-specific token accounting when genuinely required by transport/model contracts;
- introduce a new persistence database for context state;
- change hidden-reasoning handling or expose provider-private content;
- add a new CI/benchmark gate.

## 3. Current implementation evidence to inspect

The implementation agent must re-inspect at least:

- `crates/eggcontext/`;
- `src/context/`;
- `src/agent/compaction.rs`;
- `src/agent/context_frame.rs`;
- `src/agent/context_runtime.rs`;
- `src/agent/loop.rs` call sites that prepare, compact, retry, or trim context;
- provider capability/token-limit metadata;
- persisted session/history interfaces used by compaction;
- direct provider callers performing asynchronous compaction.

Do not assume that every similarly named type is duplicate. Classify each production path as canonical owner, adapter, compatibility path, or dead/duplicate path before deleting code.

## 4. Required ownership contract

By milestone end, one documented owner must provide:

- effective model/context capacity calculation;
- reserved-output/tool budget handling;
- context-item selection/truncation policy;
- compaction trigger policy;
- compaction input construction;
- bounded compaction result contract;
- failure/fallback classification;
- cacheable/durable metadata needed to avoid redoing work incorrectly;
- provider request context propagation for model-backed compaction.

The root/agent layer may retain:

- turn-specific sequencing;
- UI/projection messages;
- conversion between CodeGG session/history types and the canonical context API;
- orchestration decisions such as “retry after compaction.”

It must not retain a second independently evolving token or compaction policy.

## 5. Ordered work packages

### WP1 — Production-path inventory

Create a bounded inventory in the implementation notes or architecture doc that lists every production call path for context preparation and compaction. For each path, record owner, caller, provider interaction, persistence interaction, and whether it is synchronous/asynchronous.

Required outcome: no production context/compaction path remains unclassified.

### WP2 — Canonical API selection

Prefer extending `eggcontext` if it already owns the relevant policy semantics. If current crate boundaries make that impossible without creating circular dependencies, keep CodeGG-specific adapters in the root or `codegg-core`, but retain exactly one policy implementation.

The API should return typed results rather than mutating AgentLoop internals. A result should distinguish at least:

```text
ready
compaction_required
compacted
insufficient_capacity
provider_failure
invalid_history_or_budget
cancelled
```

Exact names may differ.

### WP3 — Move/delete duplicate policy

Migrate duplicate token/selection/compaction logic from `src/agent` and `src/context` into the selected owner. Delete helpers that only preserve an older parallel implementation once callers are migrated.

Do not split large files by mechanical extraction alone. A moved function must land under the owner of the state/policy it implements.

### WP4 — Preserve provider/session context

Every model-backed compaction request must preserve the owning session/run request context established by provider M008/M009. Stable provider affinity for one compaction operation must remain stable across its request phases where current provider policy requires it.

Add focused regression coverage for direct compaction provider calls.

### WP5 — AgentLoop adapter cleanup

Reduce AgentLoop/context-runtime code to typed requests/results and sequencing. Remove mutable shared policy state from AgentLoop where the canonical owner can own it safely.

### WP6 — Documentation

Update `architecture/agent.md` and any context architecture documentation so there is one explicit answer to:

- who owns token/context budget policy;
- who owns compaction;
- what AgentLoop owns;
- which compatibility adapters remain and why.

## 6. Storage, protocol, migration, compatibility

No protocol change is expected. No schema migration is expected unless current compaction metadata is duplicated across persistence owners and a migration is required to establish one canonical record. If a schema change becomes necessary, keep it additive/bounded and document restart compatibility.

Existing history/session data must remain readable. Do not rewrite historical transcripts merely to adopt a new context representation.

## 7. Security, cancellation, failure semantics

Compaction must not persist or expose hidden reasoning, credentials, raw sensitive tool arguments, or other content currently excluded from public/history storage.

Cancellation must abort provider-backed compaction and return a typed cancelled outcome rather than detach work.

Provider failure must not silently discard required context. Existing conservative fallback behavior should be preserved or made more explicit.

## 8. Verification

Focused verification should include the owning `eggcontext`/context tests plus agent compaction/provider-context tests. At minimum run the narrowest relevant package/test targets, then:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add a new benchmark or CI lane. If size/LOC changes are useful, record them as observational evidence only.

## 9. Static guards and regression coverage

Add or adjust a lightweight static guard only if current repository guard conventions can enforce one meaningful invariant such as “AgentLoop does not call provider compaction directly outside the canonical adapter.” Do not create a broad brittle grep framework.

Tests must cover:

- identical effective budget behavior before/after migration for representative models;
- compaction provider request context propagation;
- cancellation;
- insufficient-capacity/failure fallback;
- no duplicate compaction owner on normal production paths.

## 10. Acceptance criteria

M001 is complete only when:

- one canonical production owner for context/compaction policy is documented and used;
- all production context/compaction paths are classified;
- duplicate policy implementations are deleted or explicitly retained as bounded compatibility adapters;
- AgentLoop consumes typed context results instead of owning policy details;
- provider session/run context propagation remains correct;
- focused tests and repository quick verification pass;
- no new memory/context runtime or verification framework was introduced.

## 11. Stop conditions

Stop and record a dependency/ADR need if implementation requires changing provider authority, durable session/history ownership, public protocol semantics, or hidden-reasoning policy.

Do not mark the milestone complete if code was merely moved between files while parallel policy implementations remain.

## 12. Closure evidence required

The closure record must include:

- implementation commit(s);
- before/after ownership map;
- deleted/retained compatibility paths with rationale;
- requirement-to-test evidence;
- provider-context regression evidence;
- verification commands/outcomes;
- known limitations and any remaining duplicate path classified by severity.
