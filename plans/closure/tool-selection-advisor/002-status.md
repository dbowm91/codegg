# Tool-Selection Advisor Milestone 002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-selection-advisor/002-optional-pure-rust-runtime.md`
Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-roadmap.md#m002--optional-pure-rust-advisor-runtime-and-artifact-contract`
Repository baseline reviewed: `67d2a3a`
Implementation commits: `bc66f35` — optional pure-Rust runtime; `fef2648` — artifact/scoring qualification tests

## 1. Executive finding

M002 is complete. CodeGG has an optional in-process Rust advisor boundary with
strict artifact validation, a deterministic lightweight scorer, an explicit
policy-filtered surface projection, and a `NoopAdvisor` fallback. The runtime
does not alter tool ordering, disclosure, promotion, or execution.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Optional interface and Noop fallback | `ToolAdvisor`, `NoopAdvisor`, `advisor_from_config`; default disabled status |
| Policy-safe candidates | `candidates_from_surface` consumes only `ResolvedToolSurface::tools` after policy/plan/ceiling filtering |
| Pure-Rust artifact/runtime | `hashed-linear-v1`, Unicode alphanumeric tokenizer, JSON manifest and `LinearAdvisor`; no ML framework/native sidecar |
| Manifest/version/hash validation | `ToolAdvisorArtifactManifest`, `validate_artifact`, SHA-256 weight digest tests |
| Failure fallback/circuit breaker | `advisor_from_config` returns `NoopAdvisor` for absent/incompatible/corrupt model; repeated scorer failures open a bounded breaker |
| Off/observe configuration | `ToolAdvisorConfig`; active `rerank/promote` modes are rejected until M005 |
| Atomic artifact lifecycle | `write_artifact_atomic` and load/install round-trip test |

## 3. Production implementation evidence

The artifact is intentionally a small custom linear scorer. Framework research
was revisited during implementation: Candle and Burn would add a substantial
training/runtime dependency before a model or measured need exists, while
tract would create a separate export/training boundary. M002 therefore selects
the smallest compatible Rust artifact contract and keeps the framework choice
reversible for M003 experiments.

The manifest carries model/architecture/precision, tokenizer hash/version,
case/context schema versions, calibration version, limits, weight hash,
provenance, and license notice. The scorer only receives a bounded context and
candidate projection and exposes no broker, permission, registry, or process
handle.

## 4. Verification executed

Local verification:

- `cargo test --lib tool_advisor -- --nocapture` — 9 passed, including invalid hash/version, atomic install/load, fallback state, and bounded CPU-path tests.
- `cargo test --lib agent::tool_surface` — 5 passed.
- `cargo test --no-default-features --lib tool_advisor` — 9 passed.
- `cargo test --features tool-advisor --lib tool_advisor` — 9 passed.
- `cargo check --features tool-advisor-training` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.

These are local results on the current developer machine; no hosted/CI result
or cross-architecture performance claim is made. The bounded CPU sample runs
1,000 single-candidate scores under one second locally; release binary/RSS
measurements remain a M003/M005 qualification concern.

## 5. Invariant review

`enabled=false`, no config, no model path, corrupt artifact, unsupported schema,
and unsupported active mode all resolve to a diagnostic plus `NoopAdvisor`.
Default CodeGG remains model-free. The runtime cannot widen authority because
its candidate builder accepts only an already resolved surface.

## 6. Failure and recovery review

Artifact reads are size-bounded and fail closed on malformed JSON, schema
mismatch, limit mismatch, invalid calibration, and weight hash mismatch.
Installation writes a sibling temporary file and renames it only after complete
serialization/validation. Scoring errors are bounded and do not retry inside a
turn; the circuit breaker disables repeated failures.

## 7. Migration and compatibility review

No database, session, provider, or wire protocol migration was introduced.
Unknown advisor configuration remains non-fatal through the optional config
field. Artifact compatibility is explicit and never best-effort converted.

## 8. Security review

The scorer receives only bounded textual descriptors and context. No tool
arguments, credentials, secrets, or execution authority cross the interface.
No network, Python, C/C++ inference runtime, or default model download was
added.

## 9. Documentation and operations

`architecture/tool-advisor.md` documents the artifact, fallback, and authority
boundary. Configuration fields are represented in the shared config schema;
M003 will add the explicit training/install lifecycle commands.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Low: cross-target binary/RSS/load measurements are not available on this
  developer machine; record them during M003/M005 qualification.
- Low: the artifact format currently uses JSON rather than a compact binary
  weights container; this is bounded and versioned, and can be superseded only
  with explicit compatibility evidence.

No critical, high, or medium findings remain.

## 11. Roadmap disposition

M002 is closed. M003 is now dependency-ready because it can emit the accepted
runtime artifact contract. M004 remains ready from the M001 closure. M005
remains blocked on M003 and M004.

## 12. Registry updates

The dependency audit checked all registered tool-selection advisor plans:

- M003 moved from `blocked` to `ready`.
- M004 remains `ready`.
- M005 remains `blocked` on M003 and M004.

No corrective pass is required. The framework decision is intentionally kept as
an empirical M003 input rather than introducing an ADR prematurely.
