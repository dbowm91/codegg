# Search and Eggsearch Integration — Deep-Research Corrective Addendum

Status: active

Source roadmap:

- `plans/subsystems/search-eggsearch-integration-roadmap.md`

Historical closure records retained without revision:

- `plans/closure/search-eggsearch-integration/001-status.md`
- `plans/closure/search-eggsearch-integration/002-status.md`
- `plans/closure/search-eggsearch-integration/003-status.md`

Normative planning reference:

- `plans/003-planning-process.md#7-corrective-passes`

Current corrective implementation plan:

- `plans/implementation/search-eggsearch-integration/004-deep-research-structured-consumption-corrective-pass.md`

## 1. Corrective trigger

A post-closure review on 2026-08-16 found a narrower correctness defect that the accepted M001–M003 evidence did not exercise.

The M002 consolidation correctly removed direct Exa/Tavily/Brave/SerpAPI/Kagi ownership and made eggsearch the normal external-search owner. M003 correctly added a lossless structured MCP/search-backend path and demonstrated that the CodeGG wrapper set can reach eggsearch 0.3.6 through a real MCP process. Those implementation results remain accepted historical evidence.

The later review found that CodeGG deep research does not actually consume that structured path:

1. `src/research/sources/eggsearch.rs` calls the string-returning `dispatch_research_search` / `dispatch_security_search` functions, strips CodeGG's trust frame, and reparses the bounded model projection.
2. The research converter looks for top-level `sources`, `papers`, `results`, `hits`, `items`, or `vulns`, while eggsearch 0.3.6 `ResearchSearchResponse` returns source cards under `groups[*].results`.
3. A successful current `research_search` response can therefore produce zero CodeGG `SourceRecord`s without an upstream error.
4. The adapter forwards CodeGG-internal research mode names such as `landscape`, `library_evaluation`, `api_investigation`, `debugging`, `spec_digest`, and `narrow_answer` as eggsearch `workflow` values even though eggsearch's research workflow vocabulary is different.
5. The M003 real-process smoke invokes the raw `ResearchSearchTool` with a simple query; it does not exercise `ResearchCoordinator -> EggsearchSource -> SourceRecord` conversion or CodeGG research-mode workflow mapping.
6. The retained `codesearch` compatibility alias routes through eggsearch correctly but its structured execution path still delegates to the string result and therefore drops the upstream structured repo-search value.

This is a corrective-pass trigger under the planning process. It does not justify rewriting M003's accepted closure record or reopening the already-correct request-contract/provider-ownership work.

## 2. Preserved invariants

M004 MUST preserve all accepted M001–M003 invariants:

- eggsearch remains the default external search backend;
- `fallback_to_builtin` remains false by default;
- raw `mcp__eggsearch__*` tools remain hidden by default;
- `src/search/*` remains explicit compatibility fallback only;
- no direct Exa/Tavily/Brave/SerpAPI/Kagi execution path is reintroduced;
- CodeGG retains research orchestration, budgeting, local-source collection, synthesis, persistence, and report generation;
- eggsearch retains external provider selection, credentials, retrieval, result identity, trust markers, warnings, routing, and evidence semantics;
- external evidence remains `external_untrusted` data rather than instruction-trusted content;
- the shared MCP process/service remains the execution owner;
- no new storage schema, background search worker, provider abstraction, CI lane, scheduled compatibility job, version matrix, source scanner, release gate, or release automation is added.

## 3. Corrective milestone dependency

```text
M001 — request contract repair             closed
   |
M002 — external ownership consolidation    closed
   |
M003 — structured MCP compatibility        historical closed evidence
   |
   | corrective defect discovered after closure
   v
M004 — deep-research structured-consumption corrective pass
```

M004 is dependency-ready. It depends only on the already-landed M002/M003 interfaces present on the current branch and the documented eggsearch 0.3.6 structured response contract.

No external/network evidence blocks implementation.

## 4. Milestone 004 — Deep-research structured-consumption corrective pass

Class: capability correctness / integration closure

Objective:

Make the normal CodeGG deep-research consumer use the lossless eggsearch structured result, convert the actual current grouped research response into `SourceRecord`s, send only supported eggsearch workflow values, and retain structured evidence through the `codesearch` compatibility alias.

Required deliverables:

- `EggsearchSource` consumes the structured search-backend result before any display truncation/framing;
- current eggsearch `groups[*].results` source cards are flattened deterministically into CodeGG research sources;
- security-review external collection consumes the current structured security result rather than reparsing framed display text;
- every CodeGG `ResearchMode` either maps to a documented supported upstream workflow or deliberately omits/uses the upstream general workflow when there is no faithful specialized equivalent;
- the compatibility `codesearch` tool's `execute_structured()` path retains the upstream repo-search JSON value;
- a regression test exercises the real CodeGG research-source/coordinator consumer path against a current-shaped fake eggsearch response;
- truncated model-facing text cannot cause loss of a structured research result;
- documentation and registry state identify M004 as the controlling corrective milestone.

## 5. Scope limits

M004 MUST NOT:

- redesign CodeGG claim extraction, synthesis, persistence, or report rendering;
- introduce typed copies of the entire eggsearch response model when a narrow `serde_json::Value` consumer is sufficient;
- auto-execute eggsearch `next_actions`;
- broaden provider configuration or add provider-specific credentials to CodeGG;
- delete the explicit legacy generic builtin fallback;
- redesign generic MCP protocol handling beyond using the structured surface M003 already added;
- add network-dependent CI or rerun broad compatibility matrices.

If implementation proves that the current `SourceRecord` model cannot represent the minimum required external-source identity/provenance without a durable storage migration, stop and split that migration into a separate research plan rather than expanding M004 silently.

## 6. Verification policy

Verification remains deliberately narrow:

1. focused unit tests for CodeGG `ResearchMode` -> eggsearch workflow mapping;
2. focused conversion tests using current-shaped `ResearchSearchResponse` and security-search JSON fixtures;
3. one fake-MCP integration path through the CodeGG research source/coordinator proving a successful grouped response produces `SourceRecord`s;
4. one regression proving structured conversion succeeds even when the model-facing projection is truncated;
5. focused `codesearch` structured-result assertion;
6. `cargo fmt --all -- --check` and `git diff --check`;
7. `scripts/verify.sh quick` before closure.

The existing M003 real eggsearch 0.3.6 smoke remains accepted wrapper-level compatibility evidence. M004 does not require a new permanent network test or CI lane. A local real-process coordinator smoke is optional only if implementation evidence leaves ambiguity after the deterministic consumer-path tests.

## 7. Completion definition

This corrective addendum returns to `closed` only when:

- M004 has an accepted closure record under `plans/closure/search-eggsearch-integration/`;
- a current grouped eggsearch research response produces the expected non-zero CodeGG sources when source cards are present;
- no supported deep-research path reparses a truncated trust-framed projection when a structured value is available;
- no unsupported CodeGG-internal research workflow string is sent upstream;
- security-review collection uses the same structured evidence discipline;
- `codesearch` preserves structured repo-search metadata;
- the focused verification set and `scripts/verify.sh quick` are green;
- no new external-search ownership path or verification overengineering was introduced.

Until then, M001–M003 remain historical accepted evidence, but the search/eggsearch subsystem's strict current disposition is controlled by M004 rather than the earlier closed roadmap statement.
