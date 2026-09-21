# Tool-Selection Advisor Milestone 002 — Optional Pure-Rust Runtime

Status: ready for handoff

Repository baseline: `ed960f2ae7043acd816970f7d06e74dc69b09ae8`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-roadmap.md#m002--optional-pure-rust-advisor-runtime-and-artifact-contract`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

Hard dependency: M001 accepted closure.

## 1. Objective

Introduce an optional, in-process, pure-Rust tool-advisor runtime capable of loading a small versioned model artifact and scoring policy-allowed textual tool candidates in **observe-only** mode, with a mandatory `Noop` fallback.

M002 must not alter `tool_search` ordering or proactively advertise tools yet.

## 2. Why this milestone is blocked

M001 must first stabilize the candidate/context/prediction schema and baseline harness. M002 must consume that contract rather than invent a runtime-only representation.

## 3. Current implementation evidence

The baseline has no ML runtime dependency. `ResolvedToolSurface` already supplies the policy-filtered candidate universe and fingerprint needed for safe advisor input. Request preparation and disclosure already have defined owners; M002 should attach an observational scorer without becoming another resolver.

Current framework research makes Candle and Burn plausible Rust training/inference candidates. Candle already exposes BERT/safetensors and training; Burn unifies training/inference but requires a concrete pretrained-weight/footprint proof. tract is a viable pure-Rust inference engine but would create a separate training/export path and should only win if measured benefits justify that split.

## 4. Invariants that must not regress

- `tool_advisor.enabled=false` is the default.
- No model asset is required to start or use CodeGG.
- Missing/corrupt/incompatible model -> diagnostic + `Noop`, never failed turn.
- Advisor candidates are a subset of policy-allowed/discoverable tools.
- Advisor cannot execute, grant, register, or synthesize tool authority.
- Runtime/tokenizer path is Rust; no Python or required native inference service.
- Training-only dependencies do not enter this milestone.

## 5. Scope

### In scope

- `ToolAdvisor` interface and `NoopAdvisor`.
- Compact bounded context builder from current task/session artifacts.
- Policy-safe candidate projection from the resolved discovery universe.
- Optional learned advisor implementation behind feature/config gating.
- Model artifact manifest, hash/version validation, tokenizer loading, calibration data.
- Observe-only prediction diagnostics.
- CPU baseline and supported-target footprint/latency report.
- Circuit-break/fallback after repeated local inference failures.

### Explicitly out of scope

- Training.
- Telemetry upload.
- Reranking `tool_search`.
- Tool promotion.
- Bundling model weights into default release archives.
- GPU requirement.
- Tool-argument generation.

## 6. Required production changes

### Core/domain

Define a narrow interface similar to:

```rust
trait ToolAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction, ToolAdvisorError>;
}
```

The trait must not expose broker/permission execution handles.

Provide `NoopAdvisor` and a runtime state that can report disabled/not-installed/incompatible/ready/degraded.

### Artifact contract

A model directory/bundle must include a manifest with:

- artifact/model semantic version;
- architecture identifier;
- parameter/precision metadata;
- tokenizer hash/version;
- candidate descriptor schema version;
- context schema version;
- calibration version/data;
- maximum context/candidate limits;
- weights hash;
- provenance/training fingerprint;
- license/notice metadata required for redistribution.

Reject unknown major versions and hash mismatches before inference.

### Framework qualification

Before committing a durable dependency, measure candidate engines against:

- supported CodeGG OS/architectures;
- required native/non-Rust dependencies;
- release binary delta;
- model load time/RSS;
- CPU inference latency for realistic candidate counts;
- safetensors/tokenizer interoperability;
- ability to share exact architecture with M003 training.

Prefer one train+infer implementation unless a split clearly wins and is separately justified.

### Runtime configuration

Introduce a bounded configuration such as:

```toml
[tool_advisor]
enabled = false
mode = "observe"
model_path = "..."
max_candidates = 16
timeout_ms = ...
```

M002 accepts only `off/observe`; reject or ignore future active modes until M005.

## 7. Ordered work packages

A. Add interface/Noop/config and prove disabled equivalence.
B. Add context/candidate projection with policy-negative tests.
C. Spike and benchmark Rust runtime candidates; record selection rationale.
D. Implement artifact manifest/hash/tokenizer/calibration loading.
E. Add learned scoring in observe-only mode and bounded timeout/fallback.
F. Add diagnostics/docs and resource report.

## 8. Failure, cancellation, restart, and contention semantics

Inference is turn-bounded. Timeout/error/OOM-like allocation failure must skip advice and continue. Do not retry inference repeatedly inside one turn. A small consecutive-failure circuit breaker may disable advice for the session/process and expose a diagnostic.

Model loading should be lazy or bounded so disabled mode has no model I/O. Restart reconstructs state from config/model artifact; no durable runtime state is required.

## 9. Compatibility and migration

No database migration. Unknown advisor config must not break legacy configs. Model artifact incompatibility falls back rather than attempting unsafe conversion.

## 10. Required tests

- disabled/no-config/no-file equivalence;
- corrupt weights/manifest/hash/version fallback;
- denied/hidden/plan/parent-ceiling candidates absent;
- unknown textual tool descriptor is accepted;
- candidate/context size limits;
- deterministic scoring within backend tolerances;
- timeout/failure circuit breaker;
- supported-target compilation with advisor feature on and off.

## 11. Required verification commands

Expected minimum after implementation:

```bash
cargo test -p codegg --lib tool_advisor
cargo test -p codegg --lib agent::tool_surface
cargo test --test tool_surface_minimization
cargo test --no-default-features
cargo test --features tool-advisor
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Also record release binary/RSS/load/latency measurements for the selected runtime.

## 12. Documentation updates

- Advisor architecture/runtime lifecycle.
- Model artifact manifest and compatibility.
- Configuration and fallback semantics.
- Dependency selection rationale.

## 13. Acceptance criteria

M002 closes only when pure-Rust observe-only inference is optional, policy-safe, versioned, measurable, and failure-equivalent to no advisor. Default CodeGG remains model-free and behaviorally unchanged.

## 14. Stop conditions

Stop if the candidate runtime requires Python, a mandatory C/C++ inference runtime/sidecar, changes execution authority, or creates unacceptable unsupported-target/binary constraints without an explicitly approved architecture decision.

## 15. Closure evidence required

- framework comparison and selected rationale;
- dependency/binary-size diff;
- target compilation matrix;
- artifact validation/fallback evidence;
- candidate authority-negative tests;
- CPU latency/RSS/model-size data;
- disabled equivalence evidence.

## 16. Handoff notes

Do not add reranking because the scorer exists. Observe-only is intentional; active influence belongs to M005 after training and calibration exist.
