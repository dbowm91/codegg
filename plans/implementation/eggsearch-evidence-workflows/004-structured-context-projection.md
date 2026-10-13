# Eggsearch Evidence Workflows M004 — Structured Context Projection

Status: blocked (new M001 + legacy-retirement M001)
Class: correctness / performance
Planning baseline: CodeGG main `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
Source roadmap: `plans/subsystems/eggsearch-evidence-workflows-roadmap.md`
References: `plans/003-planning-process.md`; `plans/closure/tool-surface-upstream-compatibility/007-status.md`.
Hard dependencies: this line's `001-evidence-fidelity-and-retrieval-state.md` accepted closure; existing `search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md` accepted closure.

## 1. Objective

Replace raw diagnostic-JSON byte-prefix projection in CodeGG's model-facing eggsearch results with a typed, task-aware, bounded textual/JSON projection that preserves critical search result cards and retrieval uncertainty while keeping the **complete** upstream structured value internally available.

## 2. Audit evidence

`src/search_backend/eggsearch.rs::call_structured_tool_with_service` requests `response_detail="diagnostic"` to retain authoritative upstream structure and separately bounds model-visible `output` with `clamp_output` from `src/search_backend/framing.rs`. This is structurally sound but byte truncation can omit later evidence and warn/no-match controls. Current eggsearch declares `compact | standard | diagnostic` projections, `structured_warnings`, stable IDs, provenance, `retrieval_summary` and suggested next actions.

This milestone **must not** simply request `compact` upstream if that causes CodeGG to lose full `structuredContent`. Measure actual MCP behavior before choosing any upstream-mode change.

## 3. Invariants

- `EggsearchCallResult.value` (or current successor type) preserves all bounded, accepted structured data from MCP, including extra fields, even when user/model-facing representation is compact. This plan does not change permission or provenance authority.
- Warning/status fields that materially affect interpretation survive projection even at small budgets: partial, failed, skipped, stale/unknown, missing credential, unmet exact filter.
- Untrusted source content remains trust-framed; no direct tool metadata or search snippet may present itself as a system instruction. Preserve current text escaper and injection-defense boundaries.
- Never fabricate citations, source IDs or evidence absent upstream; zero available sources after complete success is not the same as zero after partial failure.
- Output budgets are explicit and deterministic; UTF-8 safe, bounded memory/time; truncation is declared, never hidden. No new second cache, output transport, or persisted snapshot.

## 4. Expected production seam

Replace or supplement final `clamp_output` of JSON for eggsearch with one shared projection function consuming the typed upstream structured result. Prefer existing structured SourceCard / metadata helpers and a tool-specific map rather than bespoke whole schema implementations.

Suggested shape:
- Status header: compact `retrieval_status`, queried/failed providers, coverage/constraint caveats, warnings and truncation indication.
- Evidence slots: source title, URL/repo locator, stable_id, provenance/trust, date when available, concise snippet, relevant source group and fetch hint if safe.
- Next-action hints: optional **data only**; show only validated, bounded tool names/arguments necessary for M005; never convert to executable calls here.
- Select results deterministically by structured upstream ranking/order and diversity when already available. Do not invent reranking scores, inferred freshness, or trust boosts.
- Reserve part of the byte/token-proxy budget for status/warnings even with large source lists. Select entire cards that fit the budget, not just a prefix that cuts JSON in half. Mark omitted count and model-visible truncation.
- On malformed, unexpected, text-only, or too-large structured values: bounded safe fallback, explicit unknown status and truncation marker. Do not panic or silently emit `success`.

## 5. Ordered work packages

1. Inventory structured shapes of CodeGG's nine stable wrappers and `provider_status` diagnostics from the **exact** pinned release; use existing 0.3.9 fixtures as backward-compatibility regression cases.
2. Establish a deterministic budget unit (output bytes as existing physical limit, plus optional tokenizer-independent size proxy); measure current raw diagnostic JSON and proposed projections on frozen realistic fixture sets.
3. Implement shared status/card projection with tool-specific enrichers where needed (web/repo/security/research/fetch/batch/evidence). Reuse one trust/framing enforcement point.
4. Wire projection at tool-response serialization, preserve full internal `value`, and ensure generic MCP text-only fallback still respects limits.
5. Capture regression and measurement evidence: token-proxy/byte reduction, source-card coverage, retained warnings, stable IDs and output determinism. Make no minimum-percentage performance claims without benchmark results.

## 6. Verification

```bash
cargo fmt --all -- --check
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --lib research::sources::eggsearch -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Required fixtures: complete success 1/20/large cards, source title with multibyte UTF-8 and prompt injection, 0 sources+partial failure, aggregate result with warnings at end, unknown schema additive fields, nested groups, fetch error, truncated data, missing structured content. Assert full `value` unchanged; projected output within cap; status always readable; no half-JSON ambiguity; deterministic slot order; no model-visible unframed instructions. Run before/after fixture comparison and record exact bytes/card retention (not fabricated token savings).

## 7. Migration, security, concurrency

Model-visible **text** format intentionally changes; native tool names, JSON input schemas and internal result shape stay compatible. No storage/protocol migration. Existing shared MCP transport, timeout and cancellation stay unchanged. Do not schedule new searches to fill budget or parallel fetch suggestion chains. Budget sizes follow current CodeGG policy and enforce validation before allocations.

## 8. Acceptance, stop and closure

**Accept:** projected output preserves typed failure/warning state, more complete relevant cards within bounded budget, internal full structured result remains intact, malicious text cannot escape trust framing, fixtures demonstrate measurable efficiency vs legacy prefix projection, quick and focused tests pass.

**Stop:** if upstream diagnostic structured value is not canonical/complete on pinned release, or exact status semantics cannot be represented without losing internal evidence. Record bounded compatibility corrective instead of making silent compact-mode assumption.

Closure: `plans/closure/eggsearch-evidence-workflows/004-status.md`: before/after corpus metrics, schema versions, full-vs-compact invariant tests, security and cancellation cases, test commands/outcomes, residual risks.
