# Search and Eggsearch Integration Milestone 002 — External Search Ownership Consolidation

Status: active

Repository baseline:

- CodeGG audited baseline: `40dbd1981abf1a8d96d7ab9f5ebefb4b763053f2`
- roadmap addition: `24c4df7ecdf8477cf27d51e0e92acd777d61427d`
- M001 plan addition: `1cd5a465c54e9b7791091e8534a99fc453f656f6`
- eggsearch audited baseline: 0.3.6, release commit `4ccb374af00348bba75761f6bbd1e192d385a2b9`

Source roadmap:

- `plans/subsystems/search-eggsearch-integration-roadmap.md#milestone-002--external-search-ownership-consolidation`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`

Applicable ADRs:

- None. This milestone enforces the existing ownership boundary rather than introducing a new one.

Predecessor plan:

- `plans/implementation/search-eggsearch-integration/001-current-eggsearch-contract-repair.md`

Required predecessor closure:

- `plans/closure/search-eggsearch-integration/001-status.md`

Primary class: invariant / simplification

## 1. Objective

Make eggsearch the sole normal owner of external search-provider execution in CodeGG.

After M001 has made the canonical eggsearch wrapper path compatible with the current upstream contract, M002 must remove the remaining direct external-search bypasses:

1. `codesearch` must no longer call Exa Code API directly from CodeGG;
2. CodeGG's deep-research source collection must no longer call Tavily, Brave, SerpAPI, or Kagi directly;
3. the in-tree `src/search/*` implementation must remain clearly bounded to explicit compatibility fallback rather than functioning as a parallel primary provider stack;
4. tool descriptions, registry behavior, configuration, and documentation must make the one-owner architecture obvious.

The milestone should reduce duplicate code and credentials/provider behavior. It must not replace the duplicate paths with another CodeGG-owned metasearch layer.

## 2. Why this milestone is blocked

M002 has one hard dependency: M001 must be closed first.

Redirecting `codesearch` or research collection into eggsearch before the canonical repo/research/batch request adapters are corrected would move more traffic onto a known-drifting integration and obscure whether subsequent failures come from ownership consolidation or stale request schemas.

Once `plans/closure/search-eggsearch-integration/001-status.md` is accepted, M002 becomes dependency-ready without any other architecture decision.

## 3. Current implementation evidence

### 3.1 Direct Exa `codesearch`

`src/tool/codesearch.rs` is an always-registered read-only tool that:

- reads `EXA_API_KEY` or `EXA_CODE_API_KEY` directly;
- constructs its own `reqwest::Client`;
- validates and calls `https://api.exa.ai/code` directly;
- returns Exa's response directly;
- bypasses `search_backend`, eggsearch provider routing, eggsearch capability reporting, eggsearch trust metadata, and eggsearch keyless fallback.

The tool is registered unconditionally in `src/tool/mod.rs` alongside the eggsearch wrappers.

Current eggsearch `repo_search` already supports a coding profile, host/repository constraints, language/symbol hints, exact-error mode, local workspace evidence, package awareness, provider routing, and generic fallback. Maintaining a separate direct Exa tool no longer establishes a distinct architectural capability.

### 3.2 Direct research-provider clients

`src/research/sources/search_provider.rs` contains direct network clients for:

- Tavily;
- Brave Search;
- SerpAPI;
- Kagi.

These clients own provider-specific URLs, authentication, request/response types, and result shaping inside CodeGG.

At the audited baseline, `ResearchService::build_request` defaults to local sources and `allow_network = false`, so this duplicated network stack is not necessarily exercised by ordinary research calls today. That makes it latent duplication rather than harmless ownership: future or alternate research execution can reactivate a second provider stack with semantics different from the canonical eggsearch path.

Eggsearch now provides `research_search`, `repo_search`, `security_search`, `web_search`, `web_fetch`, and `batch_fetch`, with provider routing and evidence metadata specifically intended for agent harnesses.

### 3.3 Legacy built-in web search

`src/search/*` retains the pre-eggsearch provider implementations and is reachable through explicit `[search].backend = "builtin"`, and for `websearch`/`webfetch` through configured fallback semantics where supported.

Its current module documentation already says new provider work belongs in eggsearch, but the existence of direct `codesearch` and direct research-provider clients contradicts that ownership statement.

The legacy fallback still has compatibility value and is not required to be deleted in M002.

### 3.4 Duplicate consequences

The current split creates several avoidable differences:

- different credential discovery rules;
- different provider selection/fallback behavior;
- different request timeouts and client construction;
- different provenance/trust metadata;
- different keyless behavior;
- different failure messages;
- duplicate provider-specific URL/request/response maintenance;
- more opportunity for upstream provider API drift;
- more conceptual tool-surface overlap for the model.

## 4. Invariants that must not regress

- M001's accepted eggsearch request-contract mappings remain intact.
- Eggsearch remains the default external search backend.
- `fallback_to_builtin` remains false by default.
- Raw eggsearch MCP tools remain hidden by default.
- Local `grep`, `glob`, LSP, Git, filesystem read, and local workspace evidence remain native CodeGG capabilities.
- Removing direct provider clients must not remove CodeGG's higher-level research synthesis, claim, verification, artifact, or report behavior.
- Removing direct provider clients must not force baseline users to configure Exa/Tavily/Brave/SerpAPI/Kagi credentials.
- No new CodeGG provider-specific search client may replace the deleted ones.
- The legacy `src/search/*` backend remains explicitly secondary/compatibility-only unless a separate removal plan is accepted.
- Search errors retain clear backend/provenance identity.
- No background search process, crawler, or unbounded fan-out is introduced.

## 5. Scope

### In scope

- `src/tool/codesearch.rs` and its registry/config/documentation references;
- `src/research/sources/search_provider.rs` and related research source registration/configuration;
- the narrow research collection boundary needed to use eggsearch for external evidence;
- `src/search/*` documentation/registration boundaries necessary to make it explicitly compatibility-only;
- tool descriptions/prompts that currently direct the model toward overlapping external-search tools;
- unused dependencies/environment plumbing made unnecessary by direct-client deletion;
- focused tests proving external-search execution flows through eggsearch.

### Explicitly out of scope

- rewriting research claim construction, extraction, synthesis, verification, storage, or rendering;
- deleting local-repository research sources;
- deleting `src/search/*` entirely;
- replacing general `reqwest` usage elsewhere in CodeGG;
- changing non-search provider architecture or LLM provider clients;
- adding new eggsearch providers from CodeGG;
- private-repository credential UX unless required by an already-supported eggsearch path;
- adding a search index/cache/database;
- a generic HTTP-client unification project;
- new CI/static scanning infrastructure;
- release automation.

## 6. Required production changes

### Core/domain

Establish one explicit internal rule: all external search/evidence discovery requests leave CodeGG through `search_backend` / eggsearch, except when the user explicitly chooses the documented legacy built-in web backend.

Do not create a new generic `SearchProvider` trait in CodeGG solely to wrap eggsearch; the point is to reduce duplicated provider abstraction, not rename it.

### Storage and migrations

No database migration.

Existing research-run artifacts and historical source records remain readable. Do not rewrite old provider names in persisted historical artifacts.

### Protocol and DTOs

No native daemon protocol change is expected.

If research source collection currently consumes an internal `SourceRecord`, add the smallest adapter that converts eggsearch structured search results into that existing research-source representation. Preserve upstream URL/title/provider/trust/identity metadata where the internal type can carry it. Do not teach research orchestration about Tavily/Brave/Kagi-specific DTOs.

### Runtime and concurrency

#### `codesearch`

Preferred compatibility behavior:

- retain the model-facing `codesearch` name only if existing prompts/config/tests materially rely on it;
- implement it as a thin compatibility alias over the canonical eggsearch repository search path with `profile = "coding"` and appropriate query/result-budget translation;
- attach normal eggsearch provenance/trust behavior;
- do not call Exa directly;
- do not require `EXA_API_KEY` at the CodeGG tool boundary.

If repository evidence shows the alias has no compatibility value and removal is safe, the implementation MAY remove `codesearch` entirely, but the closure record must show why removal does not leave stale prompts, configured tool names, agent profiles, or tests. Default preference is alias-first because it preserves the stable tool name while deleting the duplicate provider owner.

Do not route a `codesearch` alias through the legacy built-in backend. It is a coding/repository evidence compatibility name over eggsearch.

#### Deep research external collection

Replace `SearchProviderSource` provider-specific network execution with an eggsearch-backed external source adapter or equivalent narrow integration.

The external research collector should:

- use `research_search` for broad/multi-source technical research;
- use `repo_search` for repository-specific evidence when the research plan identifies a codebase/repository need;
- use `security_search` for security-specific evidence when the research mode/plan requests it;
- use `web_search` as generic discovery where specialized tools do not apply;
- use explicit fetch tools only for selected URLs/locators, not crawling.

Do not make the research subsystem itself reproduce eggsearch's provider routing logic. It may decide which eggsearch tool class best serves the research task; eggsearch decides which providers satisfy that retrieval request.

Preserve CodeGG's existing network budget semantics. If a research request has `allow_network = false`, it must not call eggsearch live network search simply because the adapter exists.

Preserve local source collection independently from external evidence collection.

### Frontend or operator surface

Update model/tool descriptions so:

- generic external discovery points to `websearch`;
- repository/code discovery points to `repo_search` or the compatibility `codesearch` alias if retained;
- deep evidence discovery points to `research_search` / CodeGG's high-level `research` orchestration as appropriate;
- users are not told to configure Exa/Tavily/Brave/SerpAPI/Kagi directly in CodeGG for baseline search.

### Security and authorization

Deleting direct clients should reduce CodeGG's direct handling of external search credentials.

Do not move credential values into CodeGG config just to feed eggsearch if existing MCP env/config passthrough already covers optional credentials.

Keep user/model query text bounded and untrusted response treatment unchanged.

When research converts eggsearch evidence into internal records, preserve the distinction between remote `external_untrusted` evidence and eggsearch local-workspace provenance. Local provenance is not instruction trust.

### Documentation and static guards

Update:

- `architecture/search_backend.md`;
- research architecture documentation that currently names direct provider adapters;
- `architecture/tool.md`;
- `architecture/native_crates.md` if ownership language is stale;
- `README.md` / config examples if direct search-provider credential instructions remain;
- comments in `src/search/mod.rs` to reference the current `eggstack/eggsearch` repository if stale.

Do not add a permanent source scanner or CI rule that greps for provider domains. The closure record may use targeted source inspection/grep as evidence, but routine verification should remain tests plus review.

## 7. Ordered work packages

### Work package A — Inventory every active external-search caller

Intent:

Prove the deletion/consolidation scope before changing code.

Required changes:

- enumerate calls from tools, research sources, agent prompts, config, and tests that can reach external search-provider HTTP endpoints;
- distinguish LLM/provider HTTP clients and explicit user URL fetches from search-provider execution so unrelated networking is not swept into this milestone;
- identify references to `codesearch`, `SearchProviderSource`, provider enums, provider credential env names, and direct provider endpoint strings.

Acceptance evidence:

- closure record contains a before/after ownership inventory;
- no relevant direct search-provider caller is missed.

### Work package B — Collapse `codesearch` onto eggsearch

Intent:

Delete the direct Exa execution owner while preserving user/model compatibility where justified.

Required changes:

- remove direct Exa request construction and API-key requirement;
- route retained alias behavior through the repaired M001 eggsearch repository search adapter;
- use coding profile/current repo-search fields rather than a hidden Exa-specific request;
- update registry/provenance/tests/descriptions;
- remove Exa-specific code/dependencies only when no other CodeGG feature uses them.

Acceptance evidence:

- invoking retained `codesearch` records an eggsearch MCP call and no Exa HTTP call;
- no `EXA_API_KEY` requirement remains in the `codesearch` path;
- if the tool is removed instead, stale configured/prompt references are absent.

### Work package C — Replace direct research-provider collection

Intent:

Make eggsearch the network evidence collector beneath CodeGG's research orchestration.

Required changes:

- remove provider-specific Tavily/Brave/SerpAPI/Kagi request code from the executable research source path;
- introduce one eggsearch-backed external evidence source boundary using existing search backend/MCP service ownership;
- preserve `allow_network` and source-budget semantics;
- preserve local source adapters;
- convert eggsearch result/evidence fields into existing research records without inventing provider-specific branches.

Acceptance evidence:

- research with network disabled makes no external search calls;
- a network-enabled test using fake eggsearch evidence flows through the eggsearch adapter and produces expected internal source records;
- provider-specific direct-client tests are removed or replaced by eggsearch integration tests;
- no direct Tavily/Brave/SerpAPI/Kagi endpoint call remains on the executable research collection path.

### Work package D — Fence the legacy built-in backend

Intent:

Retain compatibility without presenting two primary architectures.

Required changes:

- ensure built-in search is selected only by explicit backend configuration or documented fallback behavior already supported for generic web search/fetch;
- ensure specialized repo/security/research/evidence tools do not opportunistically route to legacy providers;
- keep docs explicit that no new provider work belongs under `src/search/*`;
- fix stale upstream repository references.

Acceptance evidence:

- default config does not invoke legacy providers;
- `backend = "builtin"` focused compatibility tests remain green for the intentionally retained generic paths;
- no new fallback behavior is introduced for specialized tools.

### Work package E — Remove dead provider plumbing and simplify dependencies

Intent:

Realize maintainability/footprint benefit from single ownership.

Required changes:

- remove dead research provider enum/client/request/response code;
- remove direct Exa code client code;
- remove provider-specific env/config helpers that are now unused by CodeGG search execution;
- remove dependencies only when repository-wide usage proves they are no longer required.

Acceptance evidence:

- `cargo check`/Clippy has no dead-code fallout;
- dependency changes are attributable to actual deleted paths, not speculative churn;
- no feature reduction outside the duplicate external search clients.

### Work package F — Documentation and focused closure

Intent:

Make the ownership boundary durable without a new verification apparatus.

Required changes:

- update architecture and user-facing docs;
- run focused tests and quick verification;
- record targeted source inspection proving no active bypass remains.

Acceptance evidence:

- docs identify eggsearch as the external search/provider owner;
- no new CI lane/static guard exists.

## 8. Failure, cancellation, restart, and contention semantics

A retained `codesearch` compatibility alias must fail the same way as the canonical eggsearch backend; it must not fall through to a second direct Exa client when eggsearch is unavailable.

Research network-disabled mode must remain fail-closed with respect to network access: no eggsearch call is permitted when the research budget disallows network.

If eggsearch is unavailable during a network-enabled research run, surface a normal bounded source-collection failure according to existing research error/partial-result semantics. Do not silently switch to direct provider clients.

Cancellation/deadline behavior remains owned by existing research/search execution boundaries. Do not add retries that escape the parent deadline.

MCP process restart/bootstrap semantics are unchanged.

Concurrent research/search callers reuse the existing shared eggsearch service; do not create per-research-run eggsearch processes.

## 9. Compatibility and migration

### `codesearch`

Default migration policy is compatibility alias rather than abrupt removal.

If retained:

- same model-facing name;
- query and result-budget semantics translated to current eggsearch repo search;
- documentation marks it compatibility-oriented and recommends `repo_search` for structured use;
- no provider-specific Exa behavior is promised.

If removed:

- prove there are no built-in agents, model profiles, config examples, tool deferral lists, or user docs that still require the name;
- document the replacement.

### Research configuration

Historical provider-specific research configuration may remain parseable for one compatibility window if other unrelated features use it, but it must not reactivate direct external search execution.

Do not migrate persisted research artifacts.

### Legacy built-in backend

No removal in M002. Explicit `backend = "builtin"` remains the compatibility escape hatch documented by the roadmap.

## 10. Required tests

### Focused unit tests

- retained `codesearch` argument translation into eggsearch coding-profile repo search;
- research external source selection uses eggsearch tool classes rather than provider enums;
- network budget prevents eggsearch external search when disabled;
- eggsearch result -> research `SourceRecord` conversion preserves URL/title/provenance/trust fields available in the current type;
- explicit built-in backend selection remains separate.

### Integration tests

- retained `codesearch` invokes fake eggsearch MCP and not direct HTTP;
- network-enabled research source collection against fake eggsearch produces sources;
- network-disabled research collection performs zero external search calls;
- default registry/search config does not execute legacy providers;
- explicit built-in generic search compatibility remains green.

### Restart and recovery tests

No new restart suite unless implementation changes MCP/search process lifecycle.

### Contention and cancellation tests

No new contention suite unless research integration adds shared mutable state. Reuse existing shared MCP/search state.

### Security and negative tests

- missing optional search-provider API keys do not cause a CodeGG baseline search preflight failure;
- eggsearch-unavailable errors do not trigger a direct provider fallback;
- network-disabled research remains network-disabled;
- external evidence remains untrusted data after conversion into research records.

### Migration and compatibility tests

- retained `codesearch` name, if kept, still resolves through the registry and returns eggsearch-framed/provenance output;
- old built-in backend config remains parseable;
- removed direct research-provider config does not break unrelated config loading.

## 11. Required verification commands

Use actual affected test target names after implementation. Expected minimum:

```bash
cargo fmt --all -- --check
git diff --check
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_legacy -- --test-threads=1
# focused research/tool tests covering the changed source adapter and codesearch alias
scripts/verify.sh quick
```

Targeted closure inspection should also search the final tree for the known direct search-provider endpoints and classify any remaining occurrences as documentation/tests/other non-executable context or a defect. This is closure evidence, not a new permanent CI guard.

Run broader/full verification only if implementation changes shared research runtime behavior beyond source collection or removes dependencies used elsewhere.

## 12. Documentation updates

- `architecture/search_backend.md`: eggsearch is the sole normal external search owner; legacy backend is explicit fallback.
- research architecture docs: external provider collection flows through eggsearch; local/synthesis responsibilities remain CodeGG-owned.
- `architecture/tool.md`: `codesearch` disposition and recommended search tool selection.
- `architecture/native_crates.md`: current eggsearch ownership/repository reference.
- `README.md` / config examples: remove direct provider credential instructions that no longer apply to CodeGG search.
- source comments under `src/search/*`: compatibility-only status and current upstream repo.

## 13. Acceptance criteria

M002 is accepted only when all are true:

1. M001 is strictly/acceptably closed and its request-contract fixes remain present.
2. No normally registered CodeGG tool performs direct Exa Code API execution.
3. If `codesearch` remains, it is a thin eggsearch-backed compatibility alias using current repository/coding search semantics.
4. `codesearch` no longer requires `EXA_API_KEY` / `EXA_CODE_API_KEY` at the CodeGG execution boundary.
5. No executable CodeGG research source directly sends search requests to Tavily, Brave Search, SerpAPI, or Kagi.
6. Network-enabled CodeGG research external evidence collection uses eggsearch; network-disabled research performs no external search call.
7. Local research/source collection and higher-level research synthesis remain available.
8. Default search configuration does not invoke `src/search/*` providers.
9. Explicit documented built-in generic search compatibility remains intact.
10. Specialized eggsearch tools do not silently fall back to legacy providers.
11. Search-provider credential handling is not duplicated/reintroduced in a new CodeGG module.
12. Focused tests and `scripts/verify.sh quick` are green.
13. Final-tree targeted inspection finds no active direct Exa/Tavily/Brave/SerpAPI/Kagi search endpoint path; any remaining strings are explicitly classified.
14. No new CI lane, static scanner, provider abstraction, crawler, search database, or release automation is introduced.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M001 is not accepted or its closure record reports unresolved medium/high request-contract defects;
- repository evidence shows `codesearch` provides a distinct supported capability eggsearch cannot represent without feature reduction;
- replacing research provider clients requires redesigning claim synthesis, persistence, or unrelated research-domain architecture;
- the only proposed replacement creates another CodeGG provider-routing abstraction rather than using eggsearch;
- removing a dependency affects unrelated provider/HTTP functionality outside the milestone;
- network budget/cancellation semantics cannot be preserved through the existing research boundary;
- a durable data migration becomes necessary;
- the scope expands into generic HTTP/provider-client unification.

## 15. Closure evidence required

Create `plans/closure/search-eggsearch-integration/002-status.md` containing:

- accepted implementation commit(s);
- accepted M001 closure dependency;
- before/after external-search ownership inventory;
- exact `codesearch` disposition and compatibility rationale;
- research external-source before/after path;
- list of direct provider client code removed;
- targeted final-tree inspection of Exa/Tavily/Brave/SerpAPI/Kagi search endpoint usage and classification of any remaining matches;
- tests/verification commands and outcomes;
- dependency changes, if any, with justification;
- documentation updates;
- unresolved findings by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- Do not start M002 before M001 closure is accepted.
- Preserve the distinction between external search and LLM provider HTTP clients; this is not a repository-wide networking cleanup.
- Prefer retaining the `codesearch` name as an alias if doing so removes direct Exa ownership without forcing user-facing churn.
- Research orchestration may choose among eggsearch tool classes, but provider selection belongs to eggsearch.
- Preserve `allow_network` exactly.
- Do not convert a cleanup milestone into a new search framework.
- Keep verification narrow and deletion-oriented.
- Preserve unrelated user changes.
