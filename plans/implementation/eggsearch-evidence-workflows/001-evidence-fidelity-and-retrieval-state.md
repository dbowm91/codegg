# Eggsearch Evidence Workflows M001 — Evidence Fidelity and Retrieval State

Status: ready
Primary class: correctness / infrastructure
Planning baseline: CodeGG `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Source roadmap: `plans/subsystems/eggsearch-evidence-workflows-roadmap.md#5-milestones-and-handoffs`
Predecessor: `plans/closure/search-eggsearch-integration/005-status.md`; `plans/closure/tool-surface-upstream-compatibility/007-status.md`.
Relevant long-term: `plans/000-long-term-specification.md` explicit search ownership and trusted agent loop; `plans/003-planning-process.md` handoff.
ADRs: none required if metadata stays additive and existing research identity/persistence authority is preserved.
Dependency: no hard dependency on the new eggsearch pin; ensure current bundled 0.3.9 fixtures and newer release fixtures are both interpretable.

## 1. Objective

Make deep research consume a truthful structured search outcome rather than reducing every result to an unqualified `Vec<SourceRecord>`. Preserve eggsearch's stable source linkage, nested publication metadata, group/provider origins and retrieval state without changing CodeGG's synthesis and claim graph, nor replacing established local source record identity.

## 2. Audit evidence

- `src/research/sources/eggsearch.rs::convert_structured` takes `StructuredSearchResult.value` or parses a framed string and produces only source records; `success`, provider failure, structured warnings, coverage, suggested fetches and error repair hints do not reach coordinator consumers.
- `source_from_result` reads top-level published fields, not modern `metadata.published_at`, and `Uuid::new_v4()` becomes the local ID. Eggsearch's stable ID becomes a `notes` string.
- `source_from_result` writes `trust=external_untrusted` for every source; local-workspace provenance must be distinguished but must never grant instruction authority.
- `result_items` already traverses modern `groups[*].results`. Keep that behavior; test exact upstream `SourceCard` shape instead of synthetic top-level fields.
- Inspect `src/research/types.rs`, coordinator/claim extraction, research storage and display consumers **before** selecting the smallest additive representation.

## 3. Invariants and non-goals

- Preserve current `SourceRecord.id`, `run_id`, URI and storage/serialization contracts unless an explicit migration is shown necessary. Do not blindly substitute upstream stable IDs for UUIDs or infer globally unique identity from URLs.
- A successful complete zero-result search is absence. Failed, partial, interrupted, unavailable, policy-skipped, unconfirmed-truncated or tool-error retrieval is not negative evidence.
- Salvage useful sources from a partial upstream response when safe, but mark scope and diagnostic uncertainty; an MCP `isError` must never silently become success.
- No execution of `next_actions`; no new search clients, prompt-supplied trust, persistent search cache, or full research engine redesign.
- Local provenance `local_trusted` means checkout provenance, **not** instruction trust; unknown/missing trust fails conservative.
- Network disabled must still fail via existing authority and should not be masked as no-match.

## 4. Expected production seam

Prefer a small typed internal `ResearchRetrievalReceipt` (or equivalent) carrying a bounded status enum, source linkage keyed by existing per-run `SourceRecord.id`, upstream `stable_id`, provider/group, normalized nested timestamp, trust markers, structured warning codes, retrieval coverage and explicit partial/failure state. Decide if the coordinator's adapter trait must return a typed batch containing both records and receipt; ensure all local adapters keep compatible semantics and build. Do not add unbounded raw JSON to persisted notes.

Normalization contract:
- Prefer `metadata.published_at` (valid RFC3339 or documented date-only) with top-level legacy fallback, no timestamp invention.
- Retain upstream stable source IDs and distinct source group/provenance; deduplicate within a run using stable IDs where applicable, handling same URL from distinct sources without false equivalence.
- Parse retrieval_summary/coverage/structured_warnings independently of source-card count and of `success`; preserve unknown states as indeterminate, not successful empty.
- Preserve original source data (bounded excerpt/snippet) and trust markers without accidentally duplicating untrusted framing as instructions.
- If older text-only MCP path cannot provide status, report status as unknown/indeterminate; preserve existing truncation fail-closed behavior.

## 5. Ordered work packages

1. Trace `ResearchSourceAdapter`, coordinator, extraction, persistence and report consumers; freeze additive type/compatibility contract. Document any intentional exclusion of optional metadata.
2. Implement typed normalization and a separate status/receipt carrier. Keep current identifiers and local-adapter behavior stable; use first-party CodeGG types where possible.
3. Carry receipt/status through coordinator to final evidence synthesis/response and diagnostics. The public answer must distinguish evidence coverage gaps/provider failure from an affirmative negative finding; status propagation must not depend on parsing free-text notes.
4. Cover partial MCP tool error with valid source cards, all-provider failure, complete no-match, skipped-by-policy, unconfirmed truncation, duplicate cards and mixed trust.
5. Correct nested publication timestamps (including date-only when guaranteed by upstream) and stable-source-ID association, while leaving prior persisted research results interpretable.

## 6. Verification

```bash
cargo fmt --all -- --check
cargo test --lib research::sources::eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Add focused receipt/coordinator tests using exact eggsearch fixture shape; offline default. Verify `success=false` with nonempty structured value cannot be reported as complete success; no-match only for confirmed successful completed query; missing/unknown enum never causes panic; nested timestamps and source IDs round-trip with repeat collection. Add storage migration tests if storage shape changes.

## 7. Migration, security, concurrency

Do not modify persistent schema without explicit additive migration/old-record read tests. Duplicate/retry retrieval attaches stable IDs deterministically while local IDs remain per-run. Cancellation/timeout propagates unknown/partial status; no global source-state cache or background async work. Keep provider-supplied errors and metadata bounded/redacted. No policy/permission changes.

## 8. Acceptance and stop conditions

**Accept** when a complete no-match, all-provider-failure, partially successful result, and unknown/truncated response yield distinct coordinator/report outcomes; modern nested published timestamp and source identity are retained; unknown additive fields are harmless; normal research paths and local adapters remain stable; focused and quick verification pass.

**Stop** if status cannot reach the synthesis consumer without changing a durable public/storage contract or if an upstream source-card identity inconsistency prevents safe linkage; record the smallest corrective/ADR decision rather than quietly dropping metadata.

Closure: `plans/closure/eggsearch-evidence-workflows/001-status.md` with changed type seam, provenance/ID tests, status matrix, exact fixture source, migration decision, security/cancellation evidence, commands and unresolved findings by severity.
