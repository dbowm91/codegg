# Decision-Model Extraction and Runtime Milestone 001 — Decision Contract and CodeGG Adapter

Status: ready for handoff

Repository baseline: `533be5941ac48334743de555cda96d8695cc6527`

Source roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m001--backend-neutral-decision-contract-and-codegg-adapter-seam`

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — authority/fallback/consent invariants remain binding.

Primary class: infrastructure

Hard dependencies: none.

## 1. Objective

Introduce the smallest backend-neutral decision contract CodeGG needs so the agent/tool-selection path can consume bounded probabilistic decisions without depending on a particular encoder, tokenizer, artifact format, training framework, or external service.

This milestone establishes types, validation, backend capabilities/state, a no-op implementation, CodeGG tool-selection adapters, and frozen compatibility fixtures. It does **not** switch live disclosure/reranking to a new backend.

## 2. Why this milestone is ready

The current repository already has the necessary source semantics:

- `ResolvedToolSurface` is the policy-filtered capability ceiling.
- `src/tool_advisor/mod.rs` contains versioned case/prediction and runtime concepts.
- `src/tool_advisor/context_v2.rs` demonstrates bounded context projection.
- existing runtime/qualification code distinguishes scoring, abstention, mode, latency, artifact version, and fallback.
- ADR-0009 already proves a learned component can be advisory without owning authorization.

The missing seam is that those concepts are still expressed as a tool-advisor/model-specific subsystem. M001 can extract the application semantics without choosing the external runtime repository or changing live behavior.

## 3. Invariants that must not regress

- `ResolvedToolSurface` remains the only candidate/authority source for tool decisions.
- No decision response can register, enable, authorize, execute, or synthesize a tool.
- `off`/no-backend behavior remains byte-for-byte or semantically equivalent to current deterministic request preparation.
- No network access, model loading, or training dependency is added by this milestone.
- The causal-frontier path remains unchanged and does not get forced through learned-model semantics.
- Existing frozen tool-advisor datasets and qualification assets are not rewritten.
- New generic types are bounded and reject oversized/duplicate/invalid inputs before backend invocation.
- The contract distinguishes multi-relevance ranking from mutually exclusive choice.

## 4. Scope

### In scope

- A small CodeGG-owned decision domain module/crate with:
  - decision request id and schema version;
  - bounded state projection;
  - Binary, Choice, Score, and Rank specifications;
  - bounded candidates/options with stable application ids;
  - backend capability declaration;
  - response status: answered, abstained, unsupported, unavailable/degraded;
  - per-option/candidate probabilities or scores where applicable;
  - backend/model/runtime provenance and latency.
- Validation and canonical ordering/fingerprinting.
- A `DecisionEngine` trait or equivalent asynchronous interface.
- `NoopDecisionEngine`.
- Adapter construction from policy-allowed tool descriptors/context into Rank requests.
- Adapter conversion from existing advisor predictions into compatibility responses for tests only.
- Frozen compatibility fixtures under a new decision-runtime asset namespace.
- Documentation of the new ownership boundary.

### Explicitly out of scope

- Moving training code.
- Importing an external decision-model crate.
- System One HTTP.
- Model download.
- Changing `tool_search` ordering or proactive disclosure.
- Retiring existing advisor runtime types.
- New model qualification.
- Causal-frontier changes.

## 5. Required production changes

### Core/domain

Prefer a dependency-light module in `codegg-core` or another existing lower-level crate if current dependency direction permits it. Do not place the durable contract under a TUI/provider module.

The exact Rust layout may adjust after inspection, but the domain boundary must not depend on:

- Candle or another ML framework;
- `src/tool_advisor` implementation modules;
- provider HTTP code;
- TUI/server presentation code.

The contract should use stable application ids rather than model-visible names as identity. Model-visible labels/descriptions are bounded payload.

### Decision validation

Validation must enforce:

- non-empty request id/version;
- bounded state bytes/field counts;
- bounded question count;
- unique question ids;
- Choice option count and uniqueness;
- Rank candidate count and uniqueness;
- finite numeric scores/probabilities;
- probability range where a backend claims probabilities;
- response ids exactly correspond to request ids/candidates;
- unsupported semantics are explicit, not encoded as empty success.

Define constants in one owner; do not scatter backend-specific limits into agent-loop code.

### CodeGG tool adapter

Create one adapter that takes already-resolved tool candidates plus the existing bounded task/context projection and emits a Rank request.

The adapter must not query registries independently. It consumes its candidate list from the caller.

A reverse adapter for compatibility testing may translate an existing `ToolAdvisorPrediction` into the generic response shape. It is not a new actuation owner.

### Backend state

Define observable backend state such as:

- Off;
- Ready;
- Degraded(reason);
- Unsupported(reason);
- Unavailable(reason).

Do not bake local artifact paths or HTTP endpoints into the generic trait.

### Storage and migration

None. The compatibility fixtures are repository assets, not user state.

### Configuration

No new user-visible backend configuration is required in M001. Existing advisor configuration remains authoritative.

### Concurrency/cancellation

The trait must be async/cancellable through the existing request/task lifetime. A backend call receives or derives a bounded deadline rather than owning an unbounded retry loop.

M001's Noop implementation returns immediately.

## 6. Frozen compatibility fixtures

Add a small `assets/decision-runtime/` fixture set representing:

- Binary yes/no;
- exclusive Choice;
- ordinal Score;
- Rank with one relevant candidate;
- Rank with multiple relevant candidates;
- Rank with no relevant candidate/abstention;
- unknown/synthetic candidate names;
- candidate-order permutations;
- malformed/oversized input rejection;
- backend unsupported/unavailable states.

At least several Rank fixtures must be derived from existing frozen tool-advisor cases without changing their historical files. Record source case ids and source fingerprints.

If a deterministic existing legacy scorer is used for output fixtures, record its implementation/artifact identity and tolerance policy. Do not freeze platform-sensitive timing.

## 7. Ordered work packages

### A — Domain contract and validation

Implement the semantic enum/specification, bounded state/candidate/options, response/status/provenance types, and validation.

Acceptance: exhaustive unit tests cover each semantic family and malformed bounds.

### B — Backend trait and Noop

Add `DecisionEngine` plus capability/state introspection and Noop behavior.

Acceptance: Noop requires no I/O and returns explicit unavailable/off semantics within the caller lifetime.

### C — Tool-selection adapter

Project the existing policy-filtered tool candidate descriptors/context into Rank requests.

Acceptance: denied/hidden/disabled/parent-ceiling-ineligible tools cannot appear when the adapter is fed the real resolved surface; candidate identity/order is deterministic.

### D — Compatibility fixture generator/checker

Create committed fixtures and a test/CLI path that validates them without ML features.

Acceptance: fixture fingerprints are deterministic and run in the default build/test graph.

### E — Documentation

Update `architecture/tool-advisor.md` with the new boundary and note that historical model-specific runtime remains temporarily authoritative until M003/M005.

## 8. Failure and recovery semantics

Malformed requests fail before backend dispatch. Unsupported backend semantics produce explicit status and preserve deterministic fallback. Backend panics/errors must be contained by the eventual caller boundary; M001 must not introduce a global process failure path.

No durable state is created, so restart recovery is trivial.

## 9. Compatibility and migration

This milestone is additive. Existing tool-advisor config, runtime, CLI, features, artifacts, and tests remain valid.

New types must be versioned from v1 and designed so external runtime adapters can be added without changing CodeGG's tool authority types.

## 10. Required tests

Focused tests:

- request validation for all semantic kinds;
- duplicate/oversized/malformed rejection;
- response-request identity matching;
- multi-relevance Rank semantics;
- explicit unsupported/unavailable handling;
- Noop behavior;
- tool-adapter authority-negative cases;
- candidate-order/fingerprint determinism;
- compatibility fixture round-trip.

Broad tests:

- existing tool-advisor focused suite;
- existing resolved-tool-surface/policy tests;
- workspace default-feature tests relevant to modified crates;
- static architecture/core-boundary guards.

## 11. Required verification commands

Use implemented package names after inspection. At minimum record:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --locked -p codegg-core
cargo test --locked -p codegg --lib tool_advisor
scripts/verify.sh quick
git diff --check
```

If the new module lands outside `codegg-core`, substitute its package and record why.

## 12. Documentation and registry updates

- `architecture/tool-advisor.md`
- `plans/subsystems/decision-model-extraction-runtime-roadmap.md`
- `plans/registry.md`
- any architecture dependency map that currently implies CodeGG owns learned model architecture.

Do not rewrite historical closure records.

## 13. Acceptance criteria

M001 closes only when:

1. one bounded model-neutral decision contract exists below tool-advisor implementation code;
2. Binary/Choice/Score/Rank are semantically distinct;
3. unsupported/unavailable/abstention are explicit;
4. Noop/default operation requires no network/model;
5. the real CodeGG tool surface can be projected without widening authority;
6. frozen compatibility fixtures exist and run without ML features;
7. no live rerank/promotion behavior changes.

## 14. Stop conditions

Stop and write a corrective/new ADR if:

- the contract requires model-framework types;
- Rank must be coerced into exclusive choice;
- the backend needs direct access to `ToolRegistry`/permissions to function;
- M001 begins moving training/runtime code;
- default behavior changes;
- generic state becomes an unbounded arbitrary transcript dump.

## 15. Closure evidence required

- implementation commit(s);
- type/validation test matrix;
- fixture fingerprint list and source provenance;
- authority-negative evidence;
- default/no-I/O evidence;
- dependency diff showing no ML/network dependency added;
- verification command results;
- unresolved findings by severity;
- dependency audit for M002/M004.
