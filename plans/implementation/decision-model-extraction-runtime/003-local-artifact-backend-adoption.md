# Decision-Model Extraction and Runtime Milestone 003 — Local Artifact Backend Adoption

Status: implemented

Repository baseline: `533be5941ac48334743de555cda96d8695cc6527`

Source roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m003--local-artifact-backend-adoption`

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

Hard dependencies:

- M002 positive closure is satisfied. SDM runtime/artifact revision `8139b064bdcf3212e8f6fd912e801a479b55751c` is pinned; the M001 fixture compatibility checker passes.

## 1. Objective

Adopt the external project's reusable local runtime as CodeGG's learned local-artifact backend and prove shadow/parity, failure fallback, resource behavior, and artifact compatibility without changing the CodeGG-owned tool authority/disclosure policy.

After this milestone, CodeGG learned inference must no longer require agent-loop code to know the encoder/head/tokenizer implementation.

## 2. Current implementation evidence

The current repository has several model-specific runtime paths under `src/tool_advisor/`, including legacy linear/contextual scoring and experiment-gated sequence-encoder/ranking code. Optional Candle dependencies are declared in the root Cargo manifest under advisor encoder features.

The established safety behavior is already correct:

- advisor default is off;
- model errors fall back;
- candidates are pre-filtered by policy;
- actuation remains in CodeGG.

M003 changes the inference owner, not those semantics.

## 3. Invariants that must not regress

- The local backend is optional and disabled/no-artifact remains valid.
- No implicit model download.
- Artifact load/hash/version failure falls back before scoring.
- Backend receives only caller-supplied bounded state/candidates.
- One actuation owner remains in CodeGG.
- Causal-frontier code/path remains unchanged.
- Historical model-specific CLIs/tests may coexist temporarily for parity but are not newly qualified.
- Default builds must not gain training dependencies.
- Resource regressions against historical runtime evidence are measured and classified, not hidden.

## 4. Scope

### In scope

- Add/pin the external local-runtime dependency behind an appropriate optional feature.
- Implement a CodeGG `DecisionEngine` adapter for it.
- Map CodeGG M001 request semantics/capabilities into the external runtime contract.
- Local artifact configuration, load/state diagnostics, validation, and failure circuit breaker.
- Shadow comparison against selected legacy deterministic/learned fixtures.
- Resource benchmarking.
- Bounded config migration/compatibility.
- Disable architecture-specific runtime use in normal learned-advisor code once parity closes.

### Out of scope

- Removing all legacy source/features (M006).
- New model training.
- System One backend (M004).
- Enabling rerank/promote by default.
- Claiming any historical model is now qualified.
- Changing causal-frontier active mode.

## 5. Required production changes

### Dependency boundary

Pin a release/tag/revision that passed M002. Runtime features must not enable destination training features.

Record `cargo tree` evidence showing no unexpected Python/native training dependency enters default or local-runtime builds.

### Backend adapter

Implement the external local runtime behind `DecisionEngine`.

Capabilities must be queried from the loaded artifact/runtime. An artifact that lacks requested Rank semantics returns Unsupported and triggers deterministic fallback.

### Artifact lifecycle

Configuration identifies the artifact path and optional expected identity/version constraints.

Load behavior:

- lazy or explicitly bounded;
- hash/manifest validation before ready state;
- no network;
- actionable diagnostics;
- repeated corrupt/load failures may circuit-break;
- replacement is atomic from CodeGG's perspective: an in-flight turn uses one immutable backend snapshot.

### Configuration migration

If existing `tool_advisor` model-path/config fields are retained, translate them explicitly to the new local backend during a bounded compatibility window.

Do not guess artifact format from filename.

### Shadow/parity

Before making the new local backend the only learned inference owner, support observe/shadow evaluation on M001 fixtures and selected historical cases.

For deterministic artifacts, score differences must satisfy a documented exact/tolerance rule. For a successor artifact with changed architecture, compare semantic ranking/status contracts rather than pretending bitwise equality.

### Performance

Measure:

- runtime dependency/binary delta;
- artifact bytes;
- cold load;
- warm RSS;
- p50/p95 request latency by candidate count;
- failure/fallback latency.

Compare against the relevant historical CodeGG runtime evidence and declare regressions.

## 6. Ordered work packages

### A — Dependency and feature isolation

Add the pinned runtime with a minimal feature set.

Acceptance: default CodeGG graph unchanged; local-runtime feature builds without external training stack.

### B — Local backend implementation

Implement load/state/capabilities/evaluate mapping.

Acceptance: all M001 semantic/status tests pass for supported semantics and unsupported requests are explicit.

### C — Artifact failure and concurrency

Add corrupt/version/hash/missing/replace/in-flight snapshot tests.

Acceptance: no partial artifact replacement and no failed turn caused solely by backend failure.

### D — Shadow parity

Run M001 compatibility fixtures and selected legacy tool-advisor fixtures through both paths.

Acceptance: declared parity/semantic criteria pass before legacy runtime routing is disabled.

### E — Resource qualification

Produce machine-readable or documented benchmark evidence.

Acceptance: no unclassified budget regression.

### F — Runtime ownership switch

Route learned local decision calls through the new backend only; keep legacy implementation compiled only where still required for historical tests until M006.

## 7. Failure/recovery semantics

A load or evaluate error is non-fatal to the agent turn and falls back immediately. Backend initialization must not hold global agent locks.

An artifact replacement cannot mutate an in-flight backend object. Use immutable snapshot/swap semantics consistent with CodeGG runtime-asset ownership patterns.

## 8. Security

- Artifact paths follow existing configured-local-file trust rules.
- Hash/manifest mismatch is rejection, never best-effort reinterpretation.
- Backend has no filesystem/tool permission beyond reading its configured artifact.
- No model request contains secret fields merely because they exist in session state.
- Runtime diagnostics avoid raw projected state.

## 9. Compatibility and migration

No database migration.

Legacy config may remain readable for one migration window. New configuration should identify backend type separately from artifact path.

Unknown backend/artifact versions degrade to deterministic operation rather than startup failure.

## 10. Required tests

- feature/dependency isolation;
- artifact validation/tamper;
- capability mismatch;
- missing/corrupt artifact fallback;
- backend snapshot replacement under concurrent turns;
- M001 fixture parity;
- tool authority-negative integration;
- default/no-model equivalence;
- resource benchmark harness.

## 11. Required verification commands

At minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --locked -p codegg --lib tool_advisor
cargo test --locked --workspace
cargo tree -e features
scripts/verify.sh quick
git diff --check
```

Record exact feature-specific runtime tests using the final feature name.

## 12. Documentation updates

- `architecture/tool-advisor.md` runtime owner and artifact lifecycle.
- config/reference docs for backend selection/artifact.
- `architecture/tool-advisor-framework-spike.md` historical implementation status.
- roadmap/registry.

## 13. Acceptance criteria

M003 closes only when:

1. CodeGG consumes the external local runtime through `DecisionEngine`;
2. agent-loop learned inference no longer depends on architecture-specific types;
3. M001 compatibility/shadow evidence passes;
4. artifact failure and concurrent replacement are safe;
5. no training dependencies enter normal builds;
6. resource evidence is recorded;
7. default/off behavior is unchanged.

## 14. Stop conditions

Stop if:

- adoption requires loosening authority/fallback;
- runtime pulls training/Python into production features;
- artifact compatibility is implicit/filename-based;
- in-flight turns can see mutable artifact replacement;
- parity requires modifying frozen historical expected outputs without a versioned reason.

## 15. Closure evidence required

- external runtime revision;
- Cargo feature/tree diff;
- compatibility/parity report;
- artifact failure/concurrency tests;
- resource report;
- authority-negative/default-equivalence evidence;
- verification results;
- unresolved findings;
- dependency audit for M005.
