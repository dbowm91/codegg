# Eggsearch Evidence Workflows C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggsearch-evidence-workflows-corrective/001-tagged-retrieval-dimension-normalization.md`

Source subsystem roadmaps:

- `plans/subsystems/eggsearch-evidence-workflows-post-closure-corrective-addendum.md`
- `plans/subsystems/eggsearch-evidence-workflows-roadmap.md`

Repository baseline reviewed: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03` (after rebase onto the six upstream planning commits)

Implementation commits:

- `234dde73c4f3fc41ae9455490c178b751494080c` — normalize tagged retrieval dimensions, add source-budget fidelity and report coverage, fixtures, tests, and architecture contract.
- This closure record and registry transition are recorded in the following commit.

Upstream contract authority: `eggstack/eggsearch` immutable tag `v0.4.1`, commit `32769d684417567dfb30fc5adea1475cee7f0ed0`.

## 1. Executive finding

C001 strictly closes. CodeGG now reads the published v0.4.1 dimensional retrieval contract instead of treating a scalar `retrieval_summary.status` or `complete` as authoritative. A complete empty result requires positive per-dimension completion evidence; failed, skipped, unavailable, interrupted, partial, unknown, conflicting, count-incomplete, or uncertain-limit work cannot become a negative finding. Completed no-match remains a successful result scoped to represented evidence roles. Historical M001 closure remains unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Freeze tagged contract and distinguish native tool envelopes | Immutable `v0.4.1` source review: `src/core/retrieval_status.rs`, `src/mcp/tools/research_search.rs`, `src/mcp/tools/security_search.rs`, and `src/mcp/projection.rs`; tagged `serde_roundtrip_response_retrieval_summary` passed; frozen representative fixtures in `tests/fixtures/eggsearch/v0.4.1/` | pass | Both diagnostic tools contain `retrieval_summary.dimensions`; security keeps its vulnerability envelope, research its subquery envelope. Compact projection removes dimensions and `workflow_coverage`, retaining only aggregate `retrieval_status` hints. Fixtures are contract-derived minimal envelopes, not captured live-process output. |
| Preserve legacy compatibility separately | `tests/fixtures/eggsearch/v0.3.9/research_search-text-only-compat.json` and `legacy_v039_text_only_compat_fixture_stays_unknown` | pass | This is an explicitly labeled compatibility fixture, not a capture from a v0.3.9 binary. Text-only/missing positive status remains unknown. |
| Normalize dimensions conservatively | `receipt_from_value`; `tagged_*` tests in `src/research/sources/eggsearch.rs` | pass | Covers exact states, fallback from `attempt_outcome`, malformed/unknown dimensions, duplicate-independent reduction, dimension/state conflicts, flags/counts, failed provider extraction, and a 256-dimension processing cap. |
| Distinguish completed empty, all failed, and mixed success/failure | Focused adapter tests plus fake-MCP end-to-end tests in `tests/fake_eggsearch_mcp.rs` | pass | Completed no-match renders as complete and explicitly scoped; failed-only dimensions render failed; mixed results retain usable source cards as partial. |
| Preserve skipped, capability, interruption, partial, truncation, unknown limit and future enum states | Table-driven adapter tests and report assertions | pass | Confirmed truncation renders truncated; `limit_reached_unknown` remains unknown; policy-only skip is skipped; capability and unknown states remain indeterminate. |
| Preserve failure despite apparently complete cards and contradictory flags | `tagged_false_success_overrides_complete_dimensions_but_keeps_cards`; conflict and `has_failures=false` tests | pass | Tool failure remains partial when cards can be salvaged. Conflicting dimension/summary evidence receives bounded warning codes. |
| Bound source-budget truncation separately from display truncation | `apply_source_budget` and `coordinator_source_budget_cap_marks_receipt_and_retained_links`; existing structured-value-over-display-truncation test | pass | The source cap marks the receipt truncated and retains only source links for retained cards. Full structured values remain authoritative over display text truncation. |
| Preserve report and old stored-run compatibility | Report assertions for receipt states; `research::store` round-trip and pre-receipt artifact compatibility tests | pass | Complete empty wording explicitly limits absence to represented retrieval scope. Incomplete states say absence is not a negative finding. No storage schema change was needed. |
| Keep authority, trust, cancellation and concurrency unchanged | Adapter still uses the shared `SearchRuntimeContext` structured dispatch; no new transport, process, task, retry, storage, or policy path; `SourceRecord.external_untrusted` behavior remains | pass | Upstream metadata remains bounded data and cannot grant authority. No cancellation or global state changes. |
| Architecture documentation matches source | `architecture/research.md` updated with v0.4.1 contract and compact-projection limits | pass | The previous unqualified zero-source statement is corrected. No research-specific skill exists to update. |

## 3. Production implementation evidence

Commit `234dde73` adds a deterministic dimension reducer in `src/research/sources/eggsearch.rs`. It recognizes the tagged state and attempt enums, confirmed and uncertain truncation, job/dimension/role counters, workflow coverage gaps/failures, `success=false`, provider failures inside dimensions, and top-level provider failures. It rejects scalar fields under `retrieval_summary` as completion evidence, while retaining historical explicit `retrieval_state` compatibility.

The reducer aggregates all represented dimensions without order-dependent overwrites, caps processing at 256 dimensions, bounds/deduplicates provider labels and warning codes, and marks malformed, conflicting, missing, or future states indeterminate. A `satisfied` state without a retained source record is not treated as an empty successful search. `completed_no_match` is complete only for its represented retrieval scope.

The existing adapter → coordinator → receipt/store/report path is retained. Source-budget truncation now uses a shared production helper and removes receipt links for dropped cards. No persisted field or migration was added. `architecture/research.md` records the tagged contract and compact-projection caveat.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
rtk cargo test -p codegg --lib research:: -- --test-threads=1
rtk cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
rtk cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
rtk cargo test -p codegg --lib research::store -- --test-threads=1
rtk cargo clippy -p codegg --all-targets --locked -- -D warnings
rtk scripts/verify.sh quick
rtk git diff --check
rtk cargo test --manifest-path /tmp/eggsearch-v041-contract/Cargo.toml --lib core::retrieval_status::tests::serde_roundtrip_response_retrieval_summary -- --exact
```

### Results

- Focused adapter tests: 18 passed.
- Research unit tests: 139 passed.
- Full fake Eggsearch MCP suite: 30 passed, including two tagged diagnostic end-to-end cases; both tagged-only cases also passed on the final code.
- Eggsearch backend tests: 9 passed.
- Research store tests: 9 passed.
- All-target root clippy: no issues.
- `scripts/verify.sh quick`: passed all formatting, schema, boundary, policy, provider, routing and workspace-check stages.
- Tagged upstream v0.4.1 retrieval-summary serialization test: 1 passed. Its temporary detached worktree was removed.
- `git diff --check`: passed.
- No live v0.4.1 process/binary smoke was run. No hosted CI or all-features run is claimed.
- One early fake-MCP run had an incorrect test expectation (`exa` for the security fixture, whose provider is `osv`); the fixture assertion was corrected and the full final suite passed 30/30. An initial quick-format gate also found unformatted final edits; rustfmt was applied and the final quick run passed.

## 5. Invariant review

- Only positive tagged dimensional completion (or explicitly recognized historical `retrieval_state`) can yield complete. Compact aggregate booleans and absence of cards cannot establish completion.
- Completed no-match is distinguished from provider failure; no result-count shortcut or `absence_kind` inference is used.
- Mixed dimensions, policy/capability skips, interruptions, result caps, malformed/additive enums and contradictory flags remain visible and conservative.
- Failed provider IDs and status codes are bounded and deduplicated; raw provider error messages are not copied into receipt warnings.
- Structured source fields remain external and untrusted. Existing shared MCP session, network gate, cancellation, task ownership and request path are unchanged.
- Storage remains backward-compatible; no database or protocol migration was introduced.

## 6. Failure and recovery review

The reducer is synchronous, local and deterministic. It starts no tasks, retries, network calls or background jobs. Malformed entries do not panic. Oversized diagnostic dimension arrays are capped and marked unknown. Source-budget truncation records the retained count and prunes links to removed sources. Existing task cancellation and MCP runtime lifetime behavior are unchanged.

## 7. Migration and compatibility review

No migration or public tool response change was made. Existing `ResearchRetrievalReceipt` fields and serialized run artifacts remain unchanged. Old runs without `retrieval.json` still decode through the existing default path. The CodeGG managed eggsearch pin remains 0.3.9; the v0.3.9 fixture is separately labeled and does not claim a live binary capture. C001 must be requalified after the separate release-adoption milestone changes that pin.

## 8. Security review

Provider IDs, warning codes, role labels and upstream coverage metadata remain data; bounded labels strip control characters. No raw provider error text is emitted. Result cards retain existing URL scheme checks and `external_untrusted` marking. The correction adds no authority, path, network, or policy surface.

## 9. Documentation and operations

- `architecture/research.md` documents the tagged dimensional contract and scoped empty-result wording.
- Frozen fixtures are under `tests/fixtures/eggsearch/v0.4.1/` and `tests/fixtures/eggsearch/v0.3.9/`.
- No new static script was needed: regression tests ensure scalar `retrieval_summary.status`/`complete` and compact `retrieval_status` cannot establish current-contract completion.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No live v0.4.1 binary smoke was run; fixtures are schema-derived and the 0.3.9 compatibility fixture is labeled rather than binary-captured. | The tagged contract is verified at source and serialization-test level, but runtime packaging evidence is absent. | Optional operational smoke when the immutable binary is available; requalify after the managed pin adoption. |

## 11. Roadmap disposition

C001 is strictly closed. M004's C001 dependency is satisfied. The evidence workflow line has no newly ready successor because the separate `search-eggsearch-legacy-retirement` M001 release-adoption milestone remains ready but unclosed:

- M002 and M003 remain hard-blocked on that release-adoption M001.
- M004 remains hard-blocked on that same M001; C001 is no longer a blocker.
- M005 remains blocked on M002 and M004.

The original `plans/closure/eggsearch-evidence-workflows/001-status.md` remains untouched as historical evidence. No additional plan in this line is eligible until the named external dependency closes.

## 12. Registry updates

- Mark corrective C001 closed with this record.
- Preserve the base roadmap's active state and M002–M005 dependency order.
- Update M004 to show C001 satisfied and legacy-retirement M001 as its remaining hard blocker.
- Keep legacy-retirement M001/M002 ownership and closure records in their separate workstream.
