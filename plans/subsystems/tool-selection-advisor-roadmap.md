# Tool-Selection Advisor Roadmap

Status: active

Repository audit baseline: `ed960f2ae7043acd816970f7d06e74dc69b09ae8`

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — accepted; controls advisor authority, optionality, pure-Rust runtime, local-first training, and telemetry consent.

Related closed predecessor work:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md`
- `plans/closure/coding-agent-tool-surface-corrective/001-status.md` through `007-status.md`
- `plans/closure/coding-agent-tool-surface-post-closure-evidence-polish/001-status.md`

## 1. Purpose and ownership boundary

This subsystem owns experimental machine-assisted **tool selection and discovery advice**: evaluation data, candidate/context schemas, local ranking inference, model artifact compatibility, local model training, training-data capture, and explicit opt-in training telemetry.

It consumes the already-resolved policy-allowed tool universe from CodeGG's tool-surface machinery. It does not own registration, permission, parent ceilings, plan-mode restrictions, broker invocation, tool arguments, scheduler admission, sandboxing, or provider execution.

The intended end state is a very small local model that can help a primary agent discover the right existing capability, especially when the primary model is tool-fragile or small. The project must remain fully functional without that model.

## 2. Work classification

### Invariants

- Advisor-disabled behavior is equivalent to the established non-advisor path.
- Advisor failure degrades to ordinary CodeGG discovery rather than failing an agent turn.
- The advisor sees only policy-allowed/discoverable candidates.
- Advisor output cannot widen tool authority or directly execute a tool.
- Production inference/tokenization is pure Rust and in-process.
- Training is opt-in and local-first.
- Remote training telemetry is separately opt-in, network-silent by default, and never implied by advisor use.
- Candidate semantics are textual so unknown MCP/plugin tools can be ranked without retraining a fixed class head.
- Security/audit logs remain semantically separate from training datasets.

### Capabilities

- Reproducible tool-selection benchmark and baseline metrics for keyword/BM25.
- Optional local learned ranking in observe-only mode.
- Opt-in semantic reranking of `tool_search`.
- Opt-in proactive promotion of strongly relevant deferred tools after qualification.
- Local training/evaluation/calibration commands producing versioned model artifacts.
- User-inspectable local training capture with export and purge.
- Explicit remote-capable telemetry sink contract with no default destination.

### Infrastructure

- Versioned `ToolAdvisorCase`, candidate descriptor, context projection, prediction, and outcome schemas.
- Dataset split/provenance utilities and unknown-tool holdouts.
- `ToolAdvisor` runtime interface with `Noop` fallback.
- Pure-Rust model/tokenizer/artifact loader.
- Training-only crate/feature or binary isolated from default production dependency graph.
- Versioned local event spool and sink abstraction.

### Polish

- Diagnostics explaining model availability, selected mode, latency, artifact version, and fallback.
- Benchmark reports with model size/RSS/load/p50/p95 latency and accuracy/calibration.
- Operator commands to inspect model/data status.

## 3. Non-goals

- Replacing `tool_search`, `ToolCatalog`, `ResolvedToolSurface`, `ToolBroker`, or permission policy.
- Having the advisor generate arbitrary prose or tool arguments.
- Training a general coding model.
- Making learned disclosure mandatory or default in this workstream.
- Uploading repository content automatically.
- Creating a project-hosted remote training service in the first round.
- Reusing live execution/security audit records as training payloads.
- Requiring Python for training, evaluation, conversion, or runtime.
- Selecting a large generative model merely because it has existing function-calling support.

## 4. Current state

At `ed960f2ae7043acd816970f7d06e74dc69b09ae8`:

- `ToolCatalog` supports keyword and BM25 search over canonical tool names/descriptions.
- `tool_search` is a policy-gated two-stage discovery contract and caps broad results at ten.
- `ResolvedToolSurface` already distinguishes the complete allowed surface from initial advertisement and retains surface fingerprints/aliases/capabilities.
- `read` and `tool_search` are protected from surface reduction.
- MCP names are namespaced and can enter the discovery universe without shadowing native tools.
- No learned tool-ranking runtime, benchmark corpus, model artifact format, or training pipeline exists.
- No general consent/analytics subsystem exists. Existing audit/telemetry-like code serves operational/security semantics and must not silently become a training-data pipeline.
- Existing shell redaction machinery demonstrates prior secret-scrubbing concerns but is shell-specific; it may be generalized only if its contract is appropriate.
- The root package is pure Rust and currently has no ML runtime dependency.

External implementation research at planning time shows:
- Hugging Face Candle is Rust, supports BERT, safetensors and model training, making it a plausible first spike for a compact encoder.
- Burn is Rust and explicitly supports both training and inference, but CodeGG must prove pretrained encoder/weight compatibility and footprint before selection.
- tract provides a pure-Rust inference path for ONNX but does not solve local training; using it as primary runtime would create a separate export/training boundary that must justify itself with footprint/performance evidence.

No framework is selected by the roadmap.

## 5. Target architecture

```text
ToolRegistry / MCP tools
        |
        v
existing policy + availability filtering
        |
        v
ResolvedToolSurface / discoverable universe
        |
        +-------------------------------+
        |                               |
        v                               v
 current keyword/BM25            ToolAdvisor (optional)
        |                         - local Rust inference
        |                         - textual candidates
        |                         - calibrated abstention
        |                               |
        +---------------+---------------+
                        |
                 advisory scores only
                        |
            +-----------+-----------+
            |                       |
     tool_search rerank       optional promotion
            |                       |
            +-----------+-----------+
                        |
                  primary LLM
                        |
                  ToolBroker /
              permission / sandbox
```

The advisor runtime must have a hard `Noop` path. Runtime configuration chooses `off`, `observe`, `rerank`, or `promote`; only `off` is the initial default. M002 introduces `off/observe`; M005 qualifies `rerank/promote`.

Training is a separate local pipeline:

```text
curated fixtures + local captured events + optional imported public data
        |
        v
dataset builder / split / unknown-tool holdouts
        |
        v
opt-in Rust trainer
        |
        v
calibration + evaluation
        |
        v
versioned model artifact
        |
        v
optional local runtime
```

Training-data transport is another separate path:

```text
training event
   |
redaction/content policy
   |
NoopSink (default)
   +--> LocalSpoolSink (explicit local capture)
   +--> Export bundle
   `--> RemoteSink contract (explicit endpoint + consent only)
```

## 6. Dependency graph

```text
M001 Evaluation/data contract and baseline harness
   | \
   |  `----------------------+
   v                         v
M002 Optional Rust runtime   M004 Consent-gated capture/telemetry
   |
   v
M003 Local Rust trainer/model lifecycle
   \                         /
    +-----------+-----------+
                v
M005 Observe/rerank/promotion qualification
```

- M001 -> M002: hard.
- M001 -> M004: hard (event/case schema must stabilize first).
- M002 -> M003: hard (trainer must emit the runtime artifact contract).
- M002/M003/M004 -> M005: hard for end-to-end qualification.
- External public datasets/models are operational inputs, not hard architecture dependencies.

## 7. Milestones

### M001 — Evaluation corpus, schemas, and deterministic baselines

Class: infrastructure

Objective: Establish a pure-Rust, no-ML-dependency benchmark/data contract for tool-selection quality, including keyword/BM25 baselines, hard negatives, no-tool cases, multi-tool labels, and unknown-tool holdouts.

Dependencies: closed coding-agent tool-surface corrective work.

Deliverable boundary: dataset/evaluation machinery only; no learned runtime and no agent behavior change.

User or operator value: CodeGG can measure whether any future learned advisor is actually better than current discovery.

Exit conditions: `ToolAdvisorCase` schema/versioning exists; curated fixture corpus exists; deterministic train/dev/test and leave-one-tool-out splits exist; baseline report is reproducible; default CodeGG behavior/dependency graph is unchanged.

### M002 — Optional pure-Rust advisor runtime and artifact contract

Class: infrastructure

Objective: Add an optional in-process Rust inference boundary with `Noop` fallback, artifact validation, observe-only scoring, and strict authority monotonicity.

Dependencies: M001 closed.

Deliverable boundary: model loading/scoring and diagnostics only; no automatic reranking/promotion.

User or operator value: experimental local models can be exercised against real sessions without risking execution semantics.

Exit conditions: feature/config default off; no-model and corrupt-model fallback proven; pure-Rust candidate runtime qualified on supported targets; model artifact manifest/versioning established; observe-only predictions recorded locally only when explicitly requested.

### M003 — Opt-in local Rust training, calibration, and model lifecycle

Class: capability

Objective: Train/fine-tune the compact tool-selection model locally using Rust-only tooling and emit M002-compatible artifacts.

Dependencies: M001 and M002 closed.

Deliverable boundary: local trainer/evaluator/calibrator and model management; no remote trainer and no default bundled model.

User or operator value: maintainers/developers can iterate on ToolMind-like models without Python and without sending training data off machine.

Exit conditions: one reproducible small encoder training recipe works locally; held-out calibration/evaluation is automated; artifacts are deterministic/versioned; training dependencies are absent from ordinary default builds.

### M004 — Consent-gated local capture and remote-ready telemetry

Class: infrastructure

Objective: Add a separate training-data event pipeline with explicit local capture, inspect/export/purge, bounded retention, content policy, and a remote sink contract that is network-silent unless separately opted in.

Dependencies: M001 closed.

Deliverable boundary: data lifecycle/transport machinery only; no project-hosted collection service and no implicit upload.

User or operator value: useful local trajectories can become training material, and future voluntary contribution does not require redesigning privacy/consent semantics.

Exit conditions: default Noop sink; local spool opt-in; remote endpoint has no default; metadata-vs-content consent is explicit; redaction/negative tests exist; remote transport tests use a local fake server; disabling telemetry produces zero telemetry network attempts.

### M005 — Advisor integration and downstream qualification

Class: capability

Objective: Compare observe-only, BM25, learned reranking, and proactive promotion, especially with smaller primary models, and expose rerank/promote only as explicit experimental modes when qualification gates are satisfied.

Dependencies: M002, M003, and M004 closed.

Deliverable boundary: search/disclosure integration and evidence; no change to default `off` mode.

User or operator value: users can opt into a validated local advisor that improves specialized tool discovery without changing permission/execution authority.

Exit conditions: advisor beats or matches deterministic baselines on declared primary metrics, unknown-tool tests pass, calibration is documented, resource budgets are reported, downstream coding trajectories show benefit or the feature remains observe-only, and disabled equivalence remains proven.

## 8. Cross-cutting requirements

### Storage and migration

Benchmark fixtures are repository data, not user state. Local captured events and model artifacts require explicit schema versions and bounded storage locations under CodeGG-managed data paths. No database migration is required for M001.

### Protocol and compatibility

No provider wire protocol changes. Advisor schemas are internal until explicitly documented otherwise. Model artifact compatibility is fail-closed with fallback, never best-effort reinterpretation.

### Security and authorization

Advisor candidates are a projection of already-allowed tools. Remote telemetry requires separate user consent and destination configuration. Credential/tool-argument/result capture is excluded by default.

### Concurrency, cancellation, and recovery

Inference must be bounded and cancellable with the turn. Failure disables/falls back rather than stalling execution. Training runs are explicit local jobs and must checkpoint safely if interruption support is implemented.

### Observability and audit

Advisor predictions are diagnostic evidence, not security audit authority. Record scores/model version/surface fingerprint/mode without exposing chain-of-thought.

### Performance and resource use

Measure model artifact size, binary delta, cold load, RSS, p50/p95 CPU latency, and candidate-count scaling on representative x86_64, Apple Silicon, and ARM64/SBC hardware where available. No resource gate is invented before M001/M002 produce baselines.

### Documentation and operations

Document how to disable/remove a model, inspect its manifest, inspect/purge training data, and prove telemetry is off.

## 9. Verification strategy

Subsystem verification must include:

- deterministic baseline benchmark;
- unknown-tool and leave-one-tool-out tests;
- disabled/no-model equivalence;
- policy-negative tests for denied/hidden/parent-ceiling candidates;
- artifact corruption/version mismatch fallback;
- local-only training test without Python;
- telemetry no-network-by-default tests;
- local fake-server tests for explicit remote configuration;
- end-to-end small-primary-model A/B trajectories before promotion is considered useful.

## 10. Risks and decision points

- A 4-5M encoder may be too weak; M003 may need the 15-25M class without changing architecture.
- Pairwise cross-encoding may be too expensive for large MCP catalogs; shortlist + Laya-style multi-candidate scoring should be benchmarked.
- Training from noisy observed tool use can teach existing model mistakes; curated/hard-negative/teacher or human labels may be required.
- A framework may be pure Rust but still impose unacceptable binary size/target constraints. M002 must measure rather than assume.
- If a durable framework/model-format choice creates a public compatibility dependency beyond ADR-0009, add a follow-up ADR rather than burying the choice in code.

## 11. Completion definition

The roadmap closes only when CodeGG has a qualified optional local advisor path, a reproducible local Rust training path, explicit training-data lifecycle/telemetry consent, downstream evidence, and unchanged normal operation with every advisor/training/telemetry feature absent or disabled.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/tool-selection-advisor/001-evaluation-corpus-and-baselines.md` | `plans/closure/tool-selection-advisor/001-status.md` | none |
| M002 | ready | `plans/implementation/tool-selection-advisor/002-optional-pure-rust-runtime.md` | — | none |
| M003 | blocked | `plans/implementation/tool-selection-advisor/003-local-rust-training-and-model-lifecycle.md` | — | M001 + M002 closure |
| M004 | ready | `plans/implementation/tool-selection-advisor/004-consent-gated-training-telemetry.md` | — | none |
| M005 | blocked | `plans/implementation/tool-selection-advisor/005-advisor-integration-and-qualification.md` | — | M002 + M003 + M004 closure |
