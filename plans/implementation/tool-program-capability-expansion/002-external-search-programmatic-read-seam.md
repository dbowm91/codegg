# Tool Program Capability Expansion Milestone 002 — External Repository Search Programmatic Read Seam

Status: ready for handoff (unblocked by M001 closure at `plans/closure/tool-program-capability-expansion/001-status.md`)

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/tool-program-capability-expansion-roadmap.md#M002--external-repository-search-programmatic-seam`

Long-term requirements: `plans/000-long-term-specification.md#46-progressive-disclosure`, `#47-correctness-before-transparent-magic`, `#27-security-requirements`.

Applicable ADRs: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`. Primary class: capability.

## 1. Objective

Allow Tool Programs to invoke canonical `repo_search` through the existing broker while preserving `SearchRuntimeContext`, external-untrusted provenance, network/backend policy, bounded results and explicitly truthful nondeterministic cache/replay behavior.

## 2. Why this milestone is ready

Unblocked by M001 closure (`plans/closure/tool-program-capability-expansion/001-status.md`). Search/eggsearch integration and explicit runtime context are closed; `RepoSearchTool` already has structured output/provenance and is read-only but lacks a programmatic caller contract.

## 3. Current implementation evidence

`RepoSearchTool` owns a `SearchRuntimeContext`, dispatches structured repository search and emits MCP/external-untrusted provenance with truncation. It is a read-only tool but uses default legacy caller policy. Tool Programs already ledger brokered call results and have cache/replay semantics that must be reviewed for changing external results.

## 4. Invariants that must not regress

No mutable/global search installation; external results remain `ExternalUntrusted`; SSRF/network/backend policy unchanged; program authority cannot choose hidden provider credentials; result count/payload bounded; no claim that repeated external search is deterministic; broker/ledger remains canonical.

## 5. Scope

In: repo_search programmatic contract, runtime-context threading into program registry, explicit cache/retry/replay declaration, program manifest/ledger/provenance, network/policy/trust negatives. Out: websearch/webfetch/research tools, search backend redesign, persistent indexing, programmatic arbitrary URLs.

## 6. Required production changes

Contract: classify repo_search programmatic read and declare nondeterministic/external cache/retry semantics supported by current contract model. Runtime: program registries receive same daemon-owned `SearchRuntimeContext`; no isolated default silently replaces configured service. Broker/security: ordinary tool policy still applies. Result: preserve structured source/provenance/truncation. If contract cannot express safe replay/cache, leave programmatic support disabled and close as blocked/corrective decision rather than weakening semantics.

## 7. Ordered work packages

A — apply M001 eligibility matrix to repo_search and document nondeterminism/trust decisions.

B — thread explicit search runtime into Tool Program callable registry/manifest if not already present.

C — enable caller policy with conservative retry/cache behavior and ledger replay semantics.

D — program search fixture, unavailable-backend/policy/SSRF/trust/size/replay tests.

E — docs/diagnostics expose external-untrusted behavior.

## 8. Failure, cancellation, restart, and contention semantics

Backend unavailable/timeouts yield typed tool failure under existing policy. Cancellation propagates through broker/backend. Recorded program call result is attributable to execution time; rerun may return different search results and must not be mislabeled deterministic. Restart/replay uses existing ledger semantics only.

## 9. Compatibility and migration

Direct repo_search behavior unchanged. Contract hash changes invalidate stale program manifests/cache as designed. No search storage migration.

## 10. Required tests

Program repo_search success; configured runtime context vs isolated default; unavailable backend; external-untrusted provenance; max-results/payload bounds; policy/URL negatives; cancellation; cache disabled/bounded as chosen; ledger replay vs rerun distinction; direct compatibility.

## 11. Required verification commands

```bash
cargo test --workspace repo_search --no-fail-fast
cargo test --test tool_program_runtime
cargo test --test tool_program_cache
cargo test --workspace search_backend --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Tool Programs/search backend/broker architecture and callable palette docs.

## 13. Acceptance criteria

A Tool Program can perform bounded canonical repo search with configured daemon runtime and preserved external-untrusted provenance; cancellation/policy failure is correct; replay/cache docs/tests never imply external determinism; no other network tool becomes callable.

## 14. Stop conditions

M001 not closed; current ToolContract cannot safely express external nondeterminism/replay; support would bypass broker/search runtime or weaken SSRF/trust policy.

## 15. Closure evidence required

M001 closure, eligibility decision, runtime-context proof, trust/policy/cache/replay tests, direct compatibility, exact verification results.

## 16. Handoff notes

A correct outcome may be to leave repo_search unsupported if the existing contract cannot represent safe semantics without architectural expansion; record that rather than adding ad hoc exceptions.
