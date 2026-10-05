# Decision-Model Extraction and Runtime Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/decision-model-extraction-runtime/001-decision-contract-and-codegg-adapter.md`

Source subsystem roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m001--backend-neutral-decision-contract-and-codegg-adapter-seam`

Repository baseline reviewed: `533be5941ac48334743de555cda96d8695cc6527`

Implementation commits:

- `731dfa4` — introduce the dependency-light decision contract, validation, Noop engine, adapter, and frozen fixtures.
- `2d0410b` — align adapter authority coverage with the resolved surface's required/never-reduce policy.

## 1. Executive finding

M001 is complete. CodeGG now has a bounded, versioned decision contract below the tool-advisor implementation and an additive adapter that only projects candidates already present on `ResolvedToolSurface`. Existing live advisor and disclosure behavior remains unchanged. No backend is authoritative.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Binary, Choice, Score, and multi-relevance Rank remain distinct | `codegg_core::decision` contract and unit matrix | pass | Choice requires at least two named options; Rank carries independent relevance/ranking scores and a multi-relevance flag. |
| Requests and answers enforce identity, schema, bounds, finite scores, and exact Rank identity | decision module validation tests | pass | Includes duplicate ids, oversize state, non-finite score range, schema/request mismatch, and incomplete candidate set rejection. |
| Unsupported, unavailable, and abstained outcomes are explicit | Noop unit test and frozen v1 fixtures | pass | Non-answer states cannot carry an answer; status reasons are bounded. |
| Noop needs no model, network, or artifact | `noop_is_off_and_returns_explicit_unavailable_without_io` | pass | Noop reports Off, no capabilities, and an immediate Unavailable result. |
| Tool candidates cannot exceed CodeGG authority | `adapter_cannot_project_denied_or_non_surface_tools`; `agent::tool_surface` tests | pass | A real resolved surface was supplied with denied, absent, required/never-reduce, ordinary, and synthetic MCP identities. Only ordinary allowed entries projected. |
| Frozen compatibility fixture set and source provenance | `assets/decision-runtime/compatibility-v1.jsonl`; `decision_runtime_fixtures` | pass | Covers all semantic families, multiple/single/no relevance, synthetic identity, candidate-order permutation, unsupported/unavailable, and oversized rejection. Four source cases are content-fingerprinted against the untouched historical corpus. |
| Existing advisor prediction maps without acquiring actuation authority | `legacy_prediction_maps_to_exact_generic_candidate_set`; abstention/unknown-name test | pass | Compatibility-only conversion; no production call site was added. |
| Default dependency graph remains free of new ML/network dependencies | implementation diff and `Cargo.toml`/`Cargo.lock` unchanged | pass | The generic contract uses dependencies already present in `codegg-core`. |
| Live ranking/disclosure remains unchanged | diff/call-site review; adapter has no production caller | pass | No behavior or configuration switch was added. |

## 3. Production implementation evidence

`crates/codegg-core/src/decision.rs` owns the v1 request/spec/response domain, semantic validation, canonical fingerprints, backend capability/state types, the async `DecisionEngine` interface, and `NoopDecisionEngine`. It imports no model framework, HTTP client, tool-advisor code, or presentation layer.

`src/tool_advisor/decision_adapter.rs` consumes a caller-provided candidate name set and an existing `ResolvedToolSurface`; it does not query a registry or hold permission, broker, or execution references. Its legacy-prediction converter exists only for compatibility fixtures/tests. There are no new live callers.

The frozen fixture generator and validator live at `scripts/generate_decision_runtime_fixtures.py` and `crates/codegg-core/tests/decision_runtime_fixtures.rs`. `architecture/tool-advisor.md` and `architecture/overview.md` describe the boundary and temporary coexistence with the old runtime.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo test --locked -p codegg-core decision::tests -- --nocapture
rtk cargo test --locked -p codegg-core --test decision_runtime_fixtures -- --nocapture
rtk env RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' cargo test --locked -p codegg --lib tool_advisor
rtk env RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' cargo test --locked -p codegg-core
rtk env RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' cargo test --locked -p codegg --lib agent::tool_surface
rtk cargo check --locked --tests -p codegg
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk scripts/verify.sh quick
rtk git diff --check
rtk python3 scripts/generate_decision_runtime_fixtures.py
```

### Results

- Decision contract unit matrix: 6 passed.
- Frozen decision fixture validator: 1 passed.
- Full tool-advisor test filter: 176 passed, 0 failed, 6 ignored.
- Full `codegg-core` suite: 824 unit tests plus 55 integration tests passed; doc tests passed.
- Resolved-tool-surface tests: 5 passed.
- Root `cargo check --tests`: passed.
- Workspace Clippy (`-D warnings`): passed.
- `scripts/verify.sh quick`: passed, including core-boundary and workspace all-target checks.
- Fixture regeneration was run twice with no resulting diff; fixture source fingerprints verified.
- The host's default MacPorts library search resolves arm64 native libraries during x86_64 links. Explicit Homebrew/SDK library paths allowed the root tests to link and run. The linker emitted ignored-arm64-library warnings; no test or verification command failed after the explicit paths were supplied.

## 5. Invariant review

- `ResolvedToolSurface` remains the only candidate and authority ceiling.
- The decision result cannot enable, authorize, register, execute, or synthesize a tool.
- Noop/off uses no model, artifact, transport, or network access.
- Causal-frontier behavior and historical frozen advisor evidence are unchanged.
- Multi-relevance Rank is not coerced into exclusive Choice.
- New request data is bounded before dispatch, and fingerprints are stable under semantically unordered candidate/state ordering.

## 6. Failure and recovery review

There is no durable state or external side effect in M001. Request validation fails before an engine call; backends receive a caller deadline and remain cancellable by dropping the future. Noop returns immediately. Unsupported, unavailable, and abstained responses carry no answer, leaving any fallback decision to CodeGG policy.

## 7. Migration and compatibility review

This is an additive internal Rust API with schema version 1. No database, user configuration, protocol, or artifact migration was introduced. Existing tool-advisor config, feature flags, runtime, and assets remain unchanged. Historical advisor case files were not rewritten; fixtures record source hashes.

## 8. Security review

The adapter receives only bounded context and candidates already admitted by `ResolvedToolSurface`; its type has no registry, permission, broker, or executor reference. The contract adds no transport or secret handling path. Candidate descriptions, state, and status reasons have byte bounds; numeric fields reject non-finite values.

## 9. Documentation and operations

- Updated `architecture/tool-advisor.md` and the `architecture/overview.md` crate map.
- Added deterministic fixture generation and a default-build integration test that validates fixture schema, provenance, semantics, and fingerprints.
- No operator configuration or recovery procedure is required in this additive milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | No open correctness, security, migration, or operational findings. | None. |

## 11. Roadmap disposition

M001 is closed. M004 is unblocked because M001 was its only hard dependency and the backend-neutral interface is stable. M002 remains blocked only on the maintainer-approved external decision-model repository and its license/release policy. M003 remains blocked on positive M002 runtime/artifact evidence. M005 and M006 remain blocked by their stated dependency graphs.

## 12. Registry updates

- Move M001 from closure review to recently closed.
- Mark the subsystem roadmap active with M001 closed, M004 ready, and M002 blocked on its external destination.
- Register M004 as dependency-ready and set its plan status to ready.
- Remove M001 and M004 from blocked work; retain M002, M003, M005, and M006 with their precise remaining blockers.
