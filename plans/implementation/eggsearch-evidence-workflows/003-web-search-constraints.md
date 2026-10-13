# Eggsearch Evidence Workflows M003 — Web Search Constraints

Status: blocked (ready after legacy-retirement M001 closes)
Class: capability / compatibility
Planning baseline: CodeGG main `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
Source roadmap: `plans/subsystems/eggsearch-evidence-workflows-roadmap.md`
References: `plans/003-planning-process.md`; `plans/implementation/tool-surface-upstream-compatibility/007-eggsearch-0.3.9-surface-alignment.md`.
Hard dependency: strict legacy-retirement M001 **pre-release development pin/provider-contract and paired-process compatibility** closure; no CodeGG release or installer qualification required.

## 1. Objective

Expose carefully selected eggsearch upstream web discovery constraints through CodeGG's existing stable `websearch` facade, so technical research can request exact dates, domain inclusion/exclusion and language/region scope without pretending upstream can always enforce them.

## 2. Audit evidence

- CodeGG's `src/tool/websearch.rs` currently provides `query, num_results, provider, intent, freshness, safe_search, excerpt_count`.
- Eggsearch 0.4.x `web_search` supports additional provider-neutral controls `date_range`, `include_domains`, `exclude_domains`, `language`, `region`, plus constraint/reporting metadata. `date_range` conflicts with `freshness`; actual restrictions depend on selected provider capabilities.
- The prior M007 0.3.9 work intentionally deferred unrelated fields, so do not treat this as a defect in its closed acceptance.

## 3. Scope and non-goals

In scope: strictly validated optional native fields with fidelity of translation and constraint enforcement reporting, provider capability checks based on current contract where available, and negative tests for conflicting or unsupported restrictions.

Out of scope: new providers, CodeGG-side web scraping/filtering pretending to implement provider limitations, default provider changes, generalized crawler, mandatory geolocation, new cache, frontend selector UI, PDF/headless browser inclusion, extension of research database schema.

## 4. Functional requirements

1. Capture pinned binary `tools/list` schemas and current `docs/tool-matrix.md` examples. Implement the release's exact `date_range` representation rather than guessing inclusive/exclusive date fields. Reject impossible ranges, malformed dates, and simultaneously supplied `date_range`+`freshness` before transport.
2. Add optional `include_domains`, `exclude_domains`, `language`, `region` with well-defined accepted syntax, normalization, caps and conflict validation consistent with eggsearch contract. Reject overlapping inclusion/exclusion rather than silently resolving. No wildcard semantics unless specifically supported upstream.
3. Map every explicitly supplied field 1:1 (CodeGG naming may remain stable) into eggsearch `web_search` JSON. Test absent fields remain absent. Preserve intent, safe-search, excerpts and provider-hint correctness from legacy-retirement M001.
4. Interpret/propagate `constraint_enforcement` / `retrieval_summary` / `structured_warnings` when upstream reports partially enforced, unsupported, provider-skipped or approximated dimensions. Do not present results as exact-date/domain/language filtered unless upstream says so; keep warnings in structured value and model-facing compact status even if result count is zero.
5. Do not auto-fallback from an explicit provider or exact filter to unfiltered search. If an upstream provider cannot enforce a hard requested constraint, follow the upstream typed failure/partial semantics, and expose diagnostics. Do not invent a new policy silently widening search.
6. Preserve original trust and URL/SSRF boundaries. User-supplied domains are constraints, never implicit authorization to fetch those URLs.

## 5. Work packages

- WP1: pin/version capability matrix, syntax and enforcement taxonomy from real binary; capture known unsupported cases.
- WP2: native schema/input validation and backend translation.
- WP3: projection/reporting of enforced vs approximate vs unsupported and retrieval completeness.
- WP4: fake-MCP positive, failure, partial enforcement, alias/regression fixtures; docs and CLI/tool descriptions.

No generic custom client-side result filter is needed: eggsearch is the sole search engine.

## 6. Verification

```bash
cargo fmt --all -- --check
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Add focused table-driven cases for malformed date ranges, reversed bounds, freshness/date mutual exclusion, include/exclude domain contradictions, locale validation, unsupported providers, partial constraint enforcement, returned empty results with failed providers, and compatibility with prior `websearch` requests. Use deterministic offline fixtures; optional real-process smoke never gates routine CI.

## 7. Compatibility, migration, security and concurrency

New fields optional; old requests preserve behavior and names. No storage/protocol migration anticipated. Existing agent permission and network policy remain in force, including explicit disabled search. Limit nested array/string sizes before dispatch to avoid abusing MCP/output budgets. Runtime timeout/cancel behavior unchanged; no extra tool retries or programmatic follow-ups.

## 8. Acceptance, stop and closure

**Accept:** supported constraints translate exactly and are validated; capability limitations are visible and truthful; positive/unavailable/no-match states differ; old fixtures pass; broad verification green.

**Stop:** if pinned release lacks advertised constraints/enforcement evidence needed to distinguish precise vs approximate filtering. Record deferral rather than implement unsupported guesses.

Closure: `plans/closure/eggsearch-evidence-workflows/003-status.md` with field/schema inventory, negative and partial-provider matrix, model-facing constraint warnings, exact test results and residual limitations.
