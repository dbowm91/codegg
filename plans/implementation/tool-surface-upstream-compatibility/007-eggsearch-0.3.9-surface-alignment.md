# Tool-Surface Upstream Compatibility M007 — Eggsearch 0.3.9 Surface Alignment

Status: ready

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Planning branch: `agent/tool-surface-compat-2026-09`

Source corrective addendum:

- `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`

Hard dependency:

- M006 generic MCP modern protocol and metadata compatibility must close before M007 closes. Request translation work may proceed against fixtures while M006 is active.

Predecessor evidence:

- `plans/subsystems/search-eggsearch-integration-roadmap.md`
- `plans/closure/search-eggsearch-integration/005-status.md`
- `src/search_backend/eggsearch.rs`
- `src/research/sources/eggsearch.rs`

Audited upstream baseline: eggsearch `0.3.9`.

## 1. Objective

Bring CodeGG's stable eggsearch facade forward from the previously audited 0.3.6-era contract to the high-value additive 0.3.9 surface without turning CodeGG into a mirror of every upstream option.

This milestone should improve evidence quality and cache/fetch control while preserving CodeGG's established ownership of model-facing names, context limits, trust framing, and progressive disclosure.

## 2. Current implementation evidence

`src/search_backend/eggsearch.rs` currently translates:

- `web_search`: query, result limit, provider hints, intent, freshness, safe-search;
- `web_fetch`: URL, max chars, extract mode, include links;
- batch web items: URL, extract mode, include links, max chars;
- repository/security/research/evidence requests through compatibility translation established by the prior workstream.

The current adapter does not expose newer eggsearch controls including:

- `excerpt_count` on search;
- `focus` on fetch;
- `cache_policy` and tightening-only `max_cache_age_seconds` on fetch;
- per-item cache policy on batch fetch where supported;
- explicit `response_detail` projection policy.

CodeGG already preserves structured eggsearch values separately from bounded display output. That invariant should be reused rather than adding a second response parser.

## 3. Non-goals

M007 MUST NOT:

- mirror every 0.3.9 provider-specific field;
- add Tavily/Exa/Brave/etc. HTTP clients to CodeGG;
- change provider ownership established by the closed search roadmap;
- auto-execute `next_actions`;
- introduce an eggsearch-specific persistent cache in CodeGG;
- expose raw MCP tools by default;
- make compact output the only internally stored representation;
- redesign research synthesis or evidence persistence;
- delete the configured built-in compatibility fallback;
- add network-dependent normal CI.

## 4. Invariants

- Full structured upstream data needed for provenance/evidence decisions remains available internally.
- Model-visible text stays bounded and trust-framed.
- Compact response projection must not be interpreted as “no evidence” when retrieval metadata reports failures or partial coverage.
- Existing stable IDs, structured warnings, trust markers, routing/retrieval state, suggested fetches, and next actions remain data, not control authority.
- Unknown additive fields remain safe.
- Legacy CodeGG aliases that still have unambiguous current semantics continue to translate.
- Unsupported/conflicting arguments fail clearly rather than being silently dropped.

## 5. Required contract audit

Before editing, capture the exact current 0.3.9 `inputSchema`/`outputSchema` for every CodeGG-wrapped eggsearch tool:

- `web_search`;
- `web_fetch`;
- `repo_search`;
- `repo_fetch`;
- `repo_map`;
- `security_search`;
- `research_search`;
- `batch_fetch`;
- `build_evidence_bundle`;
- `provider_status` where CodeGG consumes it diagnostically.

Classify each upstream field as:

- already represented correctly;
- high-value additive field to expose now;
- intentionally internal/defaulted upstream behavior;
- provider-specific/low-value field intentionally deferred;
- stale CodeGG alias requiring translation/removal.

The implementation/closure record should retain this mapping as concise evidence; do not create a permanently maintained duplicate schema document.

## 6. Expected production-code changes

Primary scope:

- `src/search_backend/eggsearch.rs`;
- CodeGG native tool parameter schemas for web/search/fetch wrappers;
- batch-fetch item schema/translation;
- tests under `tests/search_backend_*` and fake MCP fixtures;
- architecture/search docs where supported controls are described.

### 6.1 Search excerpt control

Expose `excerpt_count` where the native CodeGG search facade can pass it without ambiguity.

Requirements:

- validate the current upstream range (0-3 at the audited baseline);
- do not silently clamp invalid explicit values unless that matches existing CodeGG parameter policy; prefer actionable validation for out-of-contract values;
- absence preserves upstream/default behavior;
- tests prove zero and nonzero values survive translation.

Do not expose provider-specific highlight knobs separately; `excerpt_count` is the provider-neutral contract.

### 6.2 Query-focused fetch

Expose eggsearch `focus` on `webfetch` and any CodeGG path that intentionally performs a web fetch.

Requirements:

- non-empty string validation;
- focus is additive extraction/ranking guidance, not a new search request;
- no extra traversal or follow-up fetch should be implemented in CodeGG;
- preserve normal max-char and trust framing behavior.

### 6.3 Cache controls

Expose provider-neutral fetch cache controls where they can be represented faithfully:

- `cache_policy`;
- `max_cache_age_seconds` with upstream tightening-only semantics;
- per-item cache policy for batch web items if 0.3.9 schema supports it.

Requirements:

- validate accepted enum/value forms against current schema;
- no CodeGG-owned duplicate cache layer;
- explicit user/model request must not be silently weakened;
- batch normalization must retain per-item controls rather than dropping them.

### 6.4 Response-detail policy

Current eggsearch supports `response_detail = compact | standard | diagnostic` for search/fetch tools except diagnostic-only surfaces, with evidence bundles semantically canonical across modes.

Implement one deliberate CodeGG policy. Preferred design:

- internal structured result remains the authoritative full/canonical value available from the upstream call path;
- model-facing wrapper requests may choose a lower-detail projection only when it does not discard data CodeGG needs internally;
- if eggsearch's projection occurs before CodeGG receives structured content, default to the least verbose detail level that still preserves CodeGG's required internal evidence contract, otherwise use `diagnostic` internally and let CodeGG perform its existing display framing;
- make the choice explicit in code/config rather than relying on an upstream default that may change.

Do not add a user-facing configuration option unless there is a demonstrated need. A stable internal policy is preferable for this milestone.

### 6.5 Structured warning / failure-state fidelity

Add regression fixtures proving that response projection does not lose or misinterpret:

- `structured_warnings`;
- `providers_failed` / retrieval failure state;
- trust markers;
- stable IDs;
- next actions / suggested fetches;
- unknown additive metadata.

No automatic next-action execution is authorized.

### 6.6 Modern metadata use

After M006, use output schemas/annotations/discovery metadata for compatibility diagnostics and tool-catalog fidelity where helpful. Do not duplicate CodeGG's own discovery UI or expose raw eggsearch metadata directly to the model.

## 7. Ordered work packages

### WP1 — Capture exact 0.3.9 wrapper matrix

1. Inspect current eggsearch schemas/contract docs.
2. Compare each CodeGG builder/schema against them.
3. Record only material drift and intentional deferrals.
4. Stop if a supposedly compatible existing wrapper is now actually broken; elevate that defect within M007 rather than treating it as polish.

### WP2 — Add provider-neutral search/fetch controls

1. Add `excerpt_count` translation/schema.
2. Add `focus` translation/schema.
3. Add fetch cache controls.
4. Add batch-item cache controls where supported.
5. Add strict input validation and conflict tests.

### WP3 — Establish explicit response-detail behavior

1. Determine where eggsearch projection occurs relative to CodeGG's structured-value capture.
2. Select and document the CodeGG internal policy.
3. Add regression tests for full internal evidence versus bounded display output.
4. Ensure partial/failure retrieval state survives.

### WP4 — Compatibility fixture hardening

Make fake MCP fixtures validate real current field names/enums/ranges rather than accepting arbitrary argument JSON. Cover both accepted mappings and rejected stale/conflicting fields.

Do not copy the entire upstream JSON Schema engine into tests; targeted assertions are sufficient.

### WP5 — Documentation and diagnostics

Update search/tool architecture docs to name eggsearch 0.3.9 as the audited compatibility baseline and describe the selected high-value controls. If `doctor search` reports server/tool contract details, include protocol/server version and enough tool-schema capability information to diagnose an older installation without dumping full schemas.

## 8. Focused verification

Expected commands, adapting exact target names to current repository reality:

```bash
cargo fmt --all -- --check
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --lib research::sources::eggsearch -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

A one-time local eggsearch 0.3.9 smoke is desirable closure evidence if available. It must not become ordinary CI.

## 9. Acceptance criteria

M007 is implementation-complete when:

- every CodeGG eggsearch wrapper has been compared against 0.3.9;
- no discovered current incompatibility remains unclassified;
- `excerpt_count`, `focus`, and supported cache controls reach upstream correctly;
- response-detail behavior is explicit and preserves CodeGG's structured evidence needs;
- compact/partial responses cannot masquerade as complete evidence;
- current structured warnings/trust/stable-ID/retrieval metadata survive the wrapper boundary;
- no direct provider HTTP path is reintroduced;
- focused tests and `scripts/verify.sh quick` pass.

## 10. Stop conditions

Stop and record a blocker if:

- preserving full structured evidence requires upstream eggsearch changes rather than CodeGG adaptation;
- `response_detail` cannot be reconciled with CodeGG's internal/full versus model-visible/bounded split without a contract change;
- current schemas reveal a breaking change requiring an eggsearch release fix;
- implementation begins growing provider-specific logic or a second cache subsystem.

## 11. Closure evidence required

The closure record must include:

- implementation commit(s);
- audited eggsearch version/commit or release tag;
- concise wrapper compatibility matrix;
- tests for each newly exposed control;
- response-detail/failure-state evidence;
- real-process smoke result if performed;
- verification outcomes;
- intentionally deferred 0.3.9 fields with rationale.
