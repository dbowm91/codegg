# Decision-Model Extraction and Runtime Roadmap

Status: active

Repository audit baseline: `533be5941ac48334743de555cda96d8695cc6527`

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Accepted architecture:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — retained for authority/fallback/consent invariants, partially superseded for generic training/runtime ownership.

Historical evidence consumed, not reopened:

- `plans/subsystems/tool-selection-advisor-roadmap.md`
- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md`
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md`
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`
- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md`
- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`
- `architecture/tool-advisor.md`
- `architecture/tool-advisor-framework-spike.md`

## 1. Purpose and ownership boundary

This subsystem separates generic small-decision-model research from CodeGG's coding-agent runtime.

It owns the CodeGG side of:

- a backend-neutral bounded decision request/response contract;
- the adapter from CodeGG tool-selection state into that contract;
- compatibility fixtures needed to prove an external runtime before migration;
- replacement of architecture-specific in-repo learned inference with a reusable runtime backend;
- optional System One-compatible backend integration;
- migration of learned tool advice to the new boundary;
- final retirement of generic training/model-framework code from CodeGG once parity and fallback are proven.

It coordinates, but does not permanently own, the external decision-model repository that will contain generic model training/evaluation/runtime machinery.

It does not move CodeGG authority, causal tool contracts, `ResolvedToolSurface`, deterministic discovery fallback, disclosure policy, or agent-loop actuation outside CodeGG.

## 2. Why this line exists now

The original tool-advisor roadmap closed successfully as an experimental capability boundary, but subsequent architecture work accumulated into a large model-research subsystem:

- `src/tool_advisor/` contains training, contextual scorers, sequence encoders/rankers, retrieval projections, late interaction, qualification, requalification, and causal-frontier code;
- experiment assets and closure evidence span multiple generations;
- optional Candle dependencies and four advisor/training feature combinations remain in the root package;
- several learned architectures closed negatively or remained unqualified for live use;
- the deterministic causal-frontier successor is useful but CodeGG-specific.

No qualified learned backend is required by normal CodeGG operation. That makes extraction possible without migrating a mandatory production dependency.

## 3. Durable invariants

- CodeGG works normally with no decision model, model artifact, training repository, external service, or network connection.
- `ResolvedToolSurface` and existing permission/broker paths remain the capability ceiling.
- A decision engine can rank or recommend only application-supplied candidates.
- Decision output is advisory until CodeGG's own policy/actuation layer applies it.
- Backend failure, timeout, incompatibility, or unsupported semantics falls back to the deterministic path.
- The default build remains free of model weights and, after final migration, training-only ML frameworks.
- CodeGG-specific causal-frontier state/contracts remain local and unchanged unless separately planned.
- Historical frozen assets and closure records are never rewritten to claim new provenance.
- Remote backend use is explicit and never implied by local advisor configuration.
- Export/capture consent is separate from inference-backend selection.

## 4. Non-goals

- Choosing or naming the external repository in this roadmap.
- Bundling a default model.
- Reopening negative sequence/retrieval/late-interaction experiments.
- Training a new architecture inside CodeGG.
- Replacing deterministic BM25/tool discovery.
- Moving execution permission, tool broker, or disclosure budgets into a model runtime.
- Making System One the internal CodeGG protocol.
- Moving the causal-frontier implementation out of CodeGG.
- Deleting historical qualification evidence as cleanup.

## 5. Target architecture

```text
CodeGG typed state / ResolvedToolSurface
                |
                v
       CodeGG decision adapter
                |
        DecisionRequest/Spec
                |
                v
         DecisionEngine
       /       |        \
      /        |         \
   Noop   Local artifact  System One
            runtime       adapter
              ^              |
              |              +--> Jev / compatible endpoint
              |
 external decision-model repository
 training / evaluation / artifact/runtime releases
```

CodeGG owns the left side and the actuation after the response. The external repository owns generic model construction and the reusable local runtime implementation. A network adapter is optional and must preserve the same bounded application semantics.

## 6. Decision semantic contract

The first contract must cover enough semantics to avoid encoding tool selection as a special case:

- `Binary`: probability over true/false plus confidence/abstention metadata;
- `Choice`: bounded mutually exclusive named options;
- `Score`: bounded ordinal/rubric output;
- `Rank`: bounded candidates with independent relevance/ranking scores and optional multi-relevance;
- `Abstain/Unsupported/Unavailable`: explicit non-answer states.

Every request carries a versioned structured state projection and explicit size bounds. Every response identifies backend/model/runtime version and latency. CodeGG policy decides whether a response is usable.

System One can serve Binary/Choice/Score where mappings are lossless. Rank is not forced through Choice if the application semantics allow multiple relevant tools.

## 7. Dependency graph

```text
ADR-0013 accepted
      |
      v
M001 CodeGG decision contract + compatibility fixtures
      |
      +---------------------------+
      |                           |
      v                           v
M002 external repository      M004 System One adapter
extraction/runtime contract       |
      |                           |
      v                           |
M003 local runtime adoption <-----+
      |
      v
M005 learned tool-advisor migration
      |
      v
M006 legacy model/training retirement + closure
```

Dependencies:

- M001: ready now.
- M002: hard dependency on M001; operational dependency on creation/access to the external repository. It is planned here because CodeGG is the source repository and owns the extraction/compatibility requirements.
- M003: hard dependency on positive M002 runtime/artifact compatibility evidence.
- M004: hard dependency on M001 only; may proceed in parallel with M002/M003 because it is a separate backend.
- M005: hard dependencies on M003 and M004 contract stability; M004 need not be the selected backend, but the backend-neutral interface must have at least two independently implemented paths before migration is considered proven.
- M006: hard dependency on M005 closure and explicit inventory showing no remaining production caller of generic training/model-specific code.

## 8. Milestones

### M001 — Backend-neutral decision contract and CodeGG adapter seam

Plan:

- `plans/implementation/decision-model-extraction-runtime/001-decision-contract-and-codegg-adapter.md`

Status: active.

Objective: introduce the application-level decision semantics, backend trait/state, capability negotiation, bounded request/response types, and frozen compatibility fixtures without changing live tool disclosure.

Exit condition: the current deterministic path and existing advisor outputs can be represented in fixtures through the new contract; no backend is yet authoritative.

### M002 — External training/runtime repository extraction contract

Plan:

- `plans/implementation/decision-model-extraction-runtime/002-external-training-runtime-extraction.md`

Status: blocked on M001 plus external repository creation/access.

Objective: move generic training/evaluation/model-runtime ownership out of CodeGG and prove a versioned local-runtime artifact against CodeGG's frozen compatibility fixtures.

Exit condition: the external repository can train or load at least one compatibility artifact and its released/runtime interface passes CodeGG's fixture suite without CodeGG importing training code.

### M003 — Local artifact backend adoption

Plan:

- `plans/implementation/decision-model-extraction-runtime/003-local-artifact-backend-adoption.md`

Status: blocked on positive M002.

Objective: replace CodeGG's architecture-specific learned inference owner with the reusable local runtime backend while preserving `off`, observe-only parity, fallback, artifact validation, and resource bounds.

Exit condition: shadow/parity evidence passes and CodeGG no longer needs to know encoder/head implementation details for runtime inference.

### M004 — System One-compatible backend

Plan:

- `plans/implementation/decision-model-extraction-runtime/004-system-one-backend.md`

Status: blocked on M001.

Objective: add an explicit optional backend for compatible `/v1/systemone` services with exact semantic mapping, capability/profile validation, bounded deadlines, privacy controls, and deterministic fallback.

Exit condition: local fake-server plus at least one explicit implementation profile demonstrates Binary/Choice/Score mappings and failure behavior; unsupported Rank requests fail/fallback rather than being silently coerced.

### M005 — Tool-advisor migration to DecisionEngine

Plan:

- `plans/implementation/decision-model-extraction-runtime/005-tool-advisor-decision-engine-migration.md`

Status: blocked on M003 and M004 contract stability.

Objective: make learned tool advice consume `DecisionEngine` instead of model-specific CodeGG runtime types, while retaining CodeGG-owned candidate filtering, BM25 fallback, disclosure budgets, telemetry/capture policy, and causal-frontier behavior.

Exit condition: one actuation owner, disabled equivalence, authority-negative tests, and backend swap tests are green. Existing negative learned models remain negative historical evidence; no model is promoted by this migration.

### M006 — Legacy training/model code retirement and qualification

Plan:

- `plans/implementation/decision-model-extraction-runtime/006-legacy-training-runtime-retirement.md`

Status: blocked on M005.

Objective: remove generic training/framework/runtime implementation from CodeGG, retire obsolete feature flags and optional dependencies, reconcile documentation/planning, and run end-to-end qualification of no-backend/local/System-One failure paths.

Exit condition: default and all supported feature builds no longer require CodeGG-owned generic training code; Candle/model-specific dependencies remain only if another independently justified subsystem uses them; historical assets/evidence remain traceable.

## 9. Cross-cutting requirements

### Protocol and compatibility

The initial decision contract is internal Rust API unless a concrete cross-process consumer requires serialization. System One serialization belongs to its adapter. The local runtime's artifact contract is versioned independently from CodeGG configuration.

### Storage and migration

No database migration is expected. Model artifact paths/config may require a bounded configuration migration in M003/M005. Historical training data and experiment assets are not silently relocated.

### Security and privacy

Decision input comes from a minimal bounded projection. Remote backends receive no secret-bearing fields by default. Endpoint configuration, credentials, and transport policy are explicit. The model/runtime never gets authorization capability merely because it returns a high score.

### Failure and cancellation

Every decision call is cancellable/bounded by the current turn or request preparation deadline. Local load failure and network timeout fall back immediately. Repeated failures may circuit-break a backend for the process/session without failing the agent loop.

### Observability

Diagnostics identify:

- backend class and implementation profile;
- model/artifact id and version;
- semantic request type;
- candidate/option counts;
- timeout/fallback/unsupported reason;
- latency;
- resulting CodeGG action.

Raw sensitive state is not logged by default.

### Performance

M001 establishes no new model budget. M003 must compare the external local runtime against the historical in-repo runtime on artifact size, cold load, steady-state RSS, and p50/p95 scoring latency. M004 measures network/IPC overhead separately from service/model latency.

## 10. Verification strategy

The workstream requires:

- unit/property tests for semantic validation and bounds;
- frozen request/response compatibility fixtures;
- disabled/no-backend equivalence tests;
- authority-negative tests using the real resolved tool surface;
- artifact version/hash mismatch fallback;
- fake-server System One timeout/error/schema-drift tests;
- backend swap/shadow tests proving one actuation owner;
- default-build dependency checks;
- final workspace verification and relevant hosted CI;
- an explicit source/dependency inventory before any legacy deletion.

## 11. Risks and decision points

- The generic decision contract could become an over-general framework. Keep it limited to semantics actually needed by bounded decisions.
- System One implementations may differ in accepted criteria/schema details. Use explicit profiles/capability validation rather than vendor-name assumptions.
- A new local runtime may regress resource use even if model quality improves. Runtime adoption requires resource evidence, not just score parity.
- Moving too much from `src/tool_advisor` would accidentally export CodeGG domain policy. Causal/frontier and authority/promotion code remain local.
- Moving too little would leave the same architecture coupling under a new crate name. M006 requires dependency/source proof.
- Cross-repository development can create version skew. Compatibility fixtures and versioned manifests are the release gate.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | active | `plans/implementation/decision-model-extraction-runtime/001-decision-contract-and-codegg-adapter.md` | — | — |
| M002 | blocked | `plans/implementation/decision-model-extraction-runtime/002-external-training-runtime-extraction.md` | — | M001 closure and maintainer-approved external repository access |
| M003 | blocked | `plans/implementation/decision-model-extraction-runtime/003-local-artifact-backend-adoption.md` | — | Positive M002 closure |
| M004 | blocked | `plans/implementation/decision-model-extraction-runtime/004-system-one-backend.md` | — | M001 closure |
| M005 | blocked | `plans/implementation/decision-model-extraction-runtime/005-tool-advisor-decision-engine-migration.md` | — | M003 and stable M004 semantics |
| M006 | blocked | `plans/implementation/decision-model-extraction-runtime/006-legacy-training-runtime-retirement.md` | — | M005 closure and source/dependency inventory |

## 13. Completion definition

This roadmap closes only when:

1. CodeGG's agent loop can consume bounded decisions without depending on a model architecture;
2. generic training/evaluation/model-runtime implementation is independently owned outside CodeGG;
3. a local runtime backend passes the frozen compatibility contract;
4. a second backend implementation proves the interface is not artifact-specific;
5. learned tool-advisor integration uses the generic engine while CodeGG retains all authority and policy;
6. obsolete in-repo training/runtime implementation and dependencies are retired;
7. no-backend operation remains fully supported;
8. historical experiment evidence remains intact and attributable.
