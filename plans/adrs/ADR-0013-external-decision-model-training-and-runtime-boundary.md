# ADR-0013: External Decision-Model Training and Backend-Neutral Runtime Boundary

Status: accepted

Date: 2026-10-05

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/002-long-term-roadmap.md`

Affected subsystem roadmaps:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md`
- `plans/subsystems/tool-selection-advisor-roadmap.md` (closed predecessor evidence; not reopened)
- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md` (closed predecessor evidence; not reopened)
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md` (historical negative/blocked experiment; future learned-model work is superseded by this decision)
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md` (closed predecessor evidence; not reopened)
- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md` (closed predecessor evidence; not reopened)
- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md` (closed CodeGG-specific deterministic successor; retained in CodeGG)

Supersedes in part:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

ADR-0009 remains authoritative for advisor authority monotonicity, optionality, fallback, consent, privacy, candidate filtering, observability, and progressive rollout. This ADR supersedes only the assumption that CodeGG owns the generic model-training pipeline and that all future decision-model experimentation must be implemented inside the CodeGG repository.

## Context

The local tool-advisor program has produced substantially more research infrastructure than the original M001-M005 roadmap anticipated. CodeGG now contains multiple learned-ranking and retrieval architectures, local training and calibration machinery, experiment-only encoder/runtime features, qualification harnesses, model assets/provenance, and a large body of frozen experiment evidence.

That work answered several useful questions:

- the agent/tool authority boundary is sound and can remain independent from learned advice;
- a model can consume textual/dynamic candidates without becoming a fixed classifier over built-in tools;
- calibration, abstention, leakage controls, unknown-tool evaluation, and failure fallback are necessary first-class contracts;
- several learned architectures have not justified live integration under the frozen gates;
- deterministic host-owned causal state is useful, but it is CodeGG domain logic rather than generic model infrastructure.

The repository now pays a maintenance and conceptual cost for continuing to be both the coding-agent application and the decision-model research environment. The generic research surface includes sequence encoders, ranking heads, retrieval projections, late interaction, training loops, framework qualification, artifact production, and model-specific benchmark machinery. Those concerns are reusable beyond tool selection for bounded decisions such as command allow/deny, model routing, escalation, retry policy, or context-retention choices.

At the same time, the ecosystem now has external typed-decision interfaces that make a backend-neutral boundary practical. TypeSafe's System One API exposes typed probabilistic questions through `POST /v1/systemone`; Ollama 0.35 exposes a compatible local endpoint for decision models. Compatibility is not assumed to be exact across implementations, so any adapter must negotiate or validate the subset it uses rather than treating "System One compatible" as an unversioned guarantee.

The project therefore needs two separations:

1. model training/evaluation/research must be able to evolve without CodeGG;
2. CodeGG's agent loop must consume decisions through an application-owned contract that is not tied to one model architecture, one artifact format, or one provider.

## Decision drivers

- Keep CodeGG focused on agent/runtime semantics rather than model-research implementation.
- Preserve every ADR-0009 authority and fallback invariant.
- Permit decision-model experiments to use the training stack best suited to research, including Python, MLX, PyTorch, Candle, Burn, or other tooling, without changing CodeGG's shipping dependency graph.
- Keep a lightweight local Rust inference path available for qualified artifacts.
- Allow models and artifacts to be replaced independently of CodeGG releases where compatibility policy permits.
- Allow external typed-decision services such as Jev/System One-compatible endpoints to participate through an explicit adapter.
- Preserve CodeGG-specific typed state, tool policy, causal contracts, disclosure budgets, and actuation inside CodeGG.
- Avoid deleting historical experiment evidence merely because implementation ownership moves.
- Make networked decision backends explicit and opt-in; CodeGG must still work with no decision model or service.

## Considered options

### Option A — Continue model research entirely inside CodeGG

This preserves one repository and existing code paths, but it keeps training frameworks, experimental architectures, model assets, and qualification machinery coupled to the coding harness. It makes every new small-decision use case a CodeGG subsystem concern and makes model iteration compete with agent/runtime maintenance.

Rejected for future learned-model work.

### Option B — Extract only training scripts but keep model-specific inference in CodeGG

This reduces some dependencies but leaves CodeGG coupled to architecture-specific encoders, tokenizers, artifact manifests, and scoring heads. Every model architecture change can still require CodeGG production edits.

Rejected as an incomplete boundary.

### Option C — External decision-model repository plus backend-neutral CodeGG decision contract

A separate repository owns generic training/evaluation/model-runtime concerns. CodeGG owns bounded application state and maps it into a small decision request/response contract. A reusable runtime/backend layer may load local artifacts or call an external typed-decision endpoint. Tool selection becomes one downstream application of the decision machinery.

Selected.

### Option D — Standardize exclusively on Jev/System One

This provides a useful external protocol but would make CodeGG dependent on a young external interface and would not serve custom tiny local artifacts that do not map exactly to current question types.

Rejected as the sole abstraction. System One is an adapter target, not the CodeGG domain contract.

## Decision

### 1. Generic model training leaves CodeGG

Future generic learned-model architecture work, training loops, fine-tuning, calibration research, generic decision benchmarks, model conversion/quantization, artifact production, and framework-specific experimentation belong in a separate decision-model repository.

The external repository is intentionally not named by this ADR. Repository creation, naming, publication, and release policy are external operational decisions. CodeGG plans may define the required interface contract before that repository exists, but must not invent a permanent package name merely to unblock planning.

CodeGG may retain application-specific fixture exporters and compatibility fixtures, but it must not remain the primary owner of generic training frameworks once extraction completes.

### 2. Training implementation language is not a CodeGG constraint

ADR-0009's pure-Rust requirement continues to apply to CodeGG's ordinary local production inference path when CodeGG embeds that path.

It no longer applies to the external training/research repository. Training may use Rust, Python, MLX, PyTorch, Candle, Burn, or another framework when that improves iteration or hardware support. Reproducibility, provenance, deterministic splits, leakage controls, and artifact compatibility remain required regardless of framework.

CodeGG default builds must not gain Python, training frameworks, model weights, or training-only native dependencies.

### 3. CodeGG owns an application-level decision contract

CodeGG will consume optional bounded decision engines through a backend-neutral contract. The contract represents the decision semantics CodeGG needs, not a particular model API.

The initial semantic families are:

- binary / yes-no probability;
- exclusive choice over bounded named options;
- ordinal or rubric score;
- candidate relevance/ranking with zero, one, or multiple relevant candidates;
- explicit abstention/unavailable/degraded outcomes.

A backend may advertise only a subset. CodeGG must reject an unsupported semantic request or fall back; it must not silently reinterpret a multi-relevance ranking problem as an exclusive choice.

Requests carry bounded structured state, a decision specification, candidate/options where applicable, schema/version information, and an application request id. Responses carry backend/model identity, scores/probabilities, abstention or unsupported status, latency, and provenance sufficient for diagnostics.

The decision engine never receives or creates execution authority.

### 4. Runtime backends are replaceable

The CodeGG integration must permit at least these backend classes:

- `off/noop`;
- a local artifact/runtime backend supplied by the external decision-model project once qualified;
- an optional System One-compatible HTTP backend;
- future adapters that satisfy the same application contract.

Backend selection is configuration, not compile-time agent-loop architecture.

No backend is required for CodeGG startup or normal agent execution.

### 5. System One is an adapter, not the internal wire contract

A System One adapter may map compatible CodeGG decision requests to the external `state + named questions` protocol.

The adapter must validate the exact question/criteria subset it uses and must not assume every implementation has identical schema behavior. Capability probing, model discovery, protocol-version pinning, or explicit implementation profiles may be used where needed.

Remote TypeSafe/Jev use requires explicit endpoint/credential configuration. Local Ollama or another local service is still a separate process/network dependency and remains optional.

A networked decision failure, timeout, schema mismatch, or unavailable model falls back according to CodeGG policy and may never stall or fail the agent loop beyond its bounded decision deadline.

### 6. CodeGG-specific policy and state stay in CodeGG

The following remain CodeGG-owned:

- `ResolvedToolSurface` and all permission/parent-ceiling filtering;
- tool descriptors derived from the live catalog;
- bounded projection of task/session/goal/test/artifact state;
- deterministic BM25/tool-search fallback;
- disclosure and promotion budgets;
- decision actuation inside request preparation;
- downstream trajectory qualification;
- training/capture consent at the point CodeGG produces application data;
- causal-frontier facts, causal contracts, admissibility logic, and active disclosure.

The causal-frontier line is not moved merely because it lives under `src/tool_advisor`; it is domain policy over host-owned state, not a generic learned-model runtime.

### 7. Historical evidence remains immutable

Existing closure records, frozen qualification assets, negative experimental results, and preregistrations remain in CodeGG for traceability.

Extraction may move or copy reusable implementation and test fixtures, but it must not rewrite historical closure evidence to imply a model succeeded or that an old run occurred in the new repository.

Historical experiment-only source may be retired only after equivalent evidence remains inspectable and the new runtime boundary has compatibility coverage.

### 8. Artifact compatibility is explicit

The external local runtime must expose a versioned manifest/loader contract. CodeGG must not directly deserialize architecture-specific weight files once migration completes.

At minimum, compatibility identifies:

- runtime/artifact schema version;
- decision semantic capabilities;
- architecture/runtime implementation id;
- tokenizer/input schema versions where applicable;
- model/weight hash;
- calibration version;
- maximum input/candidate bounds;
- provenance/training fingerprint;
- license/provenance metadata required for redistribution.

Unsupported artifacts fail closed to another backend or the deterministic path.

### 9. Data export is separate from training consent

CodeGG may retain explicit local capture/export of decision examples. Export does not imply training, upload, or telemetry.

Any external repository import command consumes deliberate user-provided datasets/artifacts; it does not reach into CodeGG state directories implicitly.

Remote contribution remains separately consented under ADR-0009.

## Consequences

### Positive

- Model experimentation can evolve independently from CodeGG release cadence and dependency constraints.
- The same training pipeline can support tool selection and unrelated bounded decision tasks.
- CodeGG can swap local artifacts without rewriting the agent loop.
- Jev/System One-compatible services can be evaluated without becoming the permanent internal abstraction.
- CodeGG's default dependency graph can eventually shed Candle and experiment-only training features.
- Historical negative experiments remain useful without forcing their implementation to remain production-adjacent indefinitely.

### Negative

- A new repository and compatibility boundary must be maintained.
- Cross-repository releases and artifact/runtime versioning become explicit operational concerns.
- Some existing tool-advisor types must be split into generic and CodeGG-specific representations.
- External service adapters add timeout, protocol-drift, credential, and privacy considerations.

### Neutral or deferred

- The new repository name, organization, initial release number, and package publishing policy are deferred.
- Bundling a default local model is not approved by this ADR.
- Remote decision services are not default-on.
- The best model architecture remains empirical.
- This ADR does not qualify any previously negative learned architecture.

## Compatibility and migration

Migration is staged. CodeGG first introduces the backend-neutral contract and compatibility fixtures while the current implementation remains authoritative. The external repository then proves it can reproduce a selected existing artifact/scoring fixture or an explicitly versioned successor. Only after that proof may CodeGG switch the learned runtime owner and remove training/model-specific implementation.

During migration, duplicate execution of a decision may be allowed only in observe/shadow mode for parity evidence. There must be one actuation owner.

Configuration changes must preserve `off` as a valid mode. Legacy tool-advisor configuration may be read and translated for a bounded compatibility period; unknown or removed model-specific fields must never make CodeGG unusable.

## Security and reliability implications

- CodeGG constructs decision inputs only after application policy has bounded the candidate/state universe.
- External/networked backends receive the minimum projected state required for the requested decision.
- Secrets, raw tool arguments/results, credentials, and unrestricted repository content are excluded unless a future explicit contract permits a field.
- Remote decision backends require explicit configuration and obey bounded deadlines.
- Backend failure never widens tool authority or grants permissions.
- Local artifact validation is fail-closed.
- Training/export consent remains distinct from runtime backend selection.
- Backend diagnostics must avoid logging raw sensitive state by default.

## Verification

Conforming implementation must prove:

- `off`/missing-backend behavior is equivalent to the current deterministic path;
- decision backends cannot add candidates or authority;
- semantic capability mismatch fails explicitly;
- local artifact incompatibility falls back safely;
- System One timeout/schema/model failures fall back safely;
- network activity is absent unless an external backend is explicitly configured;
- CodeGG-specific causal-frontier behavior remains unchanged by learned-runtime extraction;
- an external runtime can pass a frozen compatibility fixture before CodeGG retires its legacy implementation;
- default CodeGG builds no longer contain training-only dependencies after the final migration milestone;
- historical qualification assets/closure records remain inspectable.
